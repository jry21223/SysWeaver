use ratatui::style::{Color, Modifier, Style};

use crate::types::risk::RiskLevel;

// ── 颜色定义（匹配 JSX 设计 oklch → RGB）────────────────────────────────────
// JSX: AMBER = 'oklch(0.78 0.14 75)' → 温暖琥珀色
pub const CLR_AMBER: Color    = Color::Rgb(212, 160, 74);
// JSX: CYAN = 'oklch(0.78 0.14 220)' → 清冷青色
pub const CLR_CYAN: Color     = Color::Rgb(100, 200, 255);
// JSX: DIM = 'oklch(0.45 0.01 260)' → 次要文字灰蓝
pub const CLR_DIM: Color      = Color::Rgb(100, 100, 120);
// JSX: FG = 'oklch(0.88 0.01 260)' → 主文字浅灰白
pub const CLR_FG: Color       = Color::Rgb(220, 220, 240);
// JSX: FG_MUTED = 'oklch(0.62 0.01 260)' → 次要文字灰
pub const CLR_FG_MUTED: Color = Color::Rgb(150, 150, 170);
// JSX: BG = 'oklch(0.17 0.02 260)' → 深蓝黑背景
pub const CLR_BG: Color       = Color::Rgb(32, 32, 48);
// JSX: BG_PANEL = 'oklch(0.20 0.02 260)' → 面板背景
pub const CLR_BG_PANEL: Color = Color::Rgb(40, 40, 56);
// JSX: BG_INPUT = 'oklch(0.14 0.02 260)' → 输入框背景更深
pub const CLR_BG_INPUT: Color = Color::Rgb(26, 26, 38);
// JSX: RED = 'oklch(0.65 0.2 25)' → 错误红
pub const CLR_RED: Color      = Color::Rgb(220, 80, 80);
// JSX: GREEN = 'oklch(0.7 0.17 145)' → 成功绿
pub const CLR_GREEN: Color    = Color::Rgb(80, 200, 120);
// JSX: BORDER = 'oklch(0.28 0.02 260)' → 边框灰蓝
pub const CLR_BORDER: Color   = Color::Rgb(60, 60, 90);

// ── 保留旧常量名（兼容现有代码）────────────────────────────────────────────
pub const CLR_USER: Color      = CLR_AMBER;    // 用户消息：琥珀色
pub const CLR_AGENT: Color     = CLR_CYAN;     // Agent 回复：青色
pub const CLR_TOOL: Color      = CLR_AMBER;    // 工具调用：琥珀色
pub const CLR_SUCCESS: Color   = CLR_GREEN;    // 成功：绿
pub const CLR_ERROR: Color     = CLR_RED;      // 错误：红
pub const CLR_WARNING: Color   = Color::Rgb(255, 200, 50);   // 警告：黄（保留）
pub const CLR_DRYRUN: Color    = Color::Rgb(160, 160, 160); // DRY-RUN：灰
pub const CLR_BORDER_HL: Color = CLR_AMBER;    // 高亮边框：琥珀色
pub const CLR_STATUSBAR: Color = Color::Rgb(45, 45, 65);   // 状态栏背景（略深于 BG_PANEL）

pub const CLR_MEDIUM: Color    = Color::Rgb(255, 200, 50);
pub const CLR_HIGH: Color      = Color::Rgb(255, 140, 60);
pub const CLR_CRITICAL: Color  = Color::Rgb(255, 60, 60);

pub const CLR_GAUGE_BG: Color  = Color::Rgb(40, 40, 60);   // 进度条背景

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
    Style::default().fg(CLR_FG_MUTED).bg(CLR_STATUSBAR)
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