use ratatui::style::{Color, Modifier, Style};

use crate::types::risk::RiskLevel;

// ── 颜色定义（精确匹配 JSX 设计稿 oklch → sRGB）─────────────────────────
// 设计目标：Tokyo Night 冷暗底 + 暖琥珀强调，整体冷静专业。
// 背景刻意压低明度（L≤0.20），让琥珀/青色 accent 更跳。

// AMBER = oklch(0.78 0.14 75) — 用户消息、模式徽章、活跃 tab
pub const CLR_AMBER: Color    = Color::Rgb(220, 175, 95);
// CYAN = oklch(0.78 0.14 220) — Agent 消息、系统标题、sparkline
pub const CLR_CYAN: Color     = Color::Rgb(85, 200, 230);
// DIM = oklch(0.45 0.01 260) — 弱化提示
pub const CLR_DIM: Color      = Color::Rgb(95, 97, 105);
// FG = oklch(0.88 0.01 260) — 主文字（带极弱的蓝绿冷调）
pub const CLR_FG: Color       = Color::Rgb(216, 218, 224);
// FG_MUTED = oklch(0.62 0.01 260) — 次要文字
pub const CLR_FG_MUTED: Color = Color::Rgb(140, 143, 150);
// BG = #1e212b — 主背景（与 RATATUI_SPEC 第 53 行严格一致）
pub const CLR_BG: Color       = Color::Rgb(30, 33, 43);
// BG_PANEL = #252936 — 侧栏/活跃 tab 背景（与 spec 第 54 行一致）
pub const CLR_BG_PANEL: Color = Color::Rgb(37, 41, 54);
// BG_INPUT = #181b24 — 输入栏（最深一档，与 spec 第 55 行一致）
pub const CLR_BG_INPUT: Color = Color::Rgb(24, 27, 36);
// RED = oklch(0.65 0.2 25) — 危险/高占用
pub const CLR_RED: Color      = Color::Rgb(225, 90, 70);
// GREEN = oklch(0.7 0.17 145) — 成功/活跃服务
pub const CLR_GREEN: Color    = Color::Rgb(50, 195, 110);
// BORDER = oklch(0.28 0.02 260) — 分隔线
pub const CLR_BORDER: Color   = Color::Rgb(48, 51, 60);

// ── 衍生角色色（与主调统一冷静走向）─────────────────────────────────────
pub const CLR_SSH: Color           = Color::Rgb(70, 165, 235);   // SSH 徽章（与 CYAN 同源稍偏蓝）
pub const CLR_WATCHDOG: Color      = Color::Rgb(232, 165, 65);   // watchdog 严重等级（同 AMBER 系）
pub const CLR_WATCHDOG_MSG: Color  = Color::Rgb(238, 200, 110);  // watchdog 消息体 / "处理中"
pub const CLR_PROGRESS_MID: Color  = Color::Rgb(180, 200, 90);   // oklch(0.75 0.16 110) 黄绿过渡
pub const CLR_PROVIDER: Color      = Color::Rgb(160, 165, 178);  // provider/model 文本
pub const CLR_HINT: Color          = Color::Rgb(150, 152, 175);  // 任务提示
pub const CLR_COPY_LINK: Color     = Color::Rgb(120, 160, 130);  // "复制" 链接
pub const CLR_COPY_OK_BG: Color    = Color::Rgb(60, 200, 110);   // "✓ 已复制" 提示底色（贴近 GREEN）
pub const CLR_IMPACT: Color        = Color::Rgb(212, 175, 130);  // modal 影响 tan
pub const CLR_SUGGESTION: Color    = Color::Rgb(150, 210, 175);  // modal 建议 mint
pub const CLR_BTN_BORDER: Color    = Color::Rgb(48, 51, 60);     // modal 按钮分隔线（同 BORDER）
pub const CLR_BTN_BG_INACTIVE: Color = Color::Rgb(38, 41, 50);   // modal 未选中按钮背景
pub const CLR_BTN_FG_INACTIVE: Color = Color::Rgb(140, 143, 150);// modal 未选中按钮文字（同 FG_MUTED）
pub const CLR_BTN_BG_DARK: Color   = Color::Rgb(28, 30, 38);     // modal 禁用按钮背景（同 BG_PANEL）

// ── 保留旧常量名（兼容现有代码）────────────────────────────────────────────
pub const CLR_TOOL: Color      = CLR_AMBER;    // 工具调用：琥珀色
pub const CLR_SUCCESS: Color   = CLR_GREEN;    // 成功：绿
pub const CLR_ERROR: Color     = CLR_RED;      // 错误：红
pub const CLR_WARNING: Color   = Color::Rgb(232, 195, 70);   // 警告：金黄（与 AMBER 同源更亮）
pub const CLR_DRYRUN: Color    = Color::Rgb(140, 143, 150);  // DRY-RUN：灰（同 FG_MUTED）
pub const CLR_BORDER_HL: Color = CLR_AMBER;    // 高亮边框：琥珀色
// BG_STATUS = #282c3a — 状态栏背景（与 RATATUI_SPEC 第 56 行一致）
pub const CLR_STATUSBAR: Color = Color::Rgb(40, 44, 58);

pub const CLR_MEDIUM: Color    = Color::Rgb(255, 200, 50);
pub const CLR_HIGH: Color      = Color::Rgb(255, 140, 60);
pub const CLR_CRITICAL: Color  = Color::Rgb(255, 60, 60);

// ── Style 工厂函数 ────────────────────────────────────────────────────────

pub fn style_error() -> Style {
    Style::default().fg(CLR_ERROR).add_modifier(Modifier::BOLD)
}

pub fn style_tool() -> Style {
    Style::default().fg(CLR_TOOL).add_modifier(Modifier::BOLD)
}

pub fn style_success() -> Style {
    Style::default().fg(CLR_SUCCESS)
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

