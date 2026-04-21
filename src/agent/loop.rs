use anyhow::Result;
use std::io::{self, Write};
use uuid::Uuid;

use crate::agent::memory::{Memory, SystemContext};
use crate::config::LlmConfig;
use crate::context::system_scan;
use crate::llm::client::{LlmClient, LlmResponse};
use crate::llm::prompt::build_system_prompt;
use crate::safety::audit::AuditLogger;
use crate::safety::classifier::RiskClassifier;
use crate::tools::ToolManager;
use crate::types::risk::RiskLevel;
use crate::types::tool::ToolCall;

/// Agent 核心循环：ReAct（Reason → Act → Observe）模式
pub struct AgentLoop {
    llm: LlmClient,
    tools: ToolManager,
    classifier: RiskClassifier,
    audit: AuditLogger,
    pub memory: Memory,
    /// 运行模式：safe | normal | auto
    mode: String,
    /// 最大执行步数（防止死循环）
    max_steps: usize,
}

impl AgentLoop {
    pub fn new(llm_config: LlmConfig, mode: &str, ctx: SystemContext) -> Self {
        let session_id = Uuid::new_v4().to_string();
        let mut memory = Memory::new();
        memory.system_context = Some(ctx);

        Self {
            llm: LlmClient::new(llm_config),
            tools: ToolManager::new(),
            classifier: RiskClassifier::new(),
            audit: AuditLogger::new(&session_id),
            memory,
            mode: mode.to_string(),
            max_steps: 15,
        }
    }

    /// 执行一条用户指令，返回最终回复文本
    pub async fn run(&mut self, user_input: &str, dry_run: bool) -> Result<String> {
        self.memory.push_user_text(user_input);

        let system_prompt = build_system_prompt(self.memory.system_context.as_ref(), &self.tools);
        let tool_schemas = self.tools.all_schemas();

        for step in 0..self.max_steps {
            let messages = self.memory.build_llm_messages();

            // 调用 LLM
            let response = match self
                .llm
                .chat(&system_prompt, &messages, &tool_schemas)
                .await
            {
                Ok(r) => r,
                Err(e) => {
                    let err_msg = format!("LLM 调用失败: {}", e);
                    eprintln!("❌ {}", err_msg);
                    return Err(e);
                }
            };

            match response {
                LlmResponse::FinalAnswer(text) => {
                    self.memory.push_assistant_text(&text);
                    return Ok(text);
                }

                LlmResponse::ToolUse {
                    mut tool_call,
                    tool_use_id,
                    assistant_content,
                } => {
                    // 用户指定 dry_run 时全局强制
                    if dry_run {
                        tool_call.dry_run = true;
                    }

                    // 打印工具调用信息（让用户看到 Agent 在做什么）
                    self.print_tool_call_info(step + 1, &tool_call);

                    // 保存 assistant 的 tool_use 消息到历史
                    self.memory.push_raw_message(serde_json::json!({
                        "role": "assistant",
                        "content": assistant_content,
                    }));

                    // 风险评估
                    let risk = self.classifier.assess(&tool_call);

                    let tool_result = if risk.level.is_blocked() {
                        // CRITICAL：直接拒绝，记录审计，反馈给 LLM
                        self.audit.log_blocked(user_input, &tool_call, &risk.reason);
                        let rejection = format!(
                            "🚨 操作已被强制阻止（CRITICAL 级别）\n原因：{}\n影响：{}\n{}",
                            risk.reason,
                            risk.impact,
                            risk.alternative.as_deref().unwrap_or(""),
                        );
                        println!("\n{}", rejection);
                        self.memory.push_tool_result(&tool_use_id, &rejection, true);
                        continue;
                    } else if risk.level == RiskLevel::High {
                        // HIGH：需要用户确认
                        let confirmed = self.prompt_high_risk_confirmation(&tool_call, &risk)?;
                        self.audit.log_operation(
                            user_input,
                            &tool_call,
                            &risk.level,
                            confirmed,
                            None,
                        );

                        if !confirmed {
                            let msg = "用户取消了此操作".to_string();
                            println!("⛔ 操作已取消");
                            self.memory.push_tool_result(&tool_use_id, &msg, false);
                            continue;
                        }
                        // 执行
                        let result = self.tools.dispatch(&tool_call).await?;
                        self.audit.log_operation(
                            user_input,
                            &tool_call,
                            &risk.level,
                            true,
                            Some(&result),
                        );
                        result
                    } else if risk.level == RiskLevel::Medium && self.mode == "safe" {
                        // safe 模式下，Medium 风险也需确认
                        let confirmed = self.prompt_medium_risk_confirmation(&tool_call)?;
                        self.audit.log_operation(
                            user_input,
                            &tool_call,
                            &risk.level,
                            confirmed,
                            None,
                        );

                        if !confirmed {
                            let msg = "用户取消了此操作".to_string();
                            println!("⛔ 操作已取消");
                            self.memory.push_tool_result(&tool_use_id, &msg, false);
                            continue;
                        }
                        let result = self.tools.dispatch(&tool_call).await?;
                        self.audit.log_operation(
                            user_input,
                            &tool_call,
                            &risk.level,
                            true,
                            Some(&result),
                        );
                        result
                    } else {
                        // Safe / Low / Medium(非safe模式)：直接执行
                        let result = self.tools.dispatch(&tool_call).await?;
                        self.audit.log_operation(
                            user_input,
                            &tool_call,
                            &risk.level,
                            false,
                            Some(&result),
                        );
                        result
                    };

                    // 记录操作
                    self.memory.record_operation(tool_call, &tool_result, None);

                    // 检查是否需要刷新系统状态（多轮对话后更新环境）
                    if self.memory.needs_refresh() {
                        let new_ctx = system_scan::scan().await;
                        self.memory.refresh_system_context(new_ctx);
                        tracing::debug!("系统状态已刷新");
                    }

                    // 构建工具结果内容反馈给 LLM
                    let result_content = self.format_tool_result(&tool_result);
                    self.print_tool_result_info(&tool_result);

                    self.memory.push_tool_result(
                        &tool_use_id,
                        &result_content,
                        !tool_result.success,
                    );
                }
            }
        }

        Ok(format!(
            "⚠️  已达到最大执行步数限制（{}步），任务可能未完全完成。请重新描述您的需求。",
            self.max_steps
        ))
    }

    /// 展示 HIGH 风险确认对话框
    fn prompt_high_risk_confirmation(
        &self,
        call: &ToolCall,
        risk: &crate::types::risk::RiskAssessment,
    ) -> Result<bool> {
        println!("\n{}", "═".repeat(60));
        println!("⚠️   高风险操作 — 需要您确认");
        println!("{}", "═".repeat(60));
        println!("工具：{}", call.tool);
        println!(
            "参数：{}",
            serde_json::to_string_pretty(&call.args).unwrap_or_default()
        );
        println!("风险：{}", risk.reason);
        println!("影响：{}", risk.impact);
        if let Some(alt) = &risk.alternative {
            println!("建议：{}", alt);
        }
        println!("{}", "═".repeat(60));
        print!("输入 'yes' 确认执行，其他任意键取消 › ");
        io::stdout().flush()?;

        let mut input = String::new();
        io::stdin().read_line(&mut input)?;
        Ok(input.trim().to_lowercase() == "yes")
    }

    /// safe 模式下 Medium 风险的简化确认
    fn prompt_medium_risk_confirmation(&self, call: &ToolCall) -> Result<bool> {
        println!(
            "\n🟡 [safe 模式] 此操作会修改系统状态：{} {:?}",
            call.tool, call.args
        );
        print!("确认执行？(yes/no) › ");
        io::stdout().flush()?;

        let mut input = String::new();
        io::stdin().read_line(&mut input)?;
        Ok(input.trim().to_lowercase() == "yes")
    }

    /// 打印工具调用信息
    fn print_tool_call_info(&self, step: usize, call: &ToolCall) {
        let prefix = if call.dry_run { "[DRY-RUN] " } else { "" };
        println!(
            "\n🔧 Step {}: {}{}({})",
            step,
            prefix,
            call.tool,
            serde_json::to_string(&call.args).unwrap_or_default()
        );
    }

    /// 打印工具结果摘要
    fn print_tool_result_info(&self, result: &crate::types::tool::ToolResult) {
        if let Some(preview) = &result.dry_run_preview {
            println!("   📋 预览：{}", preview);
        } else if result.success {
            let preview = result.stdout.lines().next().unwrap_or("（无输出）");
            println!("   ✅ 成功：{}", preview);
        } else {
            println!(
                "   ❌ 失败 (exit {})：{}",
                result.exit_code,
                result.stderr.lines().next().unwrap_or("")
            );
        }
    }

    /// 格式化工具结果为 LLM 可读文本
    fn format_tool_result(&self, result: &crate::types::tool::ToolResult) -> String {
        if let Some(preview) = &result.dry_run_preview {
            return format!("[DRY-RUN 预览]\n{}", preview);
        }
        if result.success {
            if result.stdout.is_empty() {
                "操作成功（无输出）".to_string()
            } else {
                // 限制输出长度避免消耗过多 token（按字符截断，防止 UTF-8 边界 panic）
                let out = &result.stdout;
                let char_count = out.chars().count();
                if char_count > 4000 {
                    let truncated: String = out.chars().take(4000).collect();
                    format!("{}\n\n[...输出已截断，共 {} 字符]", truncated, char_count)
                } else {
                    out.clone()
                }
            }
        } else {
            format!(
                "操作失败 (exit code: {})\nstderr: {}",
                result.exit_code, result.stderr
            )
        }
    }
}
