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
    } else if state.is_thinking {
        Span::styled("⏳ ", Style::default().fg(theme::CLR_CYAN).bg(bg))
    } else {
        // JSX: 用户输入用 › 符号，琥珀色
        Span::styled("› ", Style::default().fg(theme::CLR_AMBER).bg(bg))
    };

    let placeholder = if state.input.is_empty() && !state.is_thinking && state.modal.is_none() {
        Span::styled(
            "输入指令，按 Enter 发送…",
            Style::default().fg(theme::CLR_DIM).bg(bg).add_modifier(Modifier::ITALIC),
        )
    } else {
        Span::styled(state.input.clone(), Style::default().fg(theme::CLR_FG).bg(bg))
    };

    // 右侧 Enter 提示（匹配 JSX）
    let enter_hint = Span::styled(" Enter ↵", Style::default().fg(theme::CLR_DIM).bg(bg));

    let input_line = Line::from(vec![
        prefix,
        placeholder,
        Span::styled(
            " ".repeat(area.width.saturating_sub(state.input.width() as u16 + 10) as usize),
            Style::default().bg(bg),
        ),
        enter_hint,
    ]);

    // 整行背景
    f.render_widget(
        Paragraph::new(input_line).style(Style::default().bg(bg)),
        input_line_area,
    );

    // ── 光标位置 ─────────────────────────────────────────────────────────
    if state.modal.is_none() && !state.is_thinking {
        let prefix_width = 2u16; // "› " 视觉宽度
        let text_before_cursor: String = state.input.chars().take(state.cursor_pos).collect();
        let cursor_x = input_line_area.x
            + prefix_width
            + text_before_cursor.as_str().width() as u16;
        let cursor_y = input_line_area.y;

        // 光标在可见区内时才设置
        if cursor_x < area.x + area.width - 10 { // 留右侧 Enter 提示空间
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