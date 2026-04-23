use crate::config::LlmConfig;
use crate::llm::client::LlmClient;
use crate::llm::prompt::build_planner_prompt;
use anyhow::{Result, anyhow};
use serde_json::json;

/// 任务规划器：使用 LLM 分析任务并生成执行计划
/// Phase 4 功能：可用于前置任务分解和消歧，当前 AgentLoop 的 ReAct 循环已能处理多步任务
pub struct Planner {
    llm: LlmClient,
}

#[derive(Debug, Clone, PartialEq, Eq)]
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

#[derive(Debug, Clone, PartialEq, Eq)]
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

        let resp = self.llm.chat(&system_prompt, &messages, &[]).await?;

        match resp {
            crate::llm::client::LlmResponse::FinalAnswer(text) => parse_plan_response(&text),
            crate::llm::client::LlmResponse::ToolUse { .. } => Ok(TaskPlan::Single {
                description: input.to_string(),
            }),
        }
    }
}

pub fn parse_plan_response(text: &str) -> Result<TaskPlan> {
    let json_str = extract_json_object(text).ok_or_else(|| anyhow!("计划 JSON 解析失败: 未找到 JSON 对象"))?;
    let plan_json: serde_json::Value = serde_json::from_str(&json_str)
        .map_err(|e| anyhow!("计划 JSON 解析失败: {}", e))?;

    let plan_type = plan_json["type"].as_str().unwrap_or("single");

    match plan_type {
        "single" => Ok(TaskPlan::Single {
            description: plan_json["description"]
                .as_str()
                .filter(|value| !value.trim().is_empty())
                .unwrap_or(text)
                .to_string(),
        }),
        "multi" => {
            let description = plan_json["description"]
                .as_str()
                .filter(|value| !value.trim().is_empty())
                .ok_or_else(|| anyhow!("计划 JSON 解析失败: multi 类型缺少 description"))?
                .to_string();
            let steps = plan_json["steps"]
                .as_array()
                .ok_or_else(|| anyhow!("计划 JSON 解析失败: multi 类型缺少 steps 数组"))?
                .iter()
                .map(|step| {
                    step.as_str()
                        .filter(|value| !value.trim().is_empty())
                        .map(ToString::to_string)
                        .ok_or_else(|| anyhow!("计划 JSON 解析失败: steps 必须为非空字符串"))
                })
                .collect::<Result<Vec<_>>>()?;

            Ok(TaskPlan::Multi {
                description,
                estimated_steps: steps,
            })
        }
        "ambiguous" => {
            let options = plan_json["options"]
                .as_array()
                .ok_or_else(|| anyhow!("计划 JSON 解析失败: ambiguous 类型缺少 options 数组"))?
                .iter()
                .map(|opt| {
                    let label = opt["label"]
                        .as_str()
                        .filter(|value| !value.trim().is_empty())
                        .ok_or_else(|| anyhow!("计划 JSON 解析失败: option 缺少 label"))?;
                    let description = opt["description"]
                        .as_str()
                        .filter(|value| !value.trim().is_empty())
                        .ok_or_else(|| anyhow!("计划 JSON 解析失败: option 缺少 description"))?;
                    let preview = opt["preview"].as_str().unwrap_or("");

                    Ok(DisambiguationOption {
                        label: label.to_string(),
                        description: description.to_string(),
                        preview: preview.to_string(),
                    })
                })
                .collect::<Result<Vec<_>>>()?;

            Ok(TaskPlan::Ambiguous { options })
        }
        _ => Ok(TaskPlan::Single {
            description: text.to_string(),
        }),
    }
}

fn extract_json_object(text: &str) -> Option<String> {
    let start = text.find('{')?;
    let mut depth = 0usize;

    for (offset, ch) in text[start..].char_indices() {
        match ch {
            '{' => depth += 1,
            '}' => {
                depth = depth.saturating_sub(1);
                if depth == 0 {
                    return Some(text[start..start + offset + ch.len_utf8()].to_string());
                }
            }
            _ => {}
        }
    }

    None
}

#[cfg(test)]
mod tests {
    use super::{
        DisambiguationOption, TaskPlan, extract_json_object, parse_plan_response,
    };

    #[test]
    fn parses_single_plan() {
        let plan = parse_plan_response(r#"{"type":"single","description":"查看磁盘"}"#).unwrap();
        assert_eq!(plan, TaskPlan::Single {
            description: "查看磁盘".to_string()
        });
    }

    #[test]
    fn parses_multi_plan_with_steps_in_order() {
        let plan = parse_plan_response(
            r#"{"type":"multi","description":"安装并启动 nginx","steps":["安装 nginx","启动 nginx","验证状态"]}"#,
        )
        .unwrap();

        assert_eq!(
            plan,
            TaskPlan::Multi {
                description: "安装并启动 nginx".to_string(),
                estimated_steps: vec![
                    "安装 nginx".to_string(),
                    "启动 nginx".to_string(),
                    "验证状态".to_string()
                ]
            }
        );
    }

    #[test]
    fn parses_ambiguous_plan() {
        let plan = parse_plan_response(
            r#"{"type":"ambiguous","options":[{"label":"A","description":"清理 /tmp","preview":"安全"}]}"#,
        )
        .unwrap();

        assert_eq!(
            plan,
            TaskPlan::Ambiguous {
                options: vec![DisambiguationOption {
                    label: "A".to_string(),
                    description: "清理 /tmp".to_string(),
                    preview: "安全".to_string(),
                }]
            }
        );
    }

    #[test]
    fn parses_json_inside_code_fence() {
        let plan = parse_plan_response(
            "```json\n{\"type\":\"single\",\"description\":\"查看内存\"}\n```",
        )
        .unwrap();

        assert_eq!(plan, TaskPlan::Single {
            description: "查看内存".to_string()
        });
    }

    #[test]
    fn rejects_invalid_json() {
        assert!(parse_plan_response("not json").is_err());
    }

    #[test]
    fn rejects_multi_without_steps() {
        assert!(parse_plan_response(r#"{"type":"multi","description":"x"}"#).is_err());
    }

    #[test]
    fn extracts_balanced_json_object() {
        let json = extract_json_object("before {\"type\":\"single\",\"nested\":{\"a\":1}} after").unwrap();
        assert_eq!(json, "{\"type\":\"single\",\"nested\":{\"a\":1}}");
    }
}
