use crate::types::{
    risk::RiskLevel,
    tool::{ToolCall, ToolResult},
};
use chrono::Utc;
use serde_json::json;
use std::fs::OpenOptions;
use std::io::Write;

/// 审计日志写入（JSON Lines 格式，每行一条记录）
pub struct AuditLogger {
    log_path: String,
    session_id: String,
}

impl AuditLogger {
    pub fn new(session_id: &str) -> Self {
        let log_path = format!(
            "{}/.agent-unix/audit-{}.jsonl",
            std::env::var("HOME").unwrap_or_else(|_| "/tmp".to_string()),
            chrono::Local::now().format("%Y%m%d")
        );

        // 确保日志目录存在
        if let Some(parent) = std::path::Path::new(&log_path).parent() {
            let _ = std::fs::create_dir_all(parent);
        }

        Self {
            log_path,
            session_id: session_id.to_string(),
        }
    }

    /// 记录一次操作（含结果）
    pub fn log_operation(
        &self,
        user_input: &str,
        call: &ToolCall,
        risk: &RiskLevel,
        confirmed: bool,
        result: Option<&ToolResult>,
    ) {
        let entry = json!({
            "ts": Utc::now().to_rfc3339(),
            "session": self.session_id,
            "user_input": user_input,
            "tool": call.tool,
            "args": call.args,
            "dry_run": call.dry_run,
            "risk": risk.label(),
            "confirmed": confirmed,
            "success": result.map(|r| r.success),
            "duration_ms": result.map(|r| r.duration_ms),
        });

        self.append_line(&entry.to_string());
    }

    /// 记录被拒绝的操作
    pub fn log_blocked(&self, user_input: &str, call: &ToolCall, reason: &str) {
        let entry = json!({
            "ts": Utc::now().to_rfc3339(),
            "session": self.session_id,
            "user_input": user_input,
            "tool": call.tool,
            "args": call.args,
            "risk": "CRITICAL",
            "blocked": true,
            "block_reason": reason,
        });

        self.append_line(&entry.to_string());
    }

    fn append_line(&self, line: &str) {
        if let Ok(mut file) = OpenOptions::new()
            .create(true)
            .append(true)
            .open(&self.log_path)
        {
            let _ = writeln!(file, "{}", line);
        }
    }
}
