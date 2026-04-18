use thiserror::Error;

// Error variants are defined here for structured error handling; not all are raised yet
#[allow(dead_code)]
#[derive(Error, Debug)]
pub enum AgentError {
    #[error("LLM API 调用失败: {0}")]
    LlmError(String),

    #[error("工具执行失败 [{tool}]: {message}")]
    ToolError { tool: String, message: String },

    #[error("操作被安全策略拒绝: {reason}")]
    SecurityBlocked { reason: String },

    #[error("操作被用户取消")]
    UserCancelled,

    #[error("执行超时（超过 {timeout_secs}s）")]
    Timeout { timeout_secs: u64 },

    #[error("JSON 解析失败: {0}")]
    ParseError(#[from] serde_json::Error),

    #[error("IO 错误: {0}")]
    IoError(#[from] std::io::Error),

    #[error("超过最大执行步数 {max_steps}")]
    MaxStepsExceeded { max_steps: usize },

    #[error("SSH 连接失败: {0}")]
    SshError(String),

    #[error("未知工具: {tool_name}")]
    UnknownTool { tool_name: String },
}
