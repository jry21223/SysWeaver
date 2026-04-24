use ratatui::{
    Frame,
    layout::{Constraint, Layout, Rect},
    style::{Modifier, Style},
    text::{Line, Span},
    widgets::{Block, Borders, Paragraph, Row, Table},
};

use crate::ui::state::AppState;
use crate::ui::theme;

/// 进程监控面板（匹配 JSX 设计稿 MonitorPanel）
/// 数据由 AppState::tick_process_list() 每 ~2 秒在后台刷新，不阻塞渲染线程
pub fn render(f: &mut Frame, area: Rect, state: &AppState) {
    let inner = Block::default()
        .style(Style::default().bg(theme::CLR_BG))
        .borders(Borders::empty());
    let inner_area = inner.inner(area);
    f.render_widget(inner, area);

    let [header_area, table_area] = Layout::vertical([
        Constraint::Length(3),
        Constraint::Fill(1),
    ])
    .areas(inner_area);

    let header = Paragraph::new(Line::from(vec![
        Span::styled(
            "  进程监控",
            Style::default()
                .fg(theme::CLR_CYAN)
                .add_modifier(Modifier::BOLD),
        ),
    ]))
    .style(Style::default().bg(theme::CLR_BG));
    f.render_widget(header, Rect {
        x: header_area.x,
        y: header_area.y,
        width: header_area.width,
        height: 1,
    });

    let dim = Style::default().fg(theme::CLR_DIM);
    let col_labels = Paragraph::new(Line::from(vec![
        Span::styled("  PID     ", dim),
        Span::styled("NAME                    ", dim),
        Span::styled("CPU%    ", dim),
        Span::styled("MEM    ", dim),
    ]));
    f.render_widget(col_labels, Rect {
        x: header_area.x,
        y: header_area.y + 2,
        width: header_area.width,
        height: 1,
    });

    if state.process_list.is_empty() {
        let empty = Paragraph::new(Line::from(Span::styled(
            "  正在获取进程列表…",
            Style::default().fg(theme::CLR_FG_MUTED),
        )));
        f.render_widget(empty, table_area);
        return;
    }

    let rows: Vec<Row> = state
        .process_list
        .iter()
        .map(|p| {
            let cpu_color = if p.cpu_pct > 10.0 {
                theme::CLR_AMBER
            } else {
                theme::CLR_FG_MUTED
            };
            let name: String = p.name.chars().take(24).collect();
            Row::new(vec![
                Span::styled(format!("  {}", p.pid), Style::default().fg(theme::CLR_DIM)),
                Span::styled(name, Style::default().fg(theme::CLR_FG)),
                Span::styled(format!("{:.1}  ", p.cpu_pct), Style::default().fg(cpu_color)),
                Span::styled(format!("{:.0}M  ", p.mem_mb), Style::default().fg(theme::CLR_FG_MUTED)),
            ])
        })
        .collect();

    let widths = [
        Constraint::Length(8),
        Constraint::Length(26),
        Constraint::Length(8),
        Constraint::Length(8),
    ];

    let table = Table::new(rows, widths)
        .style(Style::default().bg(theme::CLR_BG))
        .column_spacing(0);

    f.render_widget(table, table_area);
}
