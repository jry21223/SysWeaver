use anyhow::Result;
use reqwest::Client;
use serde_json::json;
use crate::types::tool::ToolCall;

pub struct LlmClient {
    client: Client,
    api_key: String,
    model: String,
    base_url: String,
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
    pub fn new(api_key: String) -> Self {
        Self {
            client: Client::new(),
            api_key,
            model: "claude-sonnet-4-5".to_string(),
            base_url: "https://api.anthropic.com".to_string(),
        }
    }

    /// 发送消息，返回 LLM 响应
    pub async fn chat(
        &self,
        system_prompt: &str,
        messages: &[serde_json::Value],
        tools: &[serde_json::Value],
    ) -> Result<LlmResponse> {
        let payload = json!({
            "model": self.model,
            "max_tokens": 4096,
            "system": system_prompt,
            "tools": tools,
            "messages": messages,
        });

        let resp = self.client
            .post(format!("{}/v1/messages", self.base_url))
            .header("x-api-key", &self.api_key)
            .header("anthropic-version", "2023-06-01")
            .header("content-type", "application/json")
            .json(&payload)
            .send()
            .await?
            .json::<serde_json::Value>()
            .await?;

        // 检查 API 错误
        if let Some(err_type) = resp.get("type").and_then(|t| t.as_str()) {
            if err_type == "error" {
                let msg = resp["error"]["message"].as_str().unwrap_or("未知错误");
                return Err(anyhow::anyhow!("Claude API 错误: {}", msg));
            }
        }

        self.parse_response(resp)
    }

    fn parse_response(&self, resp: serde_json::Value) -> Result<LlmResponse> {
        let stop_reason = resp["stop_reason"].as_str().unwrap_or("");
        let content = resp["content"].as_array()
            .cloned()
            .unwrap_or_default();

        if stop_reason == "tool_use" {
            // 找到第一个 tool_use 块
            if let Some(tool_block) = content.iter().find(|b| b["type"] == "tool_use") {
                let tool_use_id = tool_block["id"].as_str()
                    .unwrap_or("unknown_id")
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

        // LLM 直接回复文本
        let text = content.iter()
            .find(|b| b["type"] == "text")
            .and_then(|b| b["text"].as_str())
            .unwrap_or("操作完成")
            .to_string();

        Ok(LlmResponse::FinalAnswer(text))
    }
}
