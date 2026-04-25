use ratatui::{
    Frame,
    layout::{Constraint, Layout, Rect},
    style::{Modifier, Style},
    text::{Line, Span},
    widgets::{Block, Borders, Paragraph},
};

use crate::ui::state::ActiveTab;
use crate::ui::theme;

/// TabBar — 匹配 JSX 设计稿 per-tab 下划线风格
/// 活跃标签：琥珀色文字 + 琥珀色底部边框 + CLR_BG_PANEL 背景
/// 非活跃标签：灰色文字 + 透明底部边框
pub fn render(f: &mut Frame, area: Rect, active: &ActiveTab) {
    // 设计稿：◇ 对话 / ◈ 监控 / ◆ 历史，由颜色区分活跃态
    let tabs = [
        ("◇", " 对话", ActiveTab::Chat),
        ("◈", " 监控", ActiveTab::Monitor),
        ("◆", " 历史", ActiveTab::History),
    ];

    // 水平均分 3 个标签
    let widths = Layout::horizontal([
        Constraint::Fill(1),
        Constraint::Fill(1),
        Constraint::Fill(1),
    ])
    .split(area);

    for (i, (icon, label, tab_type)) in tabs.iter().enumerate() {
        let is_active = *active == *tab_type;

        let border_color = if is_active {
            theme::CLR_AMBER
        } else {
            theme::CLR_BG
        };

        let bg_color = if is_active {
            theme::CLR_BG_PANEL
        } else {
            theme::CLR_BG
        };

        let fg_color = if is_active {
            theme::CLR_AMBER
        } else {
            theme::CLR_FG_MUTED
        };

        let style = Style::default()
            .fg(fg_color)
            .bg(bg_color)
            .add_modifier(if is_active { Modifier::BOLD } else { Modifier::empty() });

        let block = Block::default()
            .borders(Borders::BOTTOM)
            .border_style(Style::default().fg(border_color));

        let text = format!(" {}{} ", icon, label);
        let para = Paragraph::new(Line::from(Span::styled(text, style)))
            .block(block);

        f.render_widget(para, widths[i]);
    }
}
