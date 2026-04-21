use crate::types::tool::{OperationRecord, Playbook, RollbackPlan, ToolCall, ToolResult};
use chrono::{DateTime, Utc};
use uuid::Uuid;

/// 会话记忆：保存对话历史（Claude API 格式）和操作记录
pub struct Memory {
    /// LLM 对话历史（Claude API 原生格式，支持 tool_use 多轮）
    pub messages: Vec<serde_json::Value>,
    /// 操作记录（用于 Undo / 审计）
    pub operations: Vec<OperationRecord>,
    /// 系统上下文快照
    pub system_context: Option<SystemContext>,
    /// 最大保留消息数（Token 窗口管理）
    pub max_messages: usize,
    /// 自上次刷新后的操作计数
    ops_since_refresh: usize,
    /// 刷新阈值（每 N 步操作刷新系统状态）
    refresh_threshold: usize,
}

#[derive(Debug, Clone)]
pub struct SystemContext {
    pub os_info: String,
    pub hostname: String,
    pub cpu_info: String,
    pub memory_info: String,
    pub disk_info: String,
    pub running_services: Vec<String>,
    pub package_manager: String, // apt | yum | dnf
    #[allow(dead_code)] // reserved for cache invalidation logic
    pub collected_at: DateTime<Utc>,
}

impl Memory {
    pub fn new() -> Self {
        Self {
            messages: Vec::new(),
            operations: Vec::new(),
            system_context: None,
            max_messages: 50,
            ops_since_refresh: 0,
            refresh_threshold: 5,
        }
    }

    /// 刷新系统上下文
    pub fn refresh_system_context(&mut self, ctx: SystemContext) {
        self.system_context = Some(ctx);
        self.ops_since_refresh = 0;
    }

    /// 检查是否需要刷新系统状态
    pub fn needs_refresh(&self) -> bool {
        self.ops_since_refresh >= self.refresh_threshold
    }

    /// 添加用户文本消息
    pub fn push_user_text(&mut self, text: &str) {
        self.push_raw_message(serde_json::json!({
            "role": "user",
            "content": text
        }));
    }

    /// 添加助手文本回复
    pub fn push_assistant_text(&mut self, text: &str) {
        self.push_raw_message(serde_json::json!({
            "role": "assistant",
            "content": text
        }));
    }

    /// 添加工具调用结果（Claude API tool_result 格式）
    pub fn push_tool_result(&mut self, tool_use_id: &str, content: &str, is_error: bool) {
        self.push_raw_message(serde_json::json!({
            "role": "user",
            "content": [{
                "type": "tool_result",
                "tool_use_id": tool_use_id,
                "content": content,
                "is_error": is_error
            }]
        }));
    }

    /// 直接推入原始 Claude API 格式消息
    pub fn push_raw_message(&mut self, msg: serde_json::Value) {
        self.messages.push(msg);
        // 超出窗口时成对删除：移除最老的 user+assistant 对，保持角色交替不破坏 tool_use 配对
        while self.messages.len() > self.max_messages {
            if self.messages.len() >= 2 {
                // 删除最老的两条（通常是 user + assistant 对）
                self.messages.drain(..2);
            } else {
                self.messages.remove(0);
            }
        }
    }

    /// 记录一次操作（含 Undo 方案）
    pub fn record_operation(
        &mut self,
        call: ToolCall,
        result: &ToolResult,
        rollback: Option<RollbackPlan>,
    ) {
        self.operations.push(OperationRecord {
            id: Uuid::new_v4(),
            tool_call: call,
            result: Some(crate::types::tool::ToolResultRecord {
                success: result.success,
                summary: if result.success {
                    result.stdout.chars().take(200).collect()
                } else {
                    result.stderr.chars().take(200).collect()
                },
            }),
            rollback,
            timestamp: Utc::now(),
        });
        // 增加操作计数（用于系统状态刷新检测）
        self.ops_since_refresh += 1;
    }

    /// 获取最近一次可回滚的操作
    pub fn last_undoable(&self) -> Option<&OperationRecord> {
        self.operations
            .iter()
            .rev()
            .find(|op| op.rollback.is_some())
    }

    /// 将多步操作保存为 Playbook（Phase 4：Playbook 功能保留）
    #[allow(dead_code)]
    pub fn save_as_playbook(&self, name: &str, description: &str, step_count: usize) -> Playbook {
        let steps: Vec<ToolCall> = self
            .operations
            .iter()
            .rev()
            .take(step_count)
            .rev()
            .map(|op| op.tool_call.clone())
            .collect();

        Playbook {
            name: name.to_string(),
            description: description.to_string(),
            steps,
            created_at: Utc::now(),
            run_count: 0,
        }
    }

    /// 返回发送给 LLM 的消息列表
    pub fn build_llm_messages(&self) -> Vec<serde_json::Value> {
        self.messages.clone()
    }
}

impl Default for Memory {
    fn default() -> Self {
        Self::new()
    }
}
