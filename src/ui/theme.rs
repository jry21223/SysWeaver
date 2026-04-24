use ratatui::style::{Color, Modifier, Style};

use crate::types::risk::RiskLevel;

// ── 颜色定义 ────────────────────────────────────────────────────────────────

pub const CLR_BG: Color        = Color::Rgb(18,  18,  24);   // 深蓝黑背景
pub const CLR_BG_PANEL: Color  = Color::Rgb(26,  26,  36);   // 面板背景（略浅）
pub const CLR_BORDER: Color    = Color::Rgb(60,  60,  90);   // 边框灰蓝
pub const CLR_BORDER_HL: Color = Color::Rgb(100, 120, 200);  // 高亮边框（活跃面板）
pub const CLR_STATUSBAR: Color = Color::Rgb(36,  36,  56);   // 状态栏背景

pub const CLR_USER: Color      = Color::Rgb(140, 200, 255);  // 用户消息：天蓝
pub const CLR_AGENT: Color     = Color::Rgb(180, 255, 180);  // Agent 回复：浅绿
pub const CLR_TOOL: Color      = Color::Rgb(255, 210, 100);  // 工具调用：金黄
pub const CLR_SUCCESS: Color   = Color::Rgb(80,  220, 120);  // 成功：绿
pub const CLR_ERROR: Color     = Color::Rgb(255, 100, 100);  // 错误：红
pub const CLR_WARNING: Color   = Color::Rgb(255, 200, 50);   // 警告：黄
pub const CLR_DRYRUN: Color    = Color::Rgb(160, 160, 160);  // DRY-RUN：灰
pub const CLR_DIM: Color       = Color::Rgb(100, 100, 120);  // 次要文字

pub const CLR_MEDIUM: Color    = Color::Rgb(255, 200, 50);
pub const CLR_HIGH: Color      = Color::Rgb(255, 140, 60);
pub const CLR_CRITICAL: Color  = Color::Rgb(255, 60,  60);

pub const CLR_GAUGE_BG: Color  = Color::Rgb(40,  40,  60);   // 进度条背景

// ── Style 工厂函数 ────────────────────────────────────────────────────────

pub fn style_user() -> Style {
    Style::default().fg(CLR_USER).add_modifier(Modifier::BOLD)
}

pub fn style_agent() -> Style {
    Style::default().fg(CLR_AGENT)
}

pub fn style_tool() -> Style {
    Style::default().fg(CLR_TOOL).add_modifier(Modifier::BOLD)
}

pub fn style_success() -> Style {
    Style::default().fg(CLR_SUCCESS)
}

pub fn style_error() -> Style {
    Style::default().fg(CLR_ERROR).add_modifier(Modifier::BOLD)
}

pub fn style_warning() -> Style {
    Style::default().fg(CLR_WARNING)
}

pub fn style_dryrun() -> Style {
    Style::default().fg(CLR_DRYRUN).add_modifier(Modifier::ITALIC)
}

pub fn style_dim() -> Style {
    Style::default().fg(CLR_DIM)
}

pub fn style_border() -> Style {
    Style::default().fg(CLR_BORDER)
}

pub fn style_border_active() -> Style {
    Style::default().fg(CLR_BORDER_HL)
}

pub fn style_statusbar() -> Style {
    Style::default().fg(Color::White).bg(CLR_STATUSBAR)
}

#[allow(dead_code)] // 预留给未来快捷键徽章渲染
pub fn style_statusbar_key() -> Style {
    Style::default()
        .fg(Color::Black)
        .bg(CLR_BORDER_HL)
        .add_modifier(Modifier::BOLD)
}

/// 根据风险等级返回对应样式
pub fn style_for_risk(level: &RiskLevel) -> Style {
    match level {
        RiskLevel::Safe | RiskLevel::Low => style_success(),
        RiskLevel::Medium => style_warning(),
        RiskLevel::High => Style::default()
            .fg(CLR_HIGH)
            .add_modifier(Modifier::BOLD),
        RiskLevel::Critical => Style::default()
            .fg(CLR_CRITICAL)
            .add_modifier(Modifier::BOLD | Modifier::RAPID_BLINK),
    }
}

/// 根据风险等级返回弹窗背景色
pub fn bg_for_risk(level: &RiskLevel) -> Color {
    match level {
        RiskLevel::Safe | RiskLevel::Low => Color::Rgb(20, 60, 30),
        RiskLevel::Medium => Color::Rgb(60, 50, 10),
        RiskLevel::High => Color::Rgb(70, 35, 10),
        RiskLevel::Critical => Color::Rgb(80, 10, 10),
    }
}

/// 根据使用率返回进度条颜色（绿→黄→红）
pub fn gauge_color_by_pct(pct: f64) -> Color {
    if pct < 0.6 {
        CLR_SUCCESS
    } else if pct < 0.85 {
        CLR_WARNING
    } else {
        CLR_ERROR
    }
}
