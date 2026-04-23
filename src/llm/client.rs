use crate::config::{LlmConfig, LlmProviderKind};
use crate::tools::{
    from_openai_tool_name, is_valid_openai_tool_name, to_openai_tool_name,
};
use crate::types::tool::ToolCall;
use anyhow::{Context, Result};
use reqwest::Client;
use serde_json::json;
use std::collections::HashMap;

pub struct LlmClient {
    client: Client,
    config: LlmConfig,
}

pub enum LlmResponse {
    /// LLM 决定调用工具（含 tool_use_id 用于多轮对话）
    ToolUse {
        tool_call: ToolCall,
        tool_use_id: String,
        /// 完整的 assistant content 数组（存回 messages 用）
        assistant_content: Vec<serde_json::Value>,
    },
    /// LLM 决定直接回复用户（任务完成）
    FinalAnswer(String),
}

impl LlmClient {
    pub fn new(config: LlmConfig) -> Self {
        Self {
            client: Client::new(),
            config,
        }
    }

    /// 发送消息，返回 LLM 响应
    pub async fn chat(
        &self,
        system_prompt: &str,
        messages: &[serde_json::Value],
        tools: &[serde_json::Value],
    ) -> Result<LlmResponse> {
        match self.config.provider_kind {
            LlmProviderKind::Anthropic => {
                self.chat_anthropic(system_prompt, messages, tools).await
            }
            LlmProviderKind::OpenAiCompatible => {
                self.chat_openai(system_prompt, messages, tools).await
            }
        }
    }

    /// Anthropic Messages API 调用
    async fn chat_anthropic(
        &self,
        system_prompt: &str,
        messages: &[serde_json::Value],
        tools: &[serde_json::Value],
    ) -> Result<LlmResponse> {
        let payload = json!({
            "model": self.config.model,
            "max_tokens": 4096,
            "system": system_prompt,
            "tools": tools,
            "messages": messages,
        });

        let resp = self
            .client
            .post(self.config.anthropic_endpoint())
            .header("x-api-key", self.config.api_key())
            .header("anthropic-version", "2023-06-01")
            .header("content-type", "application/json")
            .json(&payload)
            .send()
            .await
            .context("Anthropic API 连接失败")?
            .json::<serde_json::Value>()
            .await
            .context("Anthropic API 响应解析失败")?;

        if let Some(err_type) = resp.get("type").and_then(|t| t.as_str()) {
            if err_type == "error" {
                let msg = resp["error"]["message"].as_str().unwrap_or("未知错误");
                return Err(anyhow::anyhow!("Anthropic API 错误: {}", msg));
            }
        }

        self.parse_anthropic_response(resp)
    }

    /// OpenAI-compatible Chat Completions API 调用
    async fn chat_openai(
        &self,
        system_prompt: &str,
        messages: &[serde_json::Value],
        tools: &[serde_json::Value],
    ) -> Result<LlmResponse> {
        let tool_name_map = self.build_openai_tool_name_map(tools)?;
        let openai_messages = self.convert_messages_to_openai(system_prompt, messages, &tool_name_map)?;
        let openai_tools = self.convert_tools_to_openai(tools, &tool_name_map)?;

        let payload = json!({
            "model": self.config.model,
            "messages": openai_messages,
            "tools": openai_tools,
            "tool_choice": "auto",
        });

        let resp = self
            .client
            .post(self.config.openai_endpoint())
            .header("Authorization", format!("Bearer {}", self.config.api_key()))
            .header("content-type", "application/json")
            .json(&payload)
            .send()
            .await
            .context("OpenAI-compatible API 连接失败")?
            .json::<serde_json::Value>()
            .await
            .context("OpenAI-compatible API 响应解析失败")?;

        if let Some(err_obj) = resp.get("error") {
            let msg = err_obj["message"].as_str().unwrap_or("未知错误");
            return Err(anyhow::anyhow!("OpenAI-compatible API 错误: {}", msg));
        }

        self.parse_openai_response(resp, &tool_name_map)
    }

    fn build_openai_tool_name_map(
        &self,
        tools: &[serde_json::Value],
    ) -> Result<HashMap<String, String>> {
        let mut name_map = HashMap::with_capacity(tools.len());

        for tool in tools {
            let internal_name = tool["name"]
                .as_str()
                .ok_or_else(|| anyhow::anyhow!("工具 schema 缺少 name 字段"))?;
            let openai_name = to_openai_tool_name(internal_name);

            if !is_valid_openai_tool_name(&openai_name) {
                return Err(anyhow::anyhow!(
                    "工具名转换后仍不合法: {} -> {}",
                    internal_name,
                    openai_name
                ));
            }

            if let Some(existing) = name_map.insert(openai_name.clone(), internal_name.to_string()) {
                return Err(anyhow::anyhow!(
                    "OpenAI-compatible 工具名冲突: {} 同时映射到 {} 和 {}",
                    openai_name,
                    existing,
                    internal_name
                ));
            }
        }

        Ok(name_map)
    }

    /// 将 Anthropic 格式 messages 转换为 OpenAI 格式
    fn convert_messages_to_openai(
        &self,
        system_prompt: &str,
        messages: &[serde_json::Value],
        tool_name_map: &HashMap<String, String>,
    ) -> Result<Vec<serde_json::Value>> {
        let mut openai_messages = Vec::with_capacity(messages.len() + 1);
        openai_messages.push(json!({
            "role": "system",
            "content": system_prompt
        }));

        for msg in messages {
            let role = msg["role"].as_str().unwrap_or("");

            match role {
                "user" => {
                    if let Some(content) = msg.get("content") {
                        if let Some(content_arr) = content.as_array() {
                            let mut tool_result_block: Option<&serde_json::Value> = None;
                            let mut text_blocks: Vec<&serde_json::Value> = Vec::new();
                            let mut image_blocks: Vec<&serde_json::Value> = Vec::new();

                            for block in content_arr {
                                match block["type"].as_str() {
                                    Some("tool_result") => {
                                        tool_result_block = Some(block);
                                        break;
                                    }
                                    Some("text") => text_blocks.push(block),
                                    Some("image") => image_blocks.push(block),
                                    _ => {}
                                }
                            }

                            if let Some(block) = tool_result_block {
                                openai_messages.push(json!({
                                    "role": "tool",
                                    "tool_call_id": block["tool_use_id"].as_str().unwrap_or(""),
                                    "content": block["content"].as_str().unwrap_or("")
                                }));
                            } else if !image_blocks.is_empty() {
                                let mut openai_content: Vec<serde_json::Value> = Vec::new();

                                for block in &text_blocks {
                                    openai_content.push(json!({
                                        "type": "text",
                                        "text": block["text"].as_str().unwrap_or("")
                                    }));
                                }

                                for block in &image_blocks {
                                    let source = &block["source"];
                                    openai_content.push(json!({
                                        "type": "image_url",
                                        "image_url": {
                                            "url": format!("data:{};base64,{}",
                                                source["media_type"].as_str().unwrap_or("image/png"),
                                                source["data"].as_str().unwrap_or(""))
                                        }
                                    }));
                                }

                                openai_messages.push(json!({
                                    "role": "user",
                                    "content": openai_content
                                }));
                            } else {
                                let text = text_blocks
                                    .first()
                                    .and_then(|b| b["text"].as_str())
                                    .unwrap_or("");
                                openai_messages.push(json!({
                                    "role": "user",
                                    "content": text
                                }));
                            }
                        } else {
                            openai_messages.push(json!({
                                "role": "user",
                                "content": content
                            }));
                        }
                    }
                }
                "assistant" => {
                    if let Some(content) = msg.get("content") {
                        if let Some(content_arr) = content.as_array() {
                            let mut text = "";
                            let mut tool_calls = Vec::new();
                            for block in content_arr {
                                match block["type"].as_str() {
                                    Some("text") => text = block["text"].as_str().unwrap_or(""),
                                    Some("tool_use") => {
                                        let tool_name = block["name"].as_str().unwrap_or("");
                                        let openai_name = self.resolve_openai_tool_name(tool_name, tool_name_map)?;
                                        tool_calls.push(json!({
                                            "id": block["id"],
                                            "type": "function",
                                            "function": {
                                                "name": openai_name,
                                                "arguments": serde_json::to_string(&block["input"]).unwrap_or_default()
                                            }
                                        }));
                                    }
                                    _ => {}
                                }
                            }
                            if tool_calls.is_empty() {
                                openai_messages.push(json!({
                                    "role": "assistant",
                                    "content": text
                                }));
                            } else {
                                openai_messages.push(json!({
                                    "role": "assistant",
                                    "content": text,
                                    "tool_calls": tool_calls
                                }));
                            }
                        } else {
                            openai_messages.push(msg.clone());
                        }
                    }
                }
                _ => {}
            }
        }

        Ok(openai_messages)
    }

    /// 将 Anthropic 格式 tools 转换为 OpenAI 格式
    fn convert_tools_to_openai(
        &self,
        tools: &[serde_json::Value],
        tool_name_map: &HashMap<String, String>,
    ) -> Result<Vec<serde_json::Value>> {
        tools.iter()
            .map(|tool| {
                let original_name = tool["name"]
                    .as_str()
                    .ok_or_else(|| anyhow::anyhow!("工具 schema 缺少 name 字段"))?;
                let openai_name = self.resolve_openai_tool_name(original_name, tool_name_map)?;
                Ok(json!({
                    "type": "function",
                    "function": {
                        "name": openai_name,
                        "description": tool["description"],
                        "parameters": tool["input_schema"]
                    }
                }))
            })
            .collect()
    }

    fn resolve_openai_tool_name(
        &self,
        internal_name: &str,
        tool_name_map: &HashMap<String, String>,
    ) -> Result<String> {
        let openai_name = to_openai_tool_name(internal_name);
        match tool_name_map.get(&openai_name) {
            Some(mapped_name) if mapped_name == internal_name => Ok(openai_name),
            Some(mapped_name) => Err(anyhow::anyhow!(
                "工具名映射不一致: {} -> {}，但注册表记录为 {}",
                internal_name,
                openai_name,
                mapped_name
            )),
            None => Err(anyhow::anyhow!("未找到工具名映射: {}", internal_name)),
        }
    }

    /// 解析 Anthropic 响应
    fn parse_anthropic_response(&self, resp: serde_json::Value) -> Result<LlmResponse> {
        let stop_reason = resp["stop_reason"].as_str().unwrap_or("");
        let content = resp["content"].as_array().cloned().unwrap_or_default();

        if stop_reason == "tool_use" {
            if let Some(tool_block) = content.iter().find(|b| b["type"] == "tool_use") {
                let tool_use_id = tool_block["id"]
                    .as_str()
                    .ok_or_else(|| anyhow::anyhow!("tool_use block 缺少 'id' 字段"))?
                    .to_string();
                let tool_name = tool_block["name"].as_str().unwrap_or("").to_string();
                let tool_input = tool_block["input"].clone();

                let tool_call = ToolCall {
                    tool: tool_name,
                    args: tool_input,
                    reason: None,
                    dry_run: false,
                };

                return Ok(LlmResponse::ToolUse {
                    tool_call,
                    tool_use_id,
                    assistant_content: content,
                });
            }
        }

        let text = content
            .iter()
            .find(|b| b["type"] == "text")
            .and_then(|b| b["text"].as_str())
            .unwrap_or("操作完成")
            .to_string();

        Ok(LlmResponse::FinalAnswer(text))
    }

    /// 解析 OpenAI-compatible 响应
    fn parse_openai_response(
        &self,
        resp: serde_json::Value,
        tool_name_map: &HashMap<String, String>,
    ) -> Result<LlmResponse> {
        let choices = resp["choices"].as_array().cloned().unwrap_or_default();

        if let Some(choice) = choices.first() {
            let message = &choice["message"];

            if let Some(tool_calls) = message.get("tool_calls").and_then(|tc| tc.as_array()) {
                if let Some(tool_call) = tool_calls.first() {
                    let tool_use_id = tool_call["id"].as_str().unwrap_or("").to_string();
                    let function = &tool_call["function"];
                    let openai_name = function["name"].as_str().unwrap_or("").to_string();
                    let tool_name = tool_name_map
                        .get(&openai_name)
                        .cloned()
                        .unwrap_or_else(|| from_openai_tool_name(&openai_name));

                    let args_str = function["arguments"].as_str().unwrap_or("{}");
                    let tool_input = serde_json::from_str::<serde_json::Value>(args_str)
                        .map_err(|e| anyhow::anyhow!("tool arguments JSON 解析失败: {}", e))?;

                    let tool_call = ToolCall {
                        tool: tool_name.clone(),
                        args: tool_input.clone(),
                        reason: None,
                        dry_run: false,
                    };

                    let assistant_content = vec![
                        json!({
                            "type": "text",
                            "text": message["content"].as_str().unwrap_or("")
                        }),
                        json!({
                            "type": "tool_use",
                            "id": tool_use_id,
                            "name": tool_name,
                            "input": tool_input
                        })
                    ];

                    return Ok(LlmResponse::ToolUse {
                        tool_call,
                        tool_use_id,
                        assistant_content,
                    });
                }
            }

            let text = message["content"].as_str().unwrap_or("操作完成").to_string();
            return Ok(LlmResponse::FinalAnswer(text));
        }

        Err(anyhow::anyhow!("OpenAI 响应格式异常：无 choices"))
    }
}
