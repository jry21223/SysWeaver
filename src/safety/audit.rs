use crate::types::{
    risk::RiskLevel,
    tool::{ToolCall, ToolResult},
};
use chrono::Utc;
use serde_json::{Value, json};
use std::fs::OpenOptions;
use std::io::{ErrorKind, Write};
use std::path::Path;

const MAX_LOG_VALUE_CHARS: usize = 256;
const REDACTED: &str = "[redacted]";
const SENSITIVE_KEYS: &[&str] = &[
    "api_key",
    "apikey",
    "token",
    "secret",
    "password",
    "passwd",
    "authorization",
    "cookie",
    "session",
    "base64_data",
    "data",
];
const SENSITIVE_PATTERNS: &[&str] = &[
    "api_key",
    "token",
    "secret",
    "password",
    "authorization",
    "bearer ",
    "sk-",
    "data:image/",
    "base64,",
    "-----begin",
];

/// 审计日志写入（JSON Lines 格式，每行一条记录）
pub struct AuditLogger {
    log_path: String,
    session_id: String,
}

impl AuditLogger {
    pub fn new(session_id: &str) -> Self {
        let log_path = format!(
            "{}/.jij/audit-{}.jsonl",
            std::env::var("HOME").or_else(|_| std::env::var("USERPROFILE"))
                .unwrap_or_else(|_| {
                    if cfg!(windows) { "C:\\Temp".to_string() } else { "/tmp".to_string() }
                }),
            chrono::Local::now().format("%Y%m%d")
        );

        if let Some(parent) = Path::new(&log_path).parent() {
            if let Err(err) = std::fs::create_dir_all(parent) {
                tracing::warn!("创建审计日志目录失败: {}", err);
            }
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
            "user_input": redact_text(user_input),
            "tool": call.tool,
            "args": redact_value(&call.args, None),
            "dry_run": call.dry_run,
            "risk": risk.label(),
            "confirmed": confirmed,
            "success": result.map(|r| r.success),
            "duration_ms": result.map(|r| r.duration_ms),
        });

        self.append_entry(&entry);
    }

    /// 记录被拒绝的操作
    pub fn log_blocked(&self, user_input: &str, call: &ToolCall, reason: &str) {
        let entry = json!({
            "ts": Utc::now().to_rfc3339(),
            "session": self.session_id,
            "user_input": redact_text(user_input),
            "tool": call.tool,
            "args": redact_value(&call.args, None),
            "risk": "CRITICAL",
            "blocked": true,
            "block_reason": redact_text(reason),
        });

        self.append_entry(&entry);
    }

    /// 记录自定义事件（如图片输入）
    pub fn log_custom(&self, event_type: &str, data: &str) {
        let entry = json!({
            "ts": Utc::now().to_rfc3339(),
            "session": self.session_id,
            "event_type": event_type,
            "data": redact_text(data),
        });

        self.append_entry(&entry);
    }

    fn append_entry(&self, entry: &Value) {
        let serialized = match serde_json::to_string(entry) {
            Ok(line) => line,
            Err(err) => {
                tracing::warn!("序列化审计日志失败: {}", err);
                return;
            }
        };

        self.append_line(&serialized);
    }

    fn append_line(&self, line: &str) {
        match OpenOptions::new()
            .create(true)
            .append(true)
            .open(&self.log_path)
        {
            Ok(mut file) => {
                if let Err(err) = writeln!(file, "{}", line) {
                    tracing::warn!("写入审计日志失败: {}", err);
                }
            }
            Err(err) if err.kind() == ErrorKind::NotFound => {
                if let Some(parent) = Path::new(&self.log_path).parent() {
                    if let Err(create_err) = std::fs::create_dir_all(parent) {
                        tracing::warn!("重建审计目录失败: {}", create_err);
                        return;
                    }
                }

                match OpenOptions::new()
                    .create(true)
                    .append(true)
                    .open(&self.log_path)
                {
                    Ok(mut file) => {
                        if let Err(write_err) = writeln!(file, "{}", line) {
                            tracing::warn!("写入审计日志失败: {}", write_err);
                        }
                    }
                    Err(open_err) => {
                        tracing::warn!("打开审计日志失败: {}", open_err);
                    }
                }
            }
            Err(err) => {
                tracing::warn!("打开审计日志失败: {}", err);
            }
        }
    }
}

pub fn should_persist_input(text: &str) -> bool {
    let lower = text.to_lowercase();
    !SENSITIVE_PATTERNS.iter().any(|pattern| lower.contains(pattern))
}

fn redact_value(value: &Value, key: Option<&str>) -> Value {
    if key.is_some_and(is_sensitive_key) {
        return Value::String(REDACTED.to_string());
    }

    match value {
        Value::Object(map) => Value::Object(
            map.iter()
                .map(|(child_key, child_value)| {
                    (
                        child_key.clone(),
                        redact_value(child_value, Some(child_key.as_str())),
                    )
                })
                .collect(),
        ),
        Value::Array(items) => Value::Array(
            items
                .iter()
                .map(|item| redact_value(item, key))
                .collect(),
        ),
        Value::String(text) => Value::String(redact_text(text)),
        _ => value.clone(),
    }
}

fn redact_text(text: &str) -> String {
    let lower = text.to_lowercase();
    if SENSITIVE_PATTERNS.iter().any(|pattern| lower.contains(pattern)) {
        return REDACTED.to_string();
    }

    if text.chars().count() > MAX_LOG_VALUE_CHARS {
        let truncated: String = text.chars().take(MAX_LOG_VALUE_CHARS).collect();
        return format!("{}...[truncated]", truncated);
    }

    text.to_string()
}

fn is_sensitive_key(key: &str) -> bool {
    let normalized = key.to_lowercase();
    SENSITIVE_KEYS.iter().any(|sensitive| normalized.contains(sensitive))
}
