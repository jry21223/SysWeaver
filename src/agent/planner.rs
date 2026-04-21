use crate::config::LlmConfig;
use crate::llm::client::LlmClient;
use crate::llm::prompt::build_planner_prompt;
use anyhow::Result;
use serde_json::json;

/// 任务规划器：使用 LLM 分析任务并生成执行计划
/// Phase 4 功能：可用于前置任务分解和消歧，当前 AgentLoop 的 ReAct 循环已能处理多步任务
#[allow(dead_code)]
pub struct Planner {
    llm: LlmClient,
}

#[allow(dead_code)]
#[derive(Debug, Clone)]
pub enum TaskPlan {
    /// 单步任务，直接进入 Agent Loop
    Single { description: String },
    /// 多步任务，包含预估步骤
    Multi {
        description: String,
        estimated_steps: Vec<String>,
    },
    /// 模糊任务，需要向用户消歧
    Ambiguous { options: Vec<DisambiguationOption> },
}

#[allow(dead_code)]
#[derive(Debug, Clone)]
pub struct DisambiguationOption {
    pub label: String,
    pub description: String,
    pub preview: String,
}

impl Planner {
    pub fn new(llm_config: LlmConfig) -> Self {
        Self {
            llm: LlmClient::new(llm_config),
        }
    }

    /// 使用 LLM 分析用户输入，返回任务计划
    pub async fn analyze(&self, input: &str, system_context: &str) -> Result<TaskPlan> {
        let system_prompt = build_planner_prompt(system_context);

        let messages = vec![json!({
            "role": "user",
            "content": input
        })];

        // Planner 不使用 tools，直接返回结构化 JSON
        let resp = self.llm.chat(&system_prompt, &messages, &[]).await?;

        // 解析 LLM 返回的计划
        match resp {
            crate::llm::client::LlmResponse::FinalAnswer(text) => {
                self.parse_plan_response(&text)
            }
            crate::llm::client::LlmResponse::ToolUse { .. } => {
                // Planner 不应该调用工具，返回单步
                Ok(TaskPlan::Single {
                    description: input.to_string(),
                })
            }
        }
    }

    /// 解析 LLM 返回的计划 JSON
    fn parse_plan_response(&self, text: &str) -> Result<TaskPlan> {
        // 尝试从文本中提取 JSON
        let json_str = text
            .trim()
            .lines()
            .skip_while(|line| !line.trim().starts_with('{'))
            .take_while(|line| !line.trim().starts_with('}') || line.trim() == "}")
            .collect::<String>();

        if json_str.is_empty() {
            // 无法解析，返回单步
            return Ok(TaskPlan::Single {
                description: text.to_string(),
            });
        }

        let plan_json: serde_json::Value = serde_json::from_str(&json_str)
            .map_err(|e| anyhow::anyhow!("计划 JSON 解析失败: {}", e))?;

        let plan_type = plan_json["type"].as_str().unwrap_or("single");

        match plan_type {
            "single" => Ok(TaskPlan::Single {
                description: plan_json["description"].as_str().unwrap_or(text).to_string(),
            }),
            "multi" => {
                let steps = plan_json["steps"]
                    .as_array()
                    .map(|arr| arr.iter().filter_map(|s| s.as_str().map(String::from)).collect())
                    .unwrap_or_default();
                Ok(TaskPlan::Multi {
                    description: plan_json["description"].as_str().unwrap_or(text).to_string(),
                    estimated_steps: steps,
                })
            }
            "ambiguous" => {
                let options = plan_json["options"]
                    .as_array()
                    .map(|arr| {
                        arr.iter()
                            .filter_map(|opt| {
                                Some(DisambiguationOption {
                                    label: opt["label"].as_str()?.to_string(),
                                    description: opt["description"].as_str()?.to_string(),
                                    preview: opt["preview"].as_str().unwrap_or("").to_string(),
                                })
                            })
                            .collect()
                    })
                    .unwrap_or_default();
                Ok(TaskPlan::Ambiguous { options })
            }
            _ => Ok(TaskPlan::Single {
                description: text.to_string(),
            }),
        }
    }
}
