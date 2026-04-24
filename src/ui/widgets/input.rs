use ratatui::{
    Frame,
    layout::Rect,
    style::{Color, Modifier, Style},
    text::{Line, Span},
    widgets::Paragraph,
};
use unicode_width::UnicodeWidthStr;

use crate::ui::state::AppState;
use crate::ui::theme;

/// InputBar（匹配 JSX 设计）
/// › 输入指令，按 Enter 发送…          Enter ↵
pub fn render(f: &mut Frame, area: Rect, state: &AppState) {
    // JSX: 输入框背景更深，无边框
    let bg = theme::CLR_BG_INPUT;

    // 单行输入框（紧凑设计）
    let input_line_area = Rect {
        x: area.x,
        y: area.y + 1, // 上方留一行空白（作为分隔）
        width: area.width,
        height: 1,
    };

    // ── 输入行 ────────────────────────────────────────────────────────────
    let prefix = if state.modal.is_some() {
        Span::styled("🔒 ", Style::default().fg(Color::Gray).bg(bg))
    } else if state.voice_recording {
        Span::styled("🎙 ", Style::default().fg(theme::CLR_RED).bg(bg))
    } else if state.is_thinking {
        Span::styled("⏳ ", Style::default().fg(theme::CLR_CYAN).bg(bg))
    } else {
        Span::styled("› ", Style::default().fg(theme::CLR_AMBER).bg(bg))
    };

    let placeholder = if state.voice_recording {
        Span::styled(
            "录音中… 再按 F5 停止并转写",
            Style::default().fg(theme::CLR_RED).bg(bg).add_modifier(Modifier::ITALIC),
        )
    } else if state.input.is_empty() && !state.is_thinking && state.modal.is_none() {
        Span::styled(
            "输入指令，按 Enter 发送…  (F5 语音输入)",
            Style::default().fg(theme::CLR_DIM).bg(bg).add_modifier(Modifier::ITALIC),
        )
    } else {
        Span::styled(state.input.clone(), Style::default().fg(theme::CLR_FG).bg(bg))
    };

    // 右侧语音指示器 + Enter 提示
    let voice_indicator = if state.voice_recording {
        Span::styled(" 🔴", Style::default().fg(theme::CLR_RED).bg(bg))
    } else if state.voice_tts_enabled {
        Span::styled(" 🎤", Style::default().fg(theme::CLR_GREEN).bg(bg))
    } else if state.is_remote {
        Span::styled(" 🔇", Style::default().fg(theme::CLR_DIM).bg(bg))
    } else {
        Span::styled("", Style::default().bg(bg))
    };
    let enter_hint = Span::styled(" Enter ↵", Style::default().fg(theme::CLR_DIM).bg(bg));

    let input_line = Line::from(vec![
        prefix,
        placeholder,
        Span::styled(
            " ".repeat(area.width.saturating_sub(state.input.width() as u16 + 14) as usize),
            Style::default().bg(bg),
        ),
        voice_indicator,
        enter_hint,
    ]);

    // 整行背景
    f.render_widget(
        Paragraph::new(input_line).style(Style::default().bg(bg)),
        input_line_area,
    );

    // ── 光标位置 ─────────────────────────────────────────────────────────
    if state.modal.is_none() && !state.is_thinking && !state.voice_recording {
        let prefix_width = 2u16; // "› " 视觉宽度
        let text_before_cursor: String = state.input.chars().take(state.cursor_pos).collect();
        let cursor_x = input_line_area.x
            + prefix_width
            + text_before_cursor.as_str().width() as u16;
        let cursor_y = input_line_area.y;

        // 光标在可见区内时才设置
        if cursor_x < area.x + area.width - 14 { // 留右侧提示空间
            f.set_cursor_position((cursor_x, cursor_y));
        }
    }

    // ── 上方分隔线 ─────────────────────────────────────────────────────────
    let sep_area = Rect {
        x: area.x,
        y: area.y,
        width: area.width,
        height: 1,
    };
    let sep = Span::styled(
        "─".repeat(area.width as usize),
        Style::default().fg(theme::CLR_BORDER).bg(theme::CLR_BG),
    );
    f.render_widget(Paragraph::new(Line::from(sep)), sep_area);
}