use ratatui::{
    Frame,
    layout::Rect,
    style::{Modifier, Style},
    text::{Line, Span},
    widgets::{Block, Borders, Paragraph},
};

use crate::ui::state::AppState;
use crate::ui::theme;

/// 操作历史面板（匹配 JSX 设计稿 HistoryPanel）
pub fn render(f: &mut Frame, area: Rect, state: &AppState) {
    let inner = Block::default()
        .style(Style::default().bg(theme::CLR_BG))
        .borders(Borders::empty());
    let inner_area = inner.inner(area);
    f.render_widget(inner, area);

    // ── 标题行 ──
    let header = Paragraph::new(Line::from(vec![
        Span::styled(
            "  操作历史",
            Style::default()
                .fg(theme::CLR_CYAN)
                .add_modifier(Modifier::BOLD),
        ),
    ]))
    .style(Style::default().bg(theme::CLR_BG));
    f.render_widget(header, Rect {
        x: inner_area.x,
        y: inner_area.y,
        width: inner_area.width,
        height: 1,
    });

    // 剩余空间
    let content_area = Rect {
        x: inner_area.x,
        y: inner_area.y + 2,
        width: inner_area.width,
        height: inner_area.height.saturating_sub(2),
    };

    if state.ops_history.is_empty() {
        let empty = Paragraph::new(Line::from(Span::styled(
            "  暂无操作记录",
            Style::default().fg(theme::CLR_FG_MUTED),
        )));
        f.render_widget(empty, content_area);
        return;
    }

    // ── 历史条目（最新在前） ──
    let max_rows = content_area.height as usize;
    let items: Vec<&crate::ui::state::OpRecord> = state
        .ops_history
        .iter()
        .rev()
        .take(max_rows.min(20))
        .collect();

    let mut lines: Vec<Line> = Vec::new();
    for rec in &items {
        let time_str = rec.timestamp.format("%H:%M").to_string();
        let status_icon = if rec.success { "✓" } else { "⚠" };
        let status_color = if rec.success {
            theme::CLR_GREEN
        } else {
            theme::CLR_AMBER
        };
        let name_color = if rec.success {
            theme::CLR_FG
        } else {
            theme::CLR_AMBER
        };

        // 截断命令名长度
        let cmd_display: String = rec.tool.chars().take(30).collect();

        let dim = Style::default().fg(theme::CLR_DIM);
        let sep = Span::styled(" │ ", dim);

        lines.push(Line::from(vec![
            Span::styled(
                format!("  {}", time_str),
                Style::default().fg(theme::CLR_DIM),
            ),
            sep.clone(),
            Span::styled(cmd_display, Style::default().fg(name_color)),
            sep.clone(),
            Span::styled(format!("{} ", status_icon), Style::default().fg(status_color)),
        ]));
    }

    let para = Paragraph::new(lines).style(Style::default().bg(theme::CLR_BG));
    f.render_widget(para, content_area);
}
