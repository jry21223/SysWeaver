use anyhow::{Result, anyhow};
use inquire::Confirm;
use serde_json::json;
use std::io::{self, Write};
use tokio::sync::{mpsc, oneshot};
use uuid::Uuid;

use crate::agent::memory::{Memory, SystemContext};
use crate::config::LlmConfig;
use crate::context::system_scan;
use crate::executor::ssh::SshConfig;
use crate::image::{ImageInfo, ImageSecurityScanner};
use crate::llm::client::{LlmClient, LlmResponse};
use crate::llm::prompt::build_system_prompt;
use crate::safety::audit::AuditLogger;
use crate::safety::classifier::RiskClassifier;
use crate::tools::ToolManager;
use crate::types::risk::RiskLevel;
use crate::types::tool::{RollbackPlan, ToolCall, ToolResult};
use crate::ui::AgentEvent;

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
    /// TUI 事件发送器（None = CLI 模式）
    tui_tx: Option<mpsc::Sender<AgentEvent>>,
}

impl AgentLoop {
    /// CLI 模式构造（保持向后兼容）
    pub fn new(llm_config: LlmConfig, mode: &str, ctx: SystemContext) -> Self {
        Self::new_inner(llm_config, mode, ctx, None, None)
    }

    /// CLI 模式构造（SSH 远程模式）
    pub fn new_with_ssh(llm_config: LlmConfig, mode: &str, ctx: SystemContext, ssh: SshConfig) -> Self {
        Self::new_inner(llm_config, mode, ctx, None, Some(ssh))
    }

    /// TUI 模式构造（接入事件 channel）
    pub fn new_with_tui(
        llm_config: LlmConfig,
        mode: &str,
        ctx: SystemContext,
        tui_tx: mpsc::Sender<AgentEvent>,
    ) -> Self {
        Self::new_inner(llm_config, mode, ctx, Some(tui_tx), None)
    }

    /// TUI + SSH 模式
    pub fn new_with_tui_and_ssh(
        llm_config: LlmConfig,
        mode: &str,
        ctx: SystemContext,
        tui_tx: mpsc::Sender<AgentEvent>,
        ssh: SshConfig,
    ) -> Self {
        Self::new_inner(llm_config, mode, ctx, Some(tui_tx), Some(ssh))
    }

    fn new_inner(
        llm_config: LlmConfig,
        mode: &str,
        ctx: SystemContext,
        tui_tx: Option<mpsc::Sender<AgentEvent>>,
        ssh: Option<SshConfig>,
    ) -> Self {
        let session_id = Uuid::new_v4().to_string();
        let mut memory = Memory::new();
        memory.system_context = Some(ctx);
        let tools = match ssh {
            Some(cfg) => ToolManager::with_ssh(cfg),
            None => ToolManager::new(),
        };
        Self {
            llm: LlmClient::new(llm_config),
            tools,
            classifier: RiskClassifier::new(),
            audit: AuditLogger::new(&session_id),
            memory,
            mode: mode.to_string(),
            max_steps: 15,
            tui_tx,
        }
    }

    /// 向 TUI 发送事件（CLI 模式下 no-op）
    async fn emit(&self, event: AgentEvent) {
        if let Some(tx) = &self.tui_tx {
            let _ = tx.send(event).await;
        }
    }

    /// 获取操作历史摘要（供 /history 命令使用）
    pub fn get_history_summary(&self) -> String {
        let ops = &self.memory.operations;
        if ops.is_empty() {
            return "📜 暂无操作记录".to_string();
        }
        let lines: Vec<String> = ops
            .iter()
            .rev()
            .take(10)
            .enumerate()
            .map(|(i, op)| {
                let ok = op.result.as_ref().map(|r| r.success).unwrap_or(false);
                let icon = if ok { "✅" } else { "❌" };
                let undo = if op.rollback.is_some() { "（可撤销）" } else { "" };
                format!("  {}. {} {} {}", i + 1, icon, op.tool_call.tool, undo)
            })
            .collect();
        format!("📜 最近操作：\n{}", lines.join("\n"))
    }

    /// 执行一条用户指令，返回最终回复文本
    pub async fn run(&mut self, user_input: &str, dry_run: bool) -> Result<String> {
        self.memory.push_user_text(user_input);
        self.run_inner(user_input, dry_run).await
    }

    pub async fn undo_last(&mut self) -> Result<String> {
        let rollback = self
            .memory
            .last_undoable()
            .and_then(|op| op.rollback.clone())
            .ok_or_else(|| anyhow!("没有可撤销的操作"))?;

        if rollback.commands.is_empty() {
            return Ok(format!(
                "⚠️  {}\n当前回滚计划需要手动处理，无法自动执行。",
                rollback.description
            ));
        }

        let mut summaries = Vec::new();
        summaries.push(format!("↩️  撤销：{}", rollback.description));
        if rollback.has_side_effects {
            summaries.push("注意：此回滚操作可能有副作用".to_string());
        }

        for command in &rollback.commands {
            let tool_call = ToolCall {
                tool: "shell.exec".to_string(),
                args: json!({ "command": command }),
                reason: Some("执行回滚计划".to_string()),
                dry_run: false,
            };
            let result = self.execute_tool_call("undo", tool_call, false).await?;

            if result.success {
                let line = result.stdout.lines().next().unwrap_or("（无输出）");
                summaries.push(format!("✅ {}", line));
            } else {
                let line = result.stderr.lines().next().unwrap_or("（无错误输出）");
                summaries.push(format!("❌ {}", line));
                break;
            }
        }

        Ok(summaries.join("\n"))
    }

    async fn execute_tool_call(
        &mut self,
        user_input: &str,
        tool_call: ToolCall,
        record_rollback: bool,
    ) -> Result<ToolResult> {
        let risk = self.classifier.assess(&tool_call);

        if risk.level.is_blocked() {
            self.audit.log_blocked(user_input, &tool_call, &risk.reason);
            let rejection_msg = format!(
                "操作已被强制阻止（CRITICAL 级别）: {}；{}",
                risk.reason, risk.impact
            );
            // CLI 模式下立即打印拒绝原因（TUI 模式通过 emit 传递）
            if self.tui_tx.is_none() {
                println!("\n🚫 【CRITICAL 危险操作已拒绝】");
                println!("   原因: {}", risk.reason);
                println!("   影响: {}", risk.impact);
                if let Some(alt) = &risk.alternative {
                    println!("   建议: {}", alt);
                }
            }
            return Ok(ToolResult::failure(
                &tool_call.tool,
                &rejection_msg,
                -1,
            ));
        }

        if risk.level == RiskLevel::High {
            let confirmed = self.prompt_high_risk_confirmation(&tool_call, &risk).await?;
            self.audit
                .log_operation(user_input, &tool_call, &risk.level, confirmed, None);
            if !confirmed {
                return Ok(ToolResult::failure(&tool_call.tool, "用户取消了此操作", -1));
            }
        } else if risk.level == RiskLevel::Medium && self.mode == "safe" {
            let confirmed = self.prompt_medium_risk_confirmation(&tool_call).await?;
            self.audit
                .log_operation(user_input, &tool_call, &risk.level, confirmed, None);
            if !confirmed {
                return Ok(ToolResult::failure(&tool_call.tool, "用户取消了此操作", -1));
            }
        }

        let result = self.tools.dispatch(&tool_call).await?;
        self.audit
            .log_operation(user_input, &tool_call, &risk.level, false, Some(&result));

        let rollback = if record_rollback {
            self.generate_rollback_plan(&tool_call, &result)
        } else {
            None
        };
        self.memory.record_operation(tool_call, &result, rollback);

        if self.memory.needs_refresh() {
            let new_ctx = system_scan::scan().await;
            let anomalies = system_scan::detect_anomalies(&new_ctx);
            self.memory.refresh_system_context(new_ctx.clone());

            // 检测到异常时，通过事件通道发出告警
            if !anomalies.is_empty() {
                let warning = format!(
                    "⚠️ 系统状态更新警告：检测到以下异常情况，请在后续操作中注意：\n{}",
                    anomalies.iter().map(|a| format!("  - {}", a)).collect::<Vec<_>>().join("\n")
                );
                self.emit(AgentEvent::WatchdogAlert {
                    severity: "⚠️ WARNING".to_string(),
                    message: warning,
                }).await;
            }

            self.emit(AgentEvent::SystemUpdate(new_ctx)).await;
            tracing::debug!("系统状态已刷新");
        }

        Ok(result)
    }

    /// 执行一条用户指令（支持图片），返回最终回复文本
    /// 包含图片安全扫描和审计日志
    pub async fn run_with_images(
        &mut self,
        user_input: &str,
        images: &[ImageInfo],
        dry_run: bool,
    ) -> Result<String> {
        // 1. 图片安全扫描
        let scanner = ImageSecurityScanner::new();
        let scans = scanner.scan_batch(images);

        // 2. 显示安全警告
        let user_warning = scanner.build_user_warning(&scans);
        if !user_warning.is_empty() {
            println!("\n{}", user_warning);

            // HIGH 风险图片：询问用户是否继续
            if scans.iter().any(|s| s.risk_level == RiskLevel::High) {
                print!("是否继续处理这些图片？(yes/no) › ");
                io::stdout().flush()?;
                let mut input = String::new();
                io::stdin().read_line(&mut input)?;
                if input.trim().to_lowercase() != "yes" {
                    return Ok("已取消：图片存在安全风险，拒绝处理。".to_string());
                }
            }
        }

        // 3. 记录审计日志
        for (i, (img, scan)) in images.iter().zip(scans.iter()).enumerate() {
            let mut record = crate::image::ImageAuditRecord::new(img, scan.clone());
            record.set_user_decision(if scan.risk_level == RiskLevel::High {
                "用户确认继续"
            } else {
                "自动通过"
            });
            self.audit.log_custom("image_input", &record.to_json_line());
            tracing::debug!("图片 #{} 安全扫描: {} 风险", i + 1, scan.risk_level.label());
        }

        // 4. 构建带图片的用户消息
        // 将安全提示注入到用户文本中
        let security_prompt = scanner.build_security_prompt(&scans);
        let final_input = if security_prompt.is_empty() {
            user_input.to_string()
        } else {
            format!("{}\n\n{}", security_prompt, user_input)
        };

        self.memory.push_user_message_with_images(&final_input, images);
        self.run_inner(&final_input, dry_run).await
    }

    /// 内部执行逻辑（共享 run 和 run_with_images）
    async fn run_inner(&mut self, user_input: &str, dry_run: bool) -> Result<String> {

        let system_prompt = build_system_prompt(self.memory.system_context.as_ref(), &self.tools);
        let tool_schemas = self.tools.all_schemas();
        let mut completed_tools: Vec<(String, bool)> = Vec::new();

        for step in 0..self.max_steps {
            let messages = self.memory.build_llm_messages();

            // 调用 LLM（失败时最多重试 2 次，指数退避）
            let response = {
                let mut last_err = None;
                let mut resp = None;
                for attempt in 0..3u64 {
                    match self.llm.chat(&system_prompt, &messages, &tool_schemas).await {
                        Ok(r) => {
                            resp = Some(r);
                            break;
                        }
                        Err(e) => {
                            tracing::warn!("LLM 调用失败（第{}次），将重试: {}", attempt + 1, e);
                            last_err = Some(e);
                            if attempt < 2 {
                                tokio::time::sleep(std::time::Duration::from_millis(500 * (attempt + 1))).await;
                            }
                        }
                    }
                }
                match resp {
                    Some(r) => r,
                    None => {
                        let err = last_err.unwrap();
                        let err_msg = format!("LLM 调用失败（已重试3次）: {}", err);
                        eprintln!("❌ {}", err_msg);
                        self.emit(AgentEvent::Error(err_msg)).await;
                        return Err(err);
                    }
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

                    // 发送步骤进度事件（TUI 状态栏）
                    self.emit(AgentEvent::StepProgress {
                        step: step + 1,
                        task_hint: tool_call.tool.clone(),
                    }).await;

                    // 发送工具调用事件（TUI/CLI 双路）
                    self.emit_tool_call(step + 1, &tool_call).await;

                    // 保存 assistant 的 tool_use 消息到历史
                    self.memory.push_raw_message(serde_json::json!({
                        "role": "assistant",
                        "content": assistant_content,
                    }));

                    let tool_result = self.execute_tool_call(user_input, tool_call.clone(), true).await?;

                    // 记录完成的步骤（用于 max_steps 摘要）
                    completed_tools.push((tool_call.tool.clone(), tool_result.success));

                    // 发送工具结果事件（TUI/CLI 双路）
                    self.emit_tool_result(&tool_result).await;
                    let result_content = self.format_tool_result(&tool_result);

                    self.memory.push_tool_result(
                        &tool_use_id,
                        &result_content,
                        !tool_result.success,
                    );
                }
            }
        }

        // 达到最大步数：生成已完成步骤摘要
        let step_summary = completed_tools
            .iter()
            .enumerate()
            .map(|(i, (tool, ok))| {
                let icon = if *ok { "✅" } else { "❌" };
                format!("  第{}步: {} {}", i + 1, icon, tool)
            })
            .collect::<Vec<_>>()
            .join("\n");

        Ok(format!(
            "⚠️  已执行 {} 步（上限 {} 步），任务可能未完全完成。\n\n已完成步骤：\n{}\n\n请继续描述下一步需求，或告诉我哪部分未完成。",
            completed_tools.len(),
            self.max_steps,
            if step_summary.is_empty() { "  （无工具调用）".to_string() } else { step_summary }
        ))
    }

    /// HIGH RISK 确认：TUI 模式走 oneshot channel，CLI 模式走 inquire
    async fn prompt_high_risk_confirmation(
        &self,
        call: &ToolCall,
        risk: &crate::types::risk::RiskAssessment,
    ) -> Result<bool> {
        if let Some(tx) = &self.tui_tx {
            // TUI 模式：发弹窗事件，等待用户按 Y/N
            let (confirm_tx, confirm_rx) = oneshot::channel::<bool>();
            let command_preview = call.args["command"]
                .as_str()
                .unwrap_or("")
                .to_string();

            let _ = tx.send(AgentEvent::RiskPrompt {
                tool: call.tool.clone(),
                command_preview,
                risk_level: risk.level.clone(),
                reason: risk.reason.clone(),
                impact: risk.impact.clone(),
                alternative: risk.alternative.clone(),
                confirm_tx,
            }).await;

            // 等待 UI 回应（用户关闭 TUI 时 confirm_rx 会返回 Err，视为取消）
            Ok(confirm_rx.await.unwrap_or(false))
        } else {
            // CLI fallback：使用 inquire
            println!("\n{}", "═".repeat(60));
            println!("⚠️   高风险操作 — 需要您确认");
            println!("{}", "═".repeat(60));
            println!("工具：{}", call.tool);
            println!("参数：{}", serde_json::to_string_pretty(&call.args).unwrap_or_default());
            println!("风险：{}", risk.reason);
            println!("影响：{}", risk.impact);
            if let Some(alt) = &risk.alternative {
                println!("建议：{}", alt);
            }
            println!("{}", "═".repeat(60));
            Confirm::new("确认执行此高风险操作？")
                .with_default(false)
                .prompt()
                .map_err(|e| anyhow::anyhow!("输入错误: {}", e))
        }
    }

    /// MEDIUM 风险确认（safe 模式）
    async fn prompt_medium_risk_confirmation(&self, call: &ToolCall) -> Result<bool> {
        if let Some(tx) = &self.tui_tx {
            let (confirm_tx, confirm_rx) = oneshot::channel::<bool>();
            let _ = tx.send(AgentEvent::RiskPrompt {
                tool: call.tool.clone(),
                command_preview: call.args["command"].as_str().unwrap_or("").to_string(),
                risk_level: RiskLevel::Medium,
                reason: "此操作会修改系统状态".to_string(),
                impact: "操作通常可逆，safe 模式要求确认".to_string(),
                alternative: None,
                confirm_tx,
            }).await;
            Ok(confirm_rx.await.unwrap_or(false))
        } else {
            println!("\n🟡 [safe 模式] 此操作会修改系统状态：{} {:?}", call.tool, call.args);
            Confirm::new("确认执行？")
                .with_default(false)
                .with_help_message("safe 模式要求 Medium 风险操作需确认")
                .prompt()
                .map_err(|e| anyhow::anyhow!("输入错误: {}", e))
        }
    }

    /// 发送工具调用事件（TUI）/ 打印到终端（CLI）
    async fn emit_tool_call(&self, step: usize, call: &ToolCall) {
        let args = serde_json::to_string(&call.args).unwrap_or_default();
        if self.tui_tx.is_some() {
            self.emit(AgentEvent::ToolCall {
                step,
                tool: call.tool.clone(),
                args,
                dry_run: call.dry_run,
            }).await;
        } else {
            let prefix = if call.dry_run { "[DRY-RUN] " } else { "" };
            println!("\n🔧 Step {}: {}{}({})", step, prefix, call.tool, args);
        }
    }

    /// 发送工具结果事件（TUI）/ 打印到终端（CLI）
    async fn emit_tool_result(&self, result: &ToolResult) {
        if self.tui_tx.is_some() {
            if let Some(preview) = &result.dry_run_preview {
                self.emit(AgentEvent::ToolResult {
                    success: true,
                    preview: format!("[DRY-RUN] {}", preview),
                    duration_ms: result.duration_ms,
                }).await;
            } else {
                let preview = if result.success {
                    result.stdout.lines().next().unwrap_or("（无输出）").to_string()
                } else {
                    result.stderr.lines().next().unwrap_or("（执行失败）").to_string()
                };
                self.emit(AgentEvent::ToolResult {
                    success: result.success,
                    preview,
                    duration_ms: result.duration_ms,
                }).await;
            }
        } else {
            // CLI 打印
            if let Some(preview) = &result.dry_run_preview {
                println!("   📋 预览：{}", preview);
            } else if result.success {
                let preview = result.stdout.lines().next().unwrap_or("（无输出）");
                println!("   ✅ 成功：{}", preview);
            } else {
                println!("   ❌ 失败 (exit {})：{}", result.exit_code,
                    result.stderr.lines().next().unwrap_or(""));
            }
        }
    }

    /// 根据工具调用生成回滚方案
    fn generate_rollback_plan(&self, call: &ToolCall, result: &ToolResult) -> Option<RollbackPlan> {
        if !result.success || call.dry_run {
            return None; // 失败或预览操作无需回滚
        }

        match call.tool.as_str() {
            "shell.exec" => self.generate_shell_rollback(call),
            "file.write" => self.generate_file_write_rollback_with_result(call, result),
            _ => None, // 只读操作无需回滚
        }
    }

    /// 为 shell.exec 生成回滚方案
    fn generate_shell_rollback(&self, call: &ToolCall) -> Option<RollbackPlan> {
        let command = call.args["command"].as_str()?;
        let command_lower = command.to_lowercase();

        // apt install → apt remove
        if command_lower.contains("apt install") || command_lower.contains("apt-get install") {
            let pkg = self.extract_package_name(command)?;
            return Some(RollbackPlan {
                description: format!("卸载已安装的包: {}", pkg),
                commands: vec![format!("sudo apt remove -y {}", pkg)],
                has_side_effects: true,
            });
        }

        // yum/dnf install → remove
        if command_lower.contains("yum install") || command_lower.contains("dnf install") {
            let pkg = self.extract_package_name(command)?;
            return Some(RollbackPlan {
                description: format!("卸载已安装的包: {}", pkg),
                commands: vec![format!("sudo yum remove -y {}", pkg)],
                has_side_effects: true,
            });
        }

        // useradd → userdel
        if command_lower.contains("useradd") {
            let user = self.extract_arg_value(command)?;
            return Some(RollbackPlan {
                description: format!("删除已创建的用户: {}", user),
                commands: vec![format!("sudo userdel -r {}", user)],
                has_side_effects: true,
            });
        }

        // systemctl start → stop
        if command_lower.contains("systemctl start") {
            let service = self.extract_arg_value(command)?;
            return Some(RollbackPlan {
                description: format!("停止已启动的服务: {}", service),
                commands: vec![format!("sudo systemctl stop {}", service)],
                has_side_effects: false,
            });
        }

        // systemctl stop → start (需要知道之前状态)
        if command_lower.contains("systemctl stop") {
            let service = self.extract_arg_value(command)?;
            return Some(RollbackPlan {
                description: format!("重新启动已停止的服务: {}", service),
                commands: vec![format!("sudo systemctl start {}", service)],
                has_side_effects: false,
            });
        }

        // rm 文件删除无法简单回滚（需备份，暂不支持）
        if command_lower.starts_with("rm") {
            return Some(RollbackPlan {
                description: "文件删除操作无法自动回滚（需要备份文件）".to_string(),
                commands: vec![], // 空数组表示无法自动回滚
                has_side_effects: true,
            });
        }

        None
    }

    /// 从命令中提取包名
    fn extract_package_name(&self, command: &str) -> Option<String> {
        // 支持 "apt install pkg1 pkg2" 和 "apt install -y pkg"
        let parts: Vec<&str> = command.split_whitespace().collect();
        for part in parts.iter().skip_while(|p| **p != "install").skip(1) {
            if !part.starts_with('-') {
                return Some(part.to_string());
            }
        }
        None
    }

    /// 从命令中提取最后一个参数值（适用于 useradd、systemctl 等）
    fn extract_arg_value(&self, command: &str) -> Option<String> {
        let parts: Vec<&str> = command.split_whitespace().collect();
        parts.last().map(|s| s.to_string())
    }

    /// 测试辅助：暴露 shell rollback 生成（仅测试可见）
    #[cfg(test)]
    pub fn generate_shell_rollback_test(&self, call: &ToolCall) -> Option<RollbackPlan> {
        self.generate_shell_rollback(call)
    }

    /// 测试辅助：暴露 file.write rollback 生成（仅测试可见）
    #[cfg(test)]
    pub fn generate_file_write_rollback_test(&self, call: &ToolCall, result: &ToolResult) -> Option<RollbackPlan> {
        self.generate_file_write_rollback_with_result(call, result)
    }

    /// 为 file.write 生成回滚方案（从 ToolResult 中提取备份路径）
    fn generate_file_write_rollback_with_result(&self, call: &ToolCall, result: &ToolResult) -> Option<RollbackPlan> {
        let path = call.args["path"].as_str()?;
        // 从 stdout 中解析备份路径标记 [BACKUP:/tmp/...]
        let backup_path = result.stdout.lines().find_map(|line| {
            line.strip_prefix("[BACKUP:").and_then(|s| s.strip_suffix(']')).map(str::to_string)
        });

        match backup_path {
            Some(bak) => Some(RollbackPlan {
                description: format!("从备份恢复文件 {}", path),
                commands: vec![format!("cp '{}' '{}'", bak, path)],
                has_side_effects: false,
            }),
            None => Some(RollbackPlan {
                description: format!("文件 {} 写入前无备份，无法自动回滚", path),
                commands: vec![],
                has_side_effects: true,
            }),
        }
    }

    /// 格式化工具结果为 LLM 可读文本（测试可见）
    #[cfg(test)]
    pub fn format_tool_result_test(&self, result: &crate::types::tool::ToolResult) -> String {
        self.format_tool_result(result)
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

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::{LlmConfig, LlmProviderKind};
    use crate::agent::memory::SystemContext;
    use crate::types::tool::{ToolCall, ToolResult};
    use chrono::Utc;
    use serde_json::json;

    fn dummy_config() -> LlmConfig {
        LlmConfig::new_test(
            LlmProviderKind::Anthropic,
            "https://api.anthropic.com",
            "claude-haiku-4-5-20251001",
        )
    }

    fn dummy_ctx() -> SystemContext {
        SystemContext {
            os_info: "Linux".to_string(),
            hostname: "testhost".to_string(),
            cpu_info: "2 cores".to_string(),
            memory_info: "4GB".to_string(),
            disk_info: "50GB".to_string(),
            running_services: vec![],
            package_manager: "apt".to_string(),
            network_info: "lo: 127.0.0.1".to_string(),
            collected_at: Utc::now(),
        }
    }

    fn make_agent() -> AgentLoop {
        AgentLoop::new(dummy_config(), "normal", dummy_ctx())
    }

    fn shell_call(cmd: &str) -> ToolCall {
        ToolCall {
            tool: "shell.exec".to_string(),
            args: json!({"command": cmd}),
            reason: None,
            dry_run: false,
        }
    }

    fn file_write_call(path: &str) -> ToolCall {
        ToolCall {
            tool: "file.write".to_string(),
            args: json!({"path": path, "content": "new content"}),
            reason: None,
            dry_run: false,
        }
    }

    // ── format_tool_result ────────────────────────────────────────────────

    #[test]
    fn format_dry_run_result_includes_preview_prefix() {
        let agent = make_agent();
        let result = ToolResult::dry_run_preview("shell.exec", "would run: ls -la");
        let text = agent.format_tool_result_test(&result);
        assert!(text.starts_with("[DRY-RUN 预览]"));
        assert!(text.contains("would run: ls -la"));
    }

    #[test]
    fn format_success_result_with_output() {
        let agent = make_agent();
        let result = ToolResult::success("shell.exec", "hello world", 10);
        let text = agent.format_tool_result_test(&result);
        assert_eq!(text, "hello world");
    }

    #[test]
    fn format_success_result_with_empty_output() {
        let agent = make_agent();
        let result = ToolResult::success("shell.exec", "", 10);
        let text = agent.format_tool_result_test(&result);
        assert_eq!(text, "操作成功（无输出）");
    }

    #[test]
    fn format_failure_result_includes_exit_code_and_stderr() {
        let agent = make_agent();
        let result = ToolResult::failure("shell.exec", "command not found", 127);
        let text = agent.format_tool_result_test(&result);
        assert!(text.contains("127"));
        assert!(text.contains("command not found"));
    }

    #[test]
    fn format_long_output_is_truncated_at_4000_chars() {
        let agent = make_agent();
        let long_output = "x".repeat(5000);
        let result = ToolResult::success("shell.exec", &long_output, 10);
        let text = agent.format_tool_result_test(&result);
        assert!(text.contains("[...输出已截断，共 5000 字符]"));
        // truncated prefix should be at most 4000 chars of 'x' plus the suffix
        let x_count = text.chars().take_while(|&c| c == 'x').count();
        assert_eq!(x_count, 4000);
    }

    // ── generate_shell_rollback ───────────────────────────────────────────

    #[test]
    fn rollback_apt_install_generates_remove_command() {
        let agent = make_agent();
        let call = shell_call("sudo apt install -y nginx");
        let plan = agent.generate_shell_rollback_test(&call).unwrap();
        assert!(plan.commands[0].contains("apt remove"));
        assert!(plan.commands[0].contains("nginx"));
    }

    #[test]
    fn rollback_yum_install_generates_remove_command() {
        let agent = make_agent();
        let call = shell_call("sudo yum install -y httpd");
        let plan = agent.generate_shell_rollback_test(&call).unwrap();
        assert!(plan.commands[0].contains("yum remove"));
        assert!(plan.commands[0].contains("httpd"));
    }

    #[test]
    fn rollback_useradd_generates_userdel_command() {
        let agent = make_agent();
        let call = shell_call("sudo useradd testuser");
        let plan = agent.generate_shell_rollback_test(&call).unwrap();
        assert!(plan.commands[0].contains("userdel"));
        assert!(plan.commands[0].contains("testuser"));
    }

    #[test]
    fn rollback_systemctl_start_generates_stop_command() {
        let agent = make_agent();
        let call = shell_call("sudo systemctl start nginx");
        let plan = agent.generate_shell_rollback_test(&call).unwrap();
        assert!(plan.commands[0].contains("systemctl stop"));
        assert!(plan.commands[0].contains("nginx"));
    }

    #[test]
    fn rollback_systemctl_stop_generates_start_command() {
        let agent = make_agent();
        let call = shell_call("sudo systemctl stop nginx");
        let plan = agent.generate_shell_rollback_test(&call).unwrap();
        assert!(plan.commands[0].contains("systemctl start"));
        assert!(plan.commands[0].contains("nginx"));
    }

    #[test]
    fn rollback_rm_returns_plan_with_empty_commands() {
        let agent = make_agent();
        let call = shell_call("rm -f /tmp/test.log");
        let plan = agent.generate_shell_rollback_test(&call).unwrap();
        assert!(plan.commands.is_empty());
        assert!(plan.has_side_effects);
    }

    #[test]
    fn rollback_unknown_command_returns_none() {
        let agent = make_agent();
        let call = shell_call("ls -la /tmp");
        let plan = agent.generate_shell_rollback_test(&call);
        assert!(plan.is_none());
    }

    // ── generate_file_write_rollback ──────────────────────────────────────

    #[test]
    fn file_write_rollback_with_backup_path_in_stdout() {
        let agent = make_agent();
        let call = file_write_call("/etc/nginx/nginx.conf");
        let result = ToolResult::success(
            "file.write",
            "写入成功\n[BACKUP:/tmp/nginx.conf.20240101.bak]",
            20,
        );
        let plan = agent.generate_file_write_rollback_test(&call, &result).unwrap();
        assert!(plan.commands[0].contains("/tmp/nginx.conf.20240101.bak"));
        assert!(plan.commands[0].contains("/etc/nginx/nginx.conf"));
        assert!(!plan.has_side_effects);
    }

    #[test]
    fn file_write_rollback_without_backup_has_empty_commands() {
        let agent = make_agent();
        let call = file_write_call("/etc/hosts");
        let result = ToolResult::success("file.write", "写入成功（无备份）", 15);
        let plan = agent.generate_file_write_rollback_test(&call, &result).unwrap();
        assert!(plan.commands.is_empty());
        assert!(plan.has_side_effects);
    }

    // ── get_history_summary ───────────────────────────────────────────────

    #[test]
    fn get_history_summary_empty_returns_no_record_message() {
        let agent = make_agent();
        let summary = agent.get_history_summary();
        assert!(summary.contains("暂无"));
    }
}
