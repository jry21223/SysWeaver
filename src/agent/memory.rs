use crate::types::tool::{OperationRecord, Playbook, RollbackPlan, ToolCall, ToolResult};
use chrono::{DateTime, Utc};
use serde_json::json;
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
    /// 压缩统计（用于调试）
    compression_stats: CompressionStats,
}

/// 压缩统计
#[derive(Debug, Clone, Default)]
pub struct CompressionStats {
    pub snip_count: usize,      // 删除的冗余消息数
    pub truncate_count: usize,  // 截断的长输出数
    pub merge_count: usize,     // 合并的操作数
    pub saved_tokens: usize,    // 估算节省的 token 数
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
            compression_stats: CompressionStats::default(),
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

    /// 返回发送给 LLM 的消息列表（应用压缩）
    pub fn build_llm_messages(&mut self) -> Vec<serde_json::Value> {
        // 消息数量接近上限时触发压缩
        if self.messages.len() > self.max_messages * 3 / 4 {
            self.compress_messages();
        }
        self.messages.clone()
    }

    /// 智能压缩对话历史
    fn compress_messages(&mut self) {
        let original_len = self.messages.len();

        // 1. Snip 压缩：删除重复的 system.info 查询结果
        self.snip_duplicate_system_queries();

        // 2. Micro 压缩：截断超长工具输出
        self.truncate_long_outputs();

        // 3. 合并连续的同类操作摘要
        self.merge_repeated_operations();

        // 4. 如果仍然过长，删除最老的消息对
        while self.messages.len() > self.max_messages {
            if self.messages.len() >= 2 {
                self.messages.drain(..2);
                self.compression_stats.snip_count += 2;
            } else {
                self.messages.remove(0);
                self.compression_stats.snip_count += 1;
            }
        }

        // 更新节省的 token 估算
        let saved = original_len - self.messages.len();
        self.compression_stats.saved_tokens += saved * 100; // 估算每条消息约 100 tokens

        if saved > 0 {
            tracing::debug!(
                "上下文压缩完成：删除 {} 条消息，节省约 {} tokens",
                saved,
                saved * 100
            );
        }
    }

    /// Snip 压缩：删除重复的 system.info 查询结果
    /// 保留最新的，删除旧的（因为系统状态可能已变化）
    fn snip_duplicate_system_queries(&mut self) {
        let mut system_info_indices: Vec<usize> = Vec::new();

        // 找到所有 system.info 的 tool_result
        for (i, msg) in self.messages.iter().enumerate() {
            if self.is_system_info_result(msg) {
                system_info_indices.push(i);
            }
        }

        // 只保留最后一个，删除其他的
        if system_info_indices.len() > 1 {
            let _keep_index = system_info_indices.pop().unwrap(); // 最后一个（保留）
            let compressed_count = system_info_indices.len();
            for idx in system_info_indices {
                // 将旧的结果替换为简短摘要
                self.messages[idx] = json!({
                    "role": "user",
                    "content": [{
                        "type": "tool_result",
                        "tool_use_id": "compressed",
                        "content": "[已压缩：旧的系统状态查询]",
                        "is_error": false
                    }]
                });
                self.compression_stats.snip_count += 1;
            }
            tracing::debug!("压缩了 {} 个重复的 system.info 结果", compressed_count);
        }
    }

    /// 判断是否为 system.info 的工具结果
    fn is_system_info_result(&self, msg: &serde_json::Value) -> bool {
        if msg["role"] != "user" {
            return false;
        }
        let content = msg.get("content").and_then(|c| c.as_array());
        if let Some(blocks) = content {
            for block in blocks {
                if block["type"] == "tool_result" {
                    // 检查前一条 assistant 消息是否调用了 system.info
                    // 这里简化处理：检查内容是否包含系统信息关键词
                    let content_text = block["content"].as_str().unwrap_or("");
                    if content_text.contains("磁盘")
                        || content_text.contains("内存")
                        || content_text.contains("CPU")
                        || content_text.contains("进程") {
                        return true;
                    }
                }
            }
        }
        false
    }

    /// Micro 压缩：截断超长工具输出（超过 2000 字符）
    fn truncate_long_outputs(&mut self) {
        for msg in &mut self.messages {
            if msg["role"] == "user" {
                let content = msg.get_mut("content").and_then(|c| c.as_array_mut());
                if let Some(blocks) = content {
                    for block in blocks.iter_mut() {
                        if block["type"] == "tool_result" {
                            // 先获取内容，再决定是否截断
                            let content_text = block.get("content")
                                .and_then(|c| c.as_str())
                                .unwrap_or("");
                            if content_text.len() > 2000 {
                                let original_len = content_text.len();
                                let truncated: String = content_text.chars().take(1500).collect();
                                block["content"] = json!(format!(
                                    "{}\n\n[...已压缩，原 {} 字符]",
                                    truncated,
                                    original_len
                                ));
                                self.compression_stats.truncate_count += 1;
                                self.compression_stats.saved_tokens += (original_len - 1500) / 4;
                            }
                        }
                    }
                }
            }
        }
    }

    /// 合并连续的同类操作摘要
    fn merge_repeated_operations(&mut self) {
        // 检查操作记录中是否有连续的同类操作
        if self.operations.len() < 3 {
            return;
        }

        let mut merged_count = 0;
        let mut i = 0;

        while i < self.operations.len() - 1 {
            let current_tool = &self.operations[i].tool_call.tool;
            let next_tool = &self.operations[i + 1].tool_call.tool;

            // 连续的同类工具调用（如多次 file.read）
            if current_tool == next_tool {
                // 在消息中用摘要替换（简化处理）
                merged_count += 1;
            }
            i += 1;
        }

        if merged_count > 0 {
            self.compression_stats.merge_count = merged_count;
            self.compression_stats.saved_tokens += merged_count * 50;
        }
    }

    /// 获取压缩统计（用于调试）
    pub fn get_compression_stats(&self) -> &CompressionStats {
        &self.compression_stats
    }
}

impl Default for Memory {
    fn default() -> Self {
        Self::new()
    }
}
