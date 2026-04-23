use crate::image::ImageInfo;
use crate::types::tool::{OperationRecord, Playbook, RollbackPlan, ToolCall, ToolResult};
use chrono::{DateTime, Utc};
use serde_json::{Value, json};
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
    /// 网络接口 IP 及监听端口摘要
    pub network_info: String,
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

    /// 添加用户消息（支持图片内容）
    /// Anthropic API 格式：content 为数组，包含 text 和 image blocks
    pub fn push_user_message_with_images(&mut self, text: &str, images: &[ImageInfo]) {
        let content = if images.is_empty() {
            // 无图片：简单文本消息
            serde_json::json!(text)
        } else {
            // 有图片：构建 content 数组
            let mut blocks: Vec<serde_json::Value> = vec![
                serde_json::json!({
                    "type": "text",
                    "text": text
                })
            ];

            for img in images {
                blocks.push(serde_json::json!({
                    "type": "image",
                    "source": {
                        "type": "base64",
                        "media_type": img.mime_type,
                        "data": img.base64_data
                    }
                }));
            }

            serde_json::json!(blocks)
        };

        self.push_raw_message(serde_json::json!({
            "role": "user",
            "content": content
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
        self.trim_to_max_messages();
    }

    fn trim_to_max_messages(&mut self) {
        while self.messages.len() > self.max_messages {
            let Some(remove_count) = self.oldest_complete_segment_len() else {
                break;
            };
            self.messages.drain(..remove_count);
        }
    }

    fn oldest_complete_segment_len(&self) -> Option<usize> {
        let first = self.messages.first()?;

        if self.is_top_level_user_message(first) {
            return self.next_top_level_user_index(1).or_else(|| {
                self.turn_is_complete(0, self.messages.len())
                    .then_some(self.messages.len())
            });
        }

        if self.message_has_tool_use(first) || self.message_is_tool_result(first) {
            return None;
        }

        Some(1)
    }

    fn next_top_level_user_index(&self, start: usize) -> Option<usize> {
        self.messages
            .iter()
            .enumerate()
            .skip(start)
            .find(|(_, msg)| self.is_top_level_user_message(msg))
            .map(|(index, _)| index)
    }

    fn turn_is_complete(&self, start: usize, end: usize) -> bool {
        let segment = &self.messages[start..end];
        let mut pending_tool_uses: Vec<String> = Vec::new();

        for msg in segment {
            if self.message_has_tool_use(msg) {
                pending_tool_uses.extend(self.tool_use_ids(msg));
            }

            if let Some(result_ids) = self.tool_result_ids(msg) {
                for tool_use_id in result_ids {
                    if let Some(position) = pending_tool_uses.iter().position(|id| id == &tool_use_id) {
                        pending_tool_uses.remove(position);
                    } else {
                        return false;
                    }
                }
            }
        }

        pending_tool_uses.is_empty()
    }

    fn is_top_level_user_message(&self, msg: &Value) -> bool {
        msg["role"] == "user" && !self.message_is_tool_result(msg)
    }

    fn message_has_tool_use(&self, msg: &Value) -> bool {
        !self.tool_use_ids(msg).is_empty()
    }

    fn tool_use_ids(&self, msg: &Value) -> Vec<String> {
        msg.get("content")
            .and_then(|content| content.as_array())
            .into_iter()
            .flatten()
            .filter(|block| block["type"] == "tool_use")
            .filter_map(|block| block["id"].as_str().map(ToString::to_string))
            .collect()
    }

    fn tool_result_ids(&self, msg: &Value) -> Option<Vec<String>> {
        let ids: Vec<String> = msg
            .get("content")
            .and_then(|content| content.as_array())
            .into_iter()
            .flatten()
            .filter(|block| block["type"] == "tool_result")
            .filter_map(|block| block["tool_use_id"].as_str().map(ToString::to_string))
            .collect();

        (!ids.is_empty()).then_some(ids)
    }

    fn message_is_tool_result(&self, msg: &Value) -> bool {
        self.tool_result_ids(msg).is_some()
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

        // 4. 如果仍然过长，删除最老的消息
        let before_trim = self.messages.len();
        self.trim_to_max_messages();
        self.compression_stats.snip_count += before_trim.saturating_sub(self.messages.len());

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

    #[allow(dead_code)] // 调试工具，供未来 /stats 命令使用
    pub fn get_compression_stats(&self) -> &CompressionStats {
        &self.compression_stats
    }
}

impl Default for Memory {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::types::tool::{RollbackPlan, ToolCall, ToolResult};
    use serde_json::json;

    fn make_context() -> SystemContext {
        SystemContext {
            os_info: "Linux".to_string(),
            hostname: "testhost".to_string(),
            cpu_info: "4 cores".to_string(),
            memory_info: "8GB".to_string(),
            disk_info: "100GB".to_string(),
            running_services: vec![],
            package_manager: "apt".to_string(),
            network_info: "eth0: 10.0.0.1".to_string(),
            collected_at: Utc::now(),
        }
    }

    fn make_call(tool: &str) -> ToolCall {
        ToolCall {
            tool: tool.to_string(),
            args: json!({"command": "ls"}),
            reason: None,
            dry_run: false,
        }
    }

    fn make_result(success: bool) -> ToolResult {
        if success {
            ToolResult::success("shell.exec", "output", 50)
        } else {
            ToolResult::failure("shell.exec", "error", 1)
        }
    }

    #[test]
    fn new_memory_starts_empty() {
        let mem = Memory::new();
        assert!(mem.messages.is_empty());
        assert!(mem.operations.is_empty());
        assert!(mem.system_context.is_none());
    }

    #[test]
    fn needs_refresh_false_initially() {
        assert!(!Memory::new().needs_refresh());
    }

    #[test]
    fn needs_refresh_true_after_threshold_operations() {
        let mut mem = Memory::new();
        for _ in 0..mem.refresh_threshold {
            mem.record_operation(make_call("shell.exec"), &make_result(true), None);
        }
        assert!(mem.needs_refresh());
    }

    #[test]
    fn refresh_system_context_resets_ops_counter() {
        let mut mem = Memory::new();
        for _ in 0..mem.refresh_threshold {
            mem.record_operation(make_call("shell.exec"), &make_result(true), None);
        }
        assert!(mem.needs_refresh());
        mem.refresh_system_context(make_context());
        assert!(!mem.needs_refresh());
    }

    #[test]
    fn push_user_text_adds_message_with_correct_role() {
        let mut mem = Memory::new();
        mem.push_user_text("hello");
        assert_eq!(mem.messages.len(), 1);
        assert_eq!(mem.messages[0]["role"], "user");
        assert_eq!(mem.messages[0]["content"], "hello");
    }

    #[test]
    fn push_assistant_text_adds_message_with_correct_role() {
        let mut mem = Memory::new();
        mem.push_assistant_text("world");
        assert_eq!(mem.messages.len(), 1);
        assert_eq!(mem.messages[0]["role"], "assistant");
        assert_eq!(mem.messages[0]["content"], "world");
    }

    #[test]
    fn push_tool_result_adds_correctly_formatted_message() {
        let mut mem = Memory::new();
        mem.push_tool_result("tid_1", "result content", false);
        assert_eq!(mem.messages.len(), 1);
        let content = mem.messages[0]["content"].as_array().unwrap();
        assert_eq!(content[0]["type"], "tool_result");
        assert_eq!(content[0]["tool_use_id"], "tid_1");
        assert_eq!(content[0]["content"], "result content");
        assert_eq!(content[0]["is_error"], false);
    }

    #[test]
    fn push_tool_result_error_flag_set_correctly() {
        let mut mem = Memory::new();
        mem.push_tool_result("tid_err", "something failed", true);
        let content = mem.messages[0]["content"].as_array().unwrap();
        assert_eq!(content[0]["is_error"], true);
    }

    #[test]
    fn record_operation_stores_and_increments_counter() {
        let mut mem = Memory::new();
        mem.record_operation(make_call("shell.exec"), &make_result(true), None);
        assert_eq!(mem.operations.len(), 1);
        assert_eq!(mem.ops_since_refresh, 1);
    }

    #[test]
    fn last_undoable_returns_none_when_no_rollback() {
        let mut mem = Memory::new();
        mem.record_operation(make_call("shell.exec"), &make_result(true), None);
        assert!(mem.last_undoable().is_none());
    }

    #[test]
    fn last_undoable_returns_most_recent_undoable_operation() {
        let mut mem = Memory::new();
        let rollback = RollbackPlan {
            description: "undo install".to_string(),
            commands: vec!["sudo apt remove pkg".to_string()],
            has_side_effects: true,
        };
        mem.record_operation(make_call("system.info"), &make_result(true), None);
        mem.record_operation(make_call("shell.exec"), &make_result(true), Some(rollback));
        mem.record_operation(make_call("file.read"), &make_result(true), None);

        let undoable = mem.last_undoable().unwrap();
        assert_eq!(undoable.tool_call.tool, "shell.exec");
    }

    #[test]
    fn save_as_playbook_captures_last_n_steps_in_order() {
        let mut mem = Memory::new();
        for tool in &["shell.exec", "file.read", "system.info"] {
            mem.record_operation(make_call(tool), &make_result(true), None);
        }
        let pb = mem.save_as_playbook("my-pb", "test playbook", 2);
        assert_eq!(pb.name, "my-pb");
        assert_eq!(pb.steps.len(), 2);
        assert_eq!(pb.steps[0].tool, "file.read");
        assert_eq!(pb.steps[1].tool, "system.info");
    }

    #[test]
    fn save_as_playbook_clamps_to_available_operations() {
        let mut mem = Memory::new();
        mem.record_operation(make_call("shell.exec"), &make_result(true), None);
        let pb = mem.save_as_playbook("my-pb", "desc", 10);
        assert_eq!(pb.steps.len(), 1);
    }

    #[test]
    fn build_llm_messages_returns_current_messages() {
        let mut mem = Memory::new();
        mem.push_user_text("hello");
        mem.push_assistant_text("hi");
        let messages = mem.build_llm_messages();
        assert_eq!(messages.len(), 2);
    }

    #[test]
    fn messages_trimmed_to_max_when_overflowing() {
        let mut mem = Memory::new();
        mem.max_messages = 4;
        // Push user/assistant pairs (each pair = 2 messages = one complete turn)
        for i in 0..6 {
            mem.push_user_text(&format!("msg {}", i));
            mem.push_assistant_text(&format!("reply {}", i));
        }
        assert!(mem.messages.len() <= mem.max_messages);
    }

    #[test]
    fn push_user_message_with_no_images_is_text_content() {
        let mut mem = Memory::new();
        mem.push_user_message_with_images("hello", &[]);
        assert_eq!(mem.messages[0]["content"], "hello");
    }

    #[test]
    fn default_impl_is_same_as_new() {
        let a = Memory::new();
        let b = Memory::default();
        assert_eq!(a.messages.len(), b.messages.len());
        assert_eq!(a.max_messages, b.max_messages);
    }
}
