use ratatui::{
    Frame,
    layout::{Constraint, Layout, Rect},
    style::{Color, Modifier, Style},
    text::{Line, Span},
    widgets::{Block, BorderType, Borders, Paragraph},
};
use unicode_width::UnicodeWidthStr;

use crate::ui::state::AppState;
use crate::ui::theme;

pub fn render(f: &mut Frame, area: Rect, state: &AppState) {
    // 外框
    let block = Block::default()
        .borders(Borders::ALL)
        .border_type(BorderType::Rounded)
        .border_style(if state.modal.is_none() {
            theme::style_border_active()
        } else {
            theme::style_border()
        })
        .style(Style::default().bg(theme::CLR_BG_PANEL));

    let inner = block.inner(area);
    f.render_widget(block, area);

    // 切分：输入行 / 提示行
    let [input_line_area, hint_line_area] = Layout::vertical([
        Constraint::Length(1),
        Constraint::Length(1),
    ])
    .areas(inner);

    // ── 输入行 ────────────────────────────────────────────────────────────
    let prefix = if state.modal.is_some() {
        Span::styled(" 🔒 ", Style::default().fg(Color::Gray))
    } else if state.is_thinking {
        Span::styled(" ⏳ ", Style::default().fg(Color::Rgb(150, 150, 200)))
    } else {
        Span::styled(" 👤 ", theme::style_user())
    };

    let input_display: String = if state.input.is_empty() && !state.is_thinking {
        String::new()
    } else {
        state.input.clone()
    };

    let placeholder = if state.input.is_empty() && !state.is_thinking && state.modal.is_none() {
        Span::styled(
            "输入指令，按 Enter 发送…",
            Style::default().fg(Color::Rgb(80, 80, 100)).add_modifier(Modifier::ITALIC),
        )
    } else {
        Span::styled(input_display.clone(), Style::default().fg(Color::White))
    };

    let input_line = Line::from(vec![prefix, placeholder]);
    f.render_widget(Paragraph::new(input_line), input_line_area);

    // ── 光标位置 ─────────────────────────────────────────────────────────
    // 只在非弹窗、非 thinking 时显示光标
    if state.modal.is_none() && !state.is_thinking {
        let prefix_width = 3u16; // " 👤 " 视觉宽度约 3（emoji 算 2，空格 1）
        let text_before_cursor: String = state.input.chars().take(state.cursor_pos).collect();
        let cursor_x = input_line_area.x
            + prefix_width
            + text_before_cursor.as_str().width() as u16;
        let cursor_y = input_line_area.y;

        // 光标在可见区内时才设置
        if cursor_x < area.x + area.width {
            f.set_cursor_position((cursor_x, cursor_y));
        }
    }

    // ── 提示行 ────────────────────────────────────────────────────────────
    let hint_text = if state.modal.is_some() {
        " 弹窗模式：Y 确认 · N 取消 · Tab 切换"
    } else if state.is_thinking {
        " Agent 思考中，请稍候…"
    } else {
        " /exit 退出 · /undo 撤销 · /history 历史 · PageUp/Dn 滚动"
    };

    let hint = Paragraph::new(Span::styled(hint_text, theme::style_dim()));
    f.render_widget(hint, hint_line_area);
}
