pub mod file;
pub mod shell;
pub mod system;

use crate::types::tool::{ToolCall, ToolResult};
use anyhow::Result;
use async_trait::async_trait;

/// 所有工具必须实现的统一接口
#[async_trait]
pub trait Tool: Send + Sync {
    /// 工具名称，格式: "namespace.action"
    fn name(&self) -> &str;
    /// 工具功能描述（发送给 LLM）
    fn description(&self) -> &str;
    /// 工具参数的 JSON Schema（发送给 LLM）
    fn schema(&self) -> serde_json::Value;
    /// 执行工具
    async fn execute(&self, args: &serde_json::Value, dry_run: bool) -> Result<ToolResult>;
}

/// 工具分发器
pub struct ToolManager {
    tools: Vec<Box<dyn Tool>>,
}

impl ToolManager {
    pub fn new() -> Self {
        Self {
            tools: vec![
                Box::new(shell::ShellTool::new()),
                Box::new(file::FileReadTool::new()),
                Box::new(file::FileWriteTool::new()),
                Box::new(file::FileSearchTool::new()),
                Box::new(system::SystemTool::new()),
            ],
        }
    }

    /// 根据 ToolCall 分发到对应工具执行
    pub async fn dispatch(&self, call: &ToolCall) -> Result<ToolResult> {
        let tool = self
            .tools
            .iter()
            .find(|t| t.name() == call.tool)
            .ok_or_else(|| anyhow::anyhow!("未知工具: {}", call.tool))?;

        tool.execute(&call.args, call.dry_run).await
    }

    /// 生成所有工具的 Schema 列表（用于 LLM tool_use）
    pub fn all_schemas(&self) -> Vec<serde_json::Value> {
        self.tools
            .iter()
            .map(|t| {
                serde_json::json!({
                    "name": t.name(),
                    "description": t.description(),
                    "input_schema": t.schema(),
                })
            })
            .collect()
    }
}

impl Default for ToolManager {
    fn default() -> Self {
        Self::new()
    }
}
