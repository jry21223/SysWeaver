use ratatui::{
    Frame,
    layout::Rect,
    style::{Color, Modifier, Style},
    text::{Line, Span},
    widgets::Paragraph,
};

use crate::ui::state::AppState;
use crate::ui::theme;

pub fn render(f: &mut Frame, area: Rect, state: &AppState) {
    let mode_color = match state.mode.as_str() {
        "safe"   => Color::Rgb(80, 220, 120),
        "normal" => Color::Rgb(100, 160, 255),
        "auto"   => Color::Rgb(255, 200, 50),
        _        => Color::Gray,
    };

    let mut spans = vec![
        Span::styled(" 🤖 Agent Unix ", Style::default()
            .fg(Color::White)
            .bg(theme::CLR_STATUSBAR)
            .add_modifier(Modifier::BOLD)),
        Span::styled(" │ ", theme::style_dim()),
        Span::styled(
            format!(" {} ", state.mode.to_uppercase()),
            Style::default().fg(Color::Black).bg(mode_color).add_modifier(Modifier::BOLD),
        ),
        Span::styled(" │ ", theme::style_dim()),
        Span::styled(
            format!(" {} ", state.provider),
            Style::default().fg(Color::Rgb(180, 180, 200)).bg(theme::CLR_STATUSBAR),
        ),
        Span::styled(" │ ", theme::style_dim()),
        Span::styled(
            format!(" sess:{} ", &state.session_id[..8.min(state.session_id.len())]),
            theme::style_dim(),
        ),
    ];

    // 若正在思考，显示步骤进度 + spinner
    if state.is_thinking {
        spans.push(Span::styled(" │ ", theme::style_dim()));
        if state.task_step > 0 {
            spans.push(Span::styled(
                format!(" Step {} ", state.task_step),
                Style::default()
                    .fg(Color::Rgb(100, 220, 255))
                    .bg(theme::CLR_STATUSBAR)
                    .add_modifier(Modifier::BOLD),
            ));
            if !state.task_hint.is_empty() {
                let hint_short: String = state.task_hint.chars().take(20).collect();
                spans.push(Span::styled(
                    format!("• {} ", hint_short),
                    Style::default().fg(Color::Rgb(160, 160, 200)).bg(theme::CLR_STATUSBAR),
                ));
            }
        }
        spans.push(Span::styled(
            format!(" {} 处理中… ", state.spinner_char()),
            Style::default()
                .fg(Color::Rgb(255, 220, 100))
                .bg(theme::CLR_STATUSBAR)
                .add_modifier(Modifier::BOLD),
        ));
    }

    // 右侧快捷键提示
    let hints = vec![
        Span::styled(" /exit", theme::style_statusbar_key()),
        Span::styled(" 退出 ", theme::style_dim()),
        Span::styled(" /undo", theme::style_statusbar_key()),
        Span::styled(" 撤销 ", theme::style_dim()),
        Span::styled(" PgUp/Dn", theme::style_statusbar_key()),
        Span::styled(" 滚动 ", theme::style_dim()),
    ];
    spans.extend(hints);

    let line = Line::from(spans);
    let para = Paragraph::new(line).style(theme::style_statusbar());
    f.render_widget(para, area);
}
