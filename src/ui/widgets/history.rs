use ratatui::{
    Frame,
    layout::Rect,
    style::{Modifier, Style},
    text::{Line, Span},
    widgets::{Block, Borders, Paragraph},
};

use crate::ui::state::AppState;
use crate::ui::theme;

/// 历史 Tab —— 富信息行：HH:MM │ tool(args) │ ✓ Xms │ ↺
pub fn render(f: &mut Frame, area: Rect, state: &AppState) {
    let inner = Block::default()
        .style(Style::default().bg(theme::CLR_BG))
        .borders(Borders::empty());
    let inner_area = inner.inner(area);
    f.render_widget(inner, area);

    if inner_area.height < 2 {
        return;
    }

    // ── 标题行 ──
    let header = Paragraph::new(Line::from(Span::styled(
        "  操作历史",
        Style::default().fg(theme::CLR_CYAN).add_modifier(Modifier::BOLD),
    )))
    .style(Style::default().bg(theme::CLR_BG));
    f.render_widget(
        header,
        Rect { x: inner_area.x, y: inner_area.y, width: inner_area.width, height: 1 },
    );

    // 副标题：列说明
    let subtitle = Paragraph::new(Line::from(vec![
        Span::styled("  时间   命令", theme::style_dim()),
    ]))
    .style(Style::default().bg(theme::CLR_BG));
    f.render_widget(
        subtitle,
        Rect { x: inner_area.x, y: inner_area.y + 1, width: inner_area.width, height: 1 },
    );

    let content_area = Rect {
        x: inner_area.x,
        y: inner_area.y + 3,
        width: inner_area.width,
        height: inner_area.height.saturating_sub(3),
    };

    if state.ops_history.is_empty() {
        let empty = Paragraph::new(vec![
            Line::from(""),
            Line::from(Span::styled(
                "  暂无操作记录 — 在对话区试试「查看磁盘使用情况」",
                Style::default().fg(theme::CLR_FG_MUTED),
            )),
        ])
        .style(Style::default().bg(theme::CLR_BG));
        f.render_widget(empty, content_area);
        return;
    }

    // 窄屏裁剪 args
    let args_max: usize = if content_area.width < 70 { 28 } else { 60 };
    let max_rows = content_area.height as usize;

    let dim = Style::default().fg(theme::CLR_DIM);
    let sep = Span::styled(" │ ", dim);

    let lines: Vec<Line> = state
        .ops_history
        .iter()
        .rev()
        .take(max_rows)
        .map(|rec| {
            let time_str = rec.timestamp.with_timezone(&chrono::Local).format("%H:%M").to_string();

            // 工具名 + (args)
            let mut row: Vec<Span> = vec![
                Span::styled(format!("  {}", time_str), Style::default().fg(theme::CLR_DIM)),
                sep.clone(),
                Span::styled(
                    rec.tool.clone(),
                    Style::default().fg(if rec.success { theme::CLR_FG } else { theme::CLR_AMBER }),
                ),
            ];
            if !rec.args_summary.is_empty() {
                let truncated: String = if rec.args_summary.chars().count() > args_max {
                    let head: String = rec.args_summary.chars().take(args_max).collect();
                    format!("({}…)", head)
                } else {
                    format!("({})", rec.args_summary)
                };
                row.push(Span::styled(format!(" {}", truncated), Style::default().fg(theme::CLR_FG_MUTED)));
            }

            row.push(sep.clone());

            // 状态 + 耗时
            if rec.success {
                row.push(Span::styled(
                    format!("✓ {}ms", rec.duration_ms),
                    Style::default().fg(theme::CLR_GREEN),
                ));
            } else {
                row.push(Span::styled(
                    format!("⚠ {}ms", rec.duration_ms),
                    Style::default().fg(theme::CLR_AMBER).add_modifier(Modifier::BOLD),
                ));
            }

            // 撤销标记
            if rec.undoable {
                row.push(Span::raw("  "));
                row.push(Span::styled("↺", Style::default().fg(theme::CLR_AMBER).add_modifier(Modifier::BOLD)));
            }

            Line::from(row)
        })
        .collect();

    let para = Paragraph::new(lines).style(Style::default().bg(theme::CLR_BG));
    f.render_widget(para, content_area);
}
