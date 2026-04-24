use chrono::{DateTime, Utc};
use tokio::sync::oneshot;

use crate::agent::memory::SystemContext;
use crate::types::risk::RiskLevel;

/// 整个 TUI 的全量渲染状态（唯一数据源）
pub struct AppState {
    // ── 左侧对话区 ──────────────────────────────────────────────
    pub messages: Vec<ChatLine>,
    pub scroll_offset: usize,

    // ── 右侧状态面板 ────────────────────────────────────────────
    pub system_ctx: Option<SystemContext>,
    pub ops_history: Vec<OpRecord>,

    // ── 底部输入框 ──────────────────────────────────────────────
    pub input: String,
    pub cursor_pos: usize,      // 字符索引（非字节）

    // ── 状态栏 ──────────────────────────────────────────────────
    pub mode: String,           // safe / normal / auto
    pub provider: String,       // claude-sonnet-4-5 等
    pub session_id: String,
    pub username: String,       // 系统登录用户名
    pub tips_tick: u64,         // tips 轮播计数（每 80ms +1）

    // ── 弹窗（Some = 显示，None = 正常界面）────────────────────
    pub modal: Option<ModalState>,

    // ── 交互状态 ────────────────────────────────────────────────
    pub is_thinking: bool,      // true = 显示 spinner
    pub spinner_frame: usize,   // spinner 动画帧

    // ── 输入历史（Ctrl+P/N 导航）────────────────────────────────
    pub input_history: Vec<String>,
    pub history_idx: Option<usize>,  // None = 未在导航中

    // ── 任务步骤进度 ──────────────────────────────────────────────
    pub task_step: usize,
    pub task_hint: String,

    // ── 最后一条 Agent 回复（供 Ctrl+Y 复制用）──────────────────
    pub last_agent_reply: String,

    // ── 复制通知倒计时（帧数）────────────────────────────────────
    pub copy_notice_frames: u8,

    // ── 语音 TTS 开关 ─────────────────────────────────────────────
    pub voice_tts_enabled: bool,
}

/// 对话区一行的内容类型
pub enum ChatLine {
    UserMsg(String),
    AgentMsg(String),
    ToolCallLine {
        step: usize,
        tool: String,
        args: String,
        dry_run: bool,
    },
    ToolResultLine {
        success: bool,
        preview: String,
        duration_ms: u64,
    },
    ErrorLine(String),
    Separator,
    WatchdogAlert {
        severity: String,
        message: String,
    },
}

/// 右侧面板的操作历史条目
pub struct OpRecord {
    pub tool: String,
    pub success: bool,
    #[allow(dead_code)] // 预留用于操作历史时间线显示
    pub timestamp: DateTime<Utc>,
}

/// HIGH RISK 弹窗状态
pub struct ModalState {
    pub tool: String,
    pub command_preview: String,
    pub risk_level: RiskLevel,
    pub reason: String,
    pub impact: String,
    pub alternative: Option<String>,
    pub confirm_tx: Option<oneshot::Sender<bool>>,
    /// true = "Yes" 高亮，false = "No" 高亮（默认 false）
    pub selected_yes: bool,
}

impl AppState {
    pub fn new(mode: String, provider: String, session_id: String, username: String) -> Self {
        Self {
            messages: Vec::new(),
            scroll_offset: usize::MAX, // 初始自动滚到底
            system_ctx: None,
            ops_history: Vec::new(),
            input: String::new(),
            cursor_pos: 0,
            mode,
            provider,
            session_id,
            username,
            tips_tick: 0,
            modal: None,
            is_thinking: false,
            spinner_frame: 0,
            input_history: Vec::new(),
            history_idx: None,
            task_step: 0,
            task_hint: String::new(),
            last_agent_reply: String::new(),
            copy_notice_frames: 0,
            voice_tts_enabled: false,
        }
    }

    /// 添加一行到对话区，并自动滚到底
    pub fn push_line(&mut self, line: ChatLine) {
        if let ChatLine::AgentMsg(ref text) = line {
            self.last_agent_reply = text.clone();
        }
        self.messages.push(line);
        self.scroll_to_bottom();
    }

    /// 将最后一条 Agent 回复复制到系统剪贴板（macOS/Linux/Windows 兼容）
    pub fn copy_last_reply_to_clipboard(&self) -> bool {
        if self.last_agent_reply.is_empty() {
            return false;
        }
        write_to_clipboard(&self.last_agent_reply)
    }

    /// 将完整对话历史导出为纯文本字符串（用于保存文件）
    pub fn export_chat_as_text(&self) -> String {
        let mut out = String::new();
        for msg in &self.messages {
            match msg {
                ChatLine::UserMsg(text) => {
                    out.push_str("【你】\n");
                    out.push_str(text);
                    out.push_str("\n\n");
                }
                ChatLine::AgentMsg(text) => {
                    out.push_str("【Agent】\n");
                    out.push_str(text);
                    out.push_str("\n\n");
                }
                ChatLine::ToolCallLine { step, tool, args, .. } => {
                    out.push_str(&format!("[Step {}] 工具: {}  参数: {}\n", step, tool, args));
                }
                ChatLine::ToolResultLine { success, preview, duration_ms } => {
                    let status = if *success { "✓" } else { "✗" };
                    out.push_str(&format!("{} {}ms\n{}\n", status, duration_ms, preview));
                }
                ChatLine::ErrorLine(msg) => {
                    out.push_str(&format!("[错误] {}\n\n", msg));
                }
                ChatLine::Separator => {
                    out.push_str("────────────────────────\n");
                }
                ChatLine::WatchdogAlert { severity, message } => {
                    out.push_str(&format!("[告警 {}] {}\n\n", severity, message));
                }
            }
        }
        out
    }

    /// 将完整对话导出到文件，返回保存路径
    pub fn export_to_file(&self) -> Result<String, String> {
        let home = std::env::var("HOME")
            .or_else(|_| std::env::var("USERPROFILE"))
            .unwrap_or_else(|_| ".".to_string());
        let timestamp = chrono::Utc::now().format("%Y%m%d_%H%M%S");
        let path = format!("{}/jij_chat_{}.txt", home, timestamp);
        let content = self.export_chat_as_text();
        std::fs::write(&path, &content).map_err(|e| e.to_string())?;
        Ok(path)
    }

    /// 递减复制通知倒计时（每帧调用）
    pub fn tick_copy_notice(&mut self) {
        self.copy_notice_frames = self.copy_notice_frames.saturating_sub(1);
    }

    /// 滚动到对话区底部
    pub fn scroll_to_bottom(&mut self) {
        self.scroll_offset = usize::MAX;
    }

    /// 向上滚动（返回是否实际发生了滚动）
    pub fn scroll_up(&mut self, lines: usize) {
        self.scroll_offset = self.scroll_offset.saturating_sub(lines);
    }

    /// 向下滚动
    pub fn scroll_down(&mut self, lines: usize) {
        self.scroll_offset = self.scroll_offset.saturating_add(lines);
    }

    /// 光标前进（字符级）
    pub fn cursor_right(&mut self) {
        let chars: Vec<char> = self.input.chars().collect();
        if self.cursor_pos < chars.len() {
            self.cursor_pos += 1;
        }
    }

    /// 光标后退
    pub fn cursor_left(&mut self) {
        self.cursor_pos = self.cursor_pos.saturating_sub(1);
    }

    /// 在光标位置插入字符
    pub fn insert_char(&mut self, c: char) {
        let mut chars: Vec<char> = self.input.chars().collect();
        chars.insert(self.cursor_pos, c);
        self.input = chars.into_iter().collect();
        self.cursor_pos += 1;
    }

    /// 删除光标前一个字符（Backspace）
    pub fn delete_before_cursor(&mut self) {
        if self.cursor_pos == 0 {
            return;
        }
        let mut chars: Vec<char> = self.input.chars().collect();
        chars.remove(self.cursor_pos - 1);
        self.input = chars.into_iter().collect();
        self.cursor_pos -= 1;
    }

    /// 取出并清空输入框内容，同时保存到历史
    pub fn take_input(&mut self) -> String {
        self.cursor_pos = 0;
        self.history_idx = None;
        let text = std::mem::take(&mut self.input);
        if !text.trim().is_empty() {
            // 避免重复添加相同的最后一条
            let is_dup = self.input_history.last().map(|s| s == &text).unwrap_or(false);
            if !is_dup {
                self.input_history.push(text.clone());
                // 保留最近 50 条
                if self.input_history.len() > 50 {
                    self.input_history.remove(0);
                }
            }
        }
        text
    }

    /// 向上浏览历史（Ctrl+P）
    pub fn history_prev(&mut self) {
        if self.input_history.is_empty() {
            return;
        }
        let new_idx = match self.history_idx {
            None => self.input_history.len() - 1,
            Some(0) => 0,
            Some(i) => i - 1,
        };
        self.history_idx = Some(new_idx);
        let hist = self.input_history[new_idx].clone();
        self.input = hist;
        let char_len = self.input.chars().count();
        self.cursor_pos = char_len;
    }

    /// 向下浏览历史（Ctrl+N）
    pub fn history_next(&mut self) {
        match self.history_idx {
            None => {}
            Some(i) if i + 1 >= self.input_history.len() => {
                self.history_idx = None;
                self.input.clear();
                self.cursor_pos = 0;
            }
            Some(i) => {
                self.history_idx = Some(i + 1);
                let hist = self.input_history[i + 1].clone();
                self.input = hist;
                let char_len = self.input.chars().count();
                self.cursor_pos = char_len;
            }
        }
    }

    /// 记录一条操作到右侧面板历史（保留最近 10 条）
    pub fn push_op(&mut self, tool: String, success: bool) {
        self.ops_history.push(OpRecord {
            tool,
            success,
            timestamp: Utc::now(),
        });
        // 最多保留 10 条
        if self.ops_history.len() > 10 {
            self.ops_history.remove(0);
        }
    }

    /// 弹出 HIGH RISK 弹窗
    pub fn show_modal(
        &mut self,
        tool: String,
        command_preview: String,
        risk_level: RiskLevel,
        reason: String,
        impact: String,
        alternative: Option<String>,
        confirm_tx: oneshot::Sender<bool>,
    ) {
        self.modal = Some(ModalState {
            tool,
            command_preview,
            risk_level,
            reason,
            impact,
            alternative,
            confirm_tx: Some(confirm_tx),
            selected_yes: false, // 默认选 No（保守）
        });
    }

    /// 关闭弹窗并发送确认结果
    pub fn close_modal(&mut self, confirmed: bool) {
        if let Some(mut modal) = self.modal.take() {
            if let Some(tx) = modal.confirm_tx.take() {
                let _ = tx.send(confirmed);
            }
        }
    }

    /// 推进 tips 轮播帧（每次 spinner_tick 调用）
    pub fn tick_tips(&mut self) {
        self.tips_tick = self.tips_tick.wrapping_add(1);
    }

    /// 推进 spinner 帧
    pub fn tick_spinner(&mut self) {
        self.spinner_frame = self.spinner_frame.wrapping_add(1);
    }

    /// 获取当前 spinner 字符
    pub fn spinner_char(&self) -> &str {
        const FRAMES: &[&str] = &["⠋", "⠙", "⠹", "⠸", "⠼", "⠴", "⠦", "⠧", "⠇", "⠏"];
        FRAMES[self.spinner_frame % FRAMES.len()]
    }
}

/// 跨平台剪贴板写入（macOS: pbcopy，Linux: xclip/xsel，Windows: clip.exe）
fn write_to_clipboard(text: &str) -> bool {
    use std::io::Write;

    #[cfg(target_os = "macos")]
    {
        return std::process::Command::new("pbcopy")
            .stdin(std::process::Stdio::piped())
            .spawn()
            .and_then(|mut child| {
                if let Some(stdin) = child.stdin.as_mut() {
                    let _ = stdin.write_all(text.as_bytes());
                }
                child.wait()
            })
            .map(|s| s.success())
            .unwrap_or(false);
    }

    #[cfg(target_os = "windows")]
    {
        return std::process::Command::new("clip")
            .stdin(std::process::Stdio::piped())
            .spawn()
            .and_then(|mut child| {
                if let Some(stdin) = child.stdin.as_mut() {
                    let _ = stdin.write_all(text.as_bytes());
                }
                child.wait()
            })
            .map(|s| s.success())
            .unwrap_or(false);
    }

    #[cfg(not(any(target_os = "macos", target_os = "windows")))]
    {
        // Linux: try xclip then xsel
        let ok = std::process::Command::new("xclip")
            .args(["-selection", "clipboard"])
            .stdin(std::process::Stdio::piped())
            .spawn()
            .and_then(|mut child| {
                if let Some(stdin) = child.stdin.as_mut() {
                    let _ = stdin.write_all(text.as_bytes());
                }
                child.wait()
            })
            .map(|s| s.success())
            .unwrap_or(false);
        if ok {
            return true;
        }
        std::process::Command::new("xsel")
            .args(["--clipboard", "--input"])
            .stdin(std::process::Stdio::piped())
            .spawn()
            .and_then(|mut child| {
                if let Some(stdin) = child.stdin.as_mut() {
                    let _ = stdin.write_all(text.as_bytes());
                }
                child.wait()
            })
            .map(|s| s.success())
            .unwrap_or(false)
    }
}
