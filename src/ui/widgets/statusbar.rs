use ratatui::{
    Frame,
    layout::{Constraint, Layout, Rect},
    style::{Color, Modifier, Style},
    text::{Line, Span},
    widgets::Paragraph,
};

use crate::ui::state::AppState;
use crate::ui::theme;

// 右侧固定快捷键区，显示宽度：/exit(5) + ` │ `(3) + Ctrl+Y(6) + ` `(1)
// + 复制(4=2CJK×2) + ` │ `(3) + PgUp/Dn(8) + 两端各1空格 ≈ 33
const RIGHT_WIDTH: u16 = 33;

pub fn render(f: &mut Frame, area: Rect, state: &AppState) {
    // ── 左侧内容 ─────────────────────────────────────────────────────────
    let mut left_spans = vec![
        Span::styled(
            format!(" ● {} ", state.username),
            Style::default()
                .fg(Color::Rgb(212, 160, 74))
                .bg(theme::CLR_STATUSBAR)
                .add_modifier(Modifier::BOLD),
        ),
        Span::styled(
            format!(" {} ", state.mode.to_uppercase()),
            Style::default().fg(Color::Black).bg(theme::CLR_AMBER).add_modifier(Modifier::BOLD),
        ),
        Span::styled(" ", theme::style_statusbar()),
        Span::styled(
            format!("{}", state.provider),
            Style::default().fg(Color::Rgb(180, 180, 200)).bg(theme::CLR_STATUSBAR),
        ),
        Span::styled(
            format!("  sess:{} ", &state.session_id[..8.min(state.session_id.len())]),
            theme::style_dim(),
        ),
    ];

    // 思考中：显示 spinner + 步骤
    if state.is_thinking {
        left_spans.push(Span::styled(" │ ", theme::style_dim()));
        if state.task_step > 0 {
            left_spans.push(Span::styled(
                format!(" Step {} ", state.task_step),
                Style::default()
                    .fg(Color::Rgb(100, 220, 255))
                    .bg(theme::CLR_STATUSBAR)
                    .add_modifier(Modifier::BOLD),
            ));
            if !state.task_hint.is_empty() {
                let hint_short: String = state.task_hint.chars().take(20).collect();
                left_spans.push(Span::styled(
                    format!("• {} ", hint_short),
                    Style::default().fg(Color::Rgb(160, 160, 200)).bg(theme::CLR_STATUSBAR),
                ));
            }
        }
        left_spans.push(Span::styled(
            format!(" {} 处理中… ", state.spinner_char()),
            Style::default()
                .fg(Color::Rgb(255, 220, 100))
                .bg(theme::CLR_STATUSBAR)
                .add_modifier(Modifier::BOLD),
        ));
    }

    // 语音 TTS 状态
    if state.voice_tts_enabled {
        left_spans.push(Span::styled(" │ ", theme::style_dim()));
        left_spans.push(Span::styled(
            " 🔊 TTS ",
            Style::default()
                .fg(Color::Black)
                .bg(Color::Rgb(80, 200, 120))
                .add_modifier(Modifier::BOLD),
        ));
    }

    // 复制成功通知
    if state.copy_notice_frames > 0 {
        left_spans.push(Span::styled(" │ ", theme::style_dim()));
        left_spans.push(Span::styled(
            " ✓ 已复制 ",
            Style::default()
                .fg(Color::Black)
                .bg(Color::Rgb(100, 220, 100))
                .add_modifier(Modifier::BOLD),
        ));
    }

    // ── 右侧固定快捷键（匹配 JSX StatusBar 设计）────────────────────────
    let dim = theme::style_dim();
    let sep = Span::styled(" │ ", dim);
    let right_spans = vec![
        Span::styled(" /exit", dim),
        sep.clone(),
        Span::styled("Ctrl+Y", dim),
        Span::styled(" 复制", Style::default().fg(Color::Rgb(130, 170, 130)).bg(theme::CLR_STATUSBAR)),
        sep.clone(),
        Span::styled("PgUp/Dn ", dim),
    ];

    // ── 布局 ──────────────────────────────────────────────────────────────
    let right_w = RIGHT_WIDTH.min(area.width / 2);
    let [left_area, right_area] = Layout::horizontal([
        Constraint::Fill(1),
        Constraint::Length(right_w),
    ])
    .areas(area);

    f.render_widget(
        Paragraph::new(Line::from(left_spans)).style(theme::style_statusbar()),
        left_area,
    );
    f.render_widget(
        Paragraph::new(Line::from(right_spans)).style(theme::style_statusbar()),
        right_area,
    );
}
