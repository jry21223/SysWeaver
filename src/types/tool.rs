use serde::{Deserialize, Serialize};
use chrono::{DateTime, Utc};
use uuid::Uuid;

/// LLM 必须输出的工具调用格式
#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct ToolCall {
    /// 工具名称，格式: "namespace.action"，例如 "shell.exec", "file.read"
    pub tool: String,
    /// 工具参数（各工具自定义结构）
    pub args: serde_json::Value,
    /// LLM 对本次调用的说明（可解释性）
    pub reason: Option<String>,
    /// 是否为 Dry-Run 模式（仅预览，不实际执行）
    #[serde(default)]
    pub dry_run: bool,
}

/// 工具执行结果
#[derive(Debug, Clone, Serialize)]
pub struct ToolResult {
    pub tool: String,
    pub success: bool,
    pub stdout: String,
    pub stderr: String,
    pub exit_code: i32,
    pub duration_ms: u64,
    /// Dry-Run 模式时的预览说明
    pub dry_run_preview: Option<String>,
}

/// 单次操作记录（用于 Undo / 审计）
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct OperationRecord {
    pub id: Uuid,
    pub tool_call: ToolCall,
    pub result: Option<ToolResultRecord>,
    pub rollback: Option<RollbackPlan>,
    pub timestamp: DateTime<Utc>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ToolResultRecord {
    pub success: bool,
    pub summary: String,
}

/// 回滚方案（Undo 功能核心）
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RollbackPlan {
    /// 用自然语言描述回滚操作
    pub description: String,
    /// 实际执行的回滚命令序列
    pub commands: Vec<String>,
    /// 回滚是否可能有副作用
    pub has_side_effects: bool,
}

/// 保存的 Playbook（多步骤任务模板）— Phase 4 功能保留
#[allow(dead_code)]
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Playbook {
    pub name: String,
    pub description: String,
    pub steps: Vec<ToolCall>,
    pub created_at: DateTime<Utc>,
    pub run_count: u32,
}

impl ToolResult {
    pub fn success(tool: &str, stdout: &str, duration_ms: u64) -> Self {
        Self {
            tool: tool.to_string(),
            success: true,
            stdout: stdout.to_string(),
            stderr: String::new(),
            exit_code: 0,
            duration_ms,
            dry_run_preview: None,
        }
    }

    pub fn failure(tool: &str, stderr: &str, exit_code: i32) -> Self {
        Self {
            tool: tool.to_string(),
            success: false,
            stdout: String::new(),
            stderr: stderr.to_string(),
            exit_code,
            duration_ms: 0,
            dry_run_preview: None,
        }
    }

    pub fn dry_run_preview(tool: &str, preview: &str) -> Self {
        Self {
            tool: tool.to_string(),
            success: true,
            stdout: String::new(),
            stderr: String::new(),
            exit_code: 0,
            duration_ms: 0,
            dry_run_preview: Some(preview.to_string()),
        }
    }
}
