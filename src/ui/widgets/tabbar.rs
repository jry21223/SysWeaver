use ratatui::{
    Frame,
    layout::{Constraint, Layout, Rect},
    style::{Modifier, Style},
    text::{Line, Span},
    widgets::Paragraph,
};

use crate::ui::state::ActiveTab;
use crate::ui::theme;

/// TabBar — spec §4：无边框，颜色区分活跃态
/// 活跃：AMBER + BOLD + BG_PANEL 背景；非活跃：FG_MUTED + BG 背景
pub fn render(f: &mut Frame, area: Rect, active: &ActiveTab) {
    // spec §4: ◇ 对话  ◈ 监控  ◆ 历史
    let tabs = [
        ("◇", "对话", ActiveTab::Chat),
        ("◈", "监控", ActiveTab::Monitor),
        ("◆", "历史", ActiveTab::History),
    ];

    let widths = Layout::horizontal([
        Constraint::Fill(1),
        Constraint::Fill(1),
        Constraint::Fill(1),
    ])
    .split(area);

    for (i, (icon, label, tab_type)) in tabs.iter().enumerate() {
        let is_active = *active == *tab_type;

        let bg_color = if is_active { theme::CLR_BG_PANEL } else { theme::CLR_BG };
        let fg_color = if is_active { theme::CLR_AMBER } else { theme::CLR_FG_MUTED };

        let style = Style::default()
            .fg(fg_color)
            .bg(bg_color)
            .add_modifier(if is_active { Modifier::BOLD } else { Modifier::empty() });

        // spec §4: 无边框，用空格填满整格形成色块效果
        let text = format!(" {} {}  ", icon, label);
        f.render_widget(
            Paragraph::new(Line::from(Span::styled(text, style))),
            widths[i],
        );
    }
}
