pub mod app;
pub mod renderer;
pub mod state;
pub mod theme;
pub mod widgets;

pub use app::run_tui;

use tokio::sync::oneshot;

use crate::agent::memory::SystemContext;
use crate::types::risk::RiskLevel;

/// Agent → TUI 的事件流（通信协议核心，定义后不随意改）
pub enum AgentEvent {
    /// LLM 正在思考（显示 spinner）
    Thinking,

    /// 工具调用开始
    ToolCall {
        step: usize,
        tool: String,
        args: String,
        dry_run: bool,
    },

    /// 工具执行结果
    ToolResult {
        success: bool,
        preview: String,
        duration_ms: u64,
    },

    /// LLM 最终回复（任务完成）
    AgentReply(String),

    /// 错误消息
    Error(String),

    /// HIGH RISK 确认请求（agent 在 confirm_rx.await 处阻塞等待）
    RiskPrompt {
        tool: String,
        command_preview: String,
        risk_level: RiskLevel,
        reason: String,
        impact: String,
        alternative: Option<String>,
        confirm_tx: oneshot::Sender<bool>,
    },

    /// 系统状态更新（启动时 + 每 5 步操作后）
    SystemUpdate(SystemContext),

    /// Watchdog 告警（后台监控触发）
    WatchdogAlert {
        severity: String,
        message: String,
    },

    /// 多步任务进度更新
    StepProgress {
        step: usize,
        task_hint: String,
    },
}
