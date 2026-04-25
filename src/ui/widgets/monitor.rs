use ratatui::{
    Frame,
    layout::{Constraint, Layout, Rect},
    style::{Modifier, Style},
    text::{Line, Span},
    widgets::{Block, Borders, Paragraph, Row, Table},
};

use crate::ui::state::AppState;
use crate::ui::theme;
use crate::ui::widgets::status::{make_colored_bar, make_sparkline, parse_disk_pct, parse_mem_pct};

/// 监控 Tab —— 顶部 CPU/MEM/DISK/服务概览 + 下方进程表
pub fn render(f: &mut Frame, area: Rect, state: &AppState) {
    let outer = Block::default()
        .style(Style::default().bg(theme::CLR_BG))
        .borders(Borders::empty());
    let inner = outer.inner(area);
    f.render_widget(outer, area);

    // 终端过窄/过矮：合并成一行汇总，下面是进程表
    let compact = inner.height < 18 || inner.width < 70;

    let header_height: u16 = if compact { 3 } else { 9 };
    let [header_area, table_area] = Layout::vertical([
        Constraint::Length(header_height),
        Constraint::Fill(1),
    ])
    .areas(inner);

    // ── 顶部概览 ────────────────────────────────────────────────────────────
    if compact {
        render_compact_header(f, header_area, state);
    } else {
        render_rich_header(f, header_area, state);
    }

    // ── 进程表 ──────────────────────────────────────────────────────────────
    render_process_table(f, table_area, state, compact);
}

/// 紧凑模式：单行汇总 CPU%·MEM%·DSK%
fn render_compact_header(f: &mut Frame, area: Rect, state: &AppState) {
    let cpu_avg = if state.cpu_history.is_empty() {
        0.0
    } else {
        state.cpu_history.iter().sum::<f32>() / state.cpu_history.len() as f32
    };
    let (mem_pct, _) = state
        .system_ctx
        .as_ref()
        .map(|c| parse_mem_pct(&c.memory_info))
        .unwrap_or((0.0, String::new()));
    let (disk_pct, _) = state
        .system_ctx
        .as_ref()
        .map(|c| parse_disk_pct(&c.disk_info))
        .unwrap_or((0.0, String::new()));

    let header_line = Line::from(vec![
        Span::styled(
            "  进程监控  ",
            Style::default().fg(theme::CLR_CYAN).add_modifier(Modifier::BOLD),
        ),
        Span::styled("CPU ", theme::style_dim()),
        Span::styled(format!("{:.0}%", cpu_avg), bar_pct_style(cpu_avg as f64 / 100.0)),
        Span::styled("  ·  MEM ", theme::style_dim()),
        Span::styled(format!("{:.0}%", mem_pct * 100.0), bar_pct_style(mem_pct)),
        Span::styled("  ·  DSK ", theme::style_dim()),
        Span::styled(format!("{:.0}%", disk_pct * 100.0), bar_pct_style(disk_pct)),
    ]);
    f.render_widget(
        Paragraph::new(header_line).style(Style::default().bg(theme::CLR_BG)),
        Rect { x: area.x, y: area.y, width: area.width, height: 1 },
    );
}

/// 富模式：CPU sparkline + MEM 条 + 服务点
fn render_rich_header(f: &mut Frame, area: Rect, state: &AppState) {
    let mut lines: Vec<Line> = Vec::new();

    // 标题
    lines.push(Line::from(Span::styled(
        "  进程监控",
        Style::default().fg(theme::CLR_CYAN).add_modifier(Modifier::BOLD),
    )));
    lines.push(Line::from(""));

    // CPU sparkline
    if state.cpu_history.len() >= 2 {
        let spark = make_sparkline(&state.cpu_history);
        let avg = state.cpu_history.iter().sum::<f32>() / state.cpu_history.len() as f32;
        lines.push(Line::from(vec![
            Span::styled("  CPU  ", Style::default().fg(theme::CLR_DIM).add_modifier(Modifier::BOLD)),
            Span::styled(spark, Style::default().fg(theme::CLR_CYAN)),
            Span::styled(
                format!("   avg {:.0}% · peak {:.0}%", avg, state.cpu_peak_pct),
                theme::style_dim(),
            ),
        ]));
    } else {
        lines.push(Line::from(vec![
            Span::styled("  CPU  ", Style::default().fg(theme::CLR_DIM).add_modifier(Modifier::BOLD)),
            Span::styled("采集数据中…", theme::style_dim()),
        ]));
    }

    // MEM 条
    if let Some(ctx) = state.system_ctx.as_ref() {
        let (mem_pct, mem_label) = parse_mem_pct(&ctx.memory_info);
        let bar_width = (area.width as usize).saturating_sub(28).clamp(10, 30);
        let mem_bar = make_colored_bar(mem_pct, bar_width, 0);
        let mut spans = vec![
            Span::styled("  MEM  ", Style::default().fg(theme::CLR_DIM).add_modifier(Modifier::BOLD)),
        ];
        // 把 make_colored_bar 返回的 Line 中的所有 Span（已带前置 "  "）追加进来；
        // 我们先去掉它前置的 "  " 缩进以避免双重缩进。
        let mut bar_spans: Vec<Span> = mem_bar.spans.into_iter().collect();
        if let Some(first) = bar_spans.first_mut() {
            if first.content.starts_with("  ") {
                first.content = first.content[2..].to_string().into();
            }
        }
        spans.extend(bar_spans);
        spans.push(Span::styled(format!("  {}", mem_label), theme::style_dim()));
        lines.push(Line::from(spans));
    } else {
        lines.push(Line::from(vec![
            Span::styled("  MEM  ", Style::default().fg(theme::CLR_DIM).add_modifier(Modifier::BOLD)),
            Span::styled("尚未扫描", theme::style_dim()),
        ]));
    }

    // 服务点
    let services: Vec<(String, bool)> = if !state.service_status.is_empty() {
        state.service_status.iter()
            .take(6)
            .map(|s| (s.name.clone(), s.cpu_pct > 0.0 || s.mem_mb > 0.0))
            .collect()
    } else if let Some(ctx) = state.system_ctx.as_ref() {
        ctx.running_services.iter()
            .take(6)
            .map(|s| (s.clone(), true))
            .collect()
    } else {
        Vec::new()
    };

    if services.is_empty() {
        lines.push(Line::from(vec![
            Span::styled("  服务  ", Style::default().fg(theme::CLR_DIM).add_modifier(Modifier::BOLD)),
            Span::styled("（采集中…）", theme::style_dim()),
        ]));
    } else {
        let mut spans = vec![
            Span::styled("  服务  ", Style::default().fg(theme::CLR_DIM).add_modifier(Modifier::BOLD)),
        ];
        for (i, (name, active)) in services.iter().enumerate() {
            if i > 0 {
                spans.push(Span::raw("  "));
            }
            spans.push(Span::styled(
                "● ",
                Style::default().fg(if *active { theme::CLR_GREEN } else { theme::CLR_DIM }),
            ));
            spans.push(Span::styled(
                name.clone(),
                Style::default().fg(if *active { theme::CLR_FG } else { theme::CLR_FG_MUTED }),
            ));
        }
        lines.push(Line::from(spans));
    }

    // 分隔线
    lines.push(Line::from(""));
    let sep_w = (area.width as usize).saturating_sub(4);
    lines.push(Line::from(Span::styled(
        format!("  {}", "─".repeat(sep_w)),
        Style::default().fg(theme::CLR_BORDER),
    )));

    let para = Paragraph::new(lines).style(Style::default().bg(theme::CLR_BG));
    f.render_widget(para, area);
}

fn render_process_table(f: &mut Frame, area: Rect, state: &AppState, compact: bool) {
    // 表头标签
    let dim = Style::default().fg(theme::CLR_DIM);
    let header_line = Paragraph::new(Line::from(vec![
        Span::styled("  PID    ", dim),
        Span::styled("NAME                    ", dim),
        Span::styled("CPU%  ", dim),
        Span::styled("MEM   ", dim),
    ]))
    .style(Style::default().bg(theme::CLR_BG));

    if area.height < 2 {
        return;
    }

    let header_area = Rect { x: area.x, y: area.y, width: area.width, height: 1 };
    f.render_widget(header_line, header_area);

    let table_area = Rect {
        x: area.x,
        y: area.y + 1,
        width: area.width,
        height: area.height - 1,
    };

    if state.process_list.is_empty() {
        let empty = Paragraph::new(Line::from(Span::styled(
            "  正在获取进程列表…",
            Style::default().fg(theme::CLR_FG_MUTED),
        )));
        f.render_widget(empty, table_area);
        return;
    }

    let name_w = if compact { 18 } else { 26 };
    let rows: Vec<Row> = state
        .process_list
        .iter()
        .map(|p| {
            let cpu_color = if p.cpu_pct > 10.0 {
                theme::CLR_AMBER
            } else {
                theme::CLR_FG_MUTED
            };
            let name: String = p.name.chars().take(name_w).collect();
            Row::new(vec![
                Span::styled(format!("  {}", p.pid), Style::default().fg(theme::CLR_DIM)),
                Span::styled(name, Style::default().fg(theme::CLR_FG)),
                Span::styled(format!("{:.1}", p.cpu_pct), Style::default().fg(cpu_color)),
                Span::styled(format!("{:.0}M", p.mem_mb), Style::default().fg(theme::CLR_FG_MUTED)),
            ])
        })
        .collect();

    let widths = [
        Constraint::Length(8),
        Constraint::Length((name_w + 2) as u16),
        Constraint::Length(7),
        Constraint::Length(8),
    ];

    let table = Table::new(rows, widths)
        .style(Style::default().bg(theme::CLR_BG))
        .column_spacing(1);

    f.render_widget(table, table_area);
}

fn bar_pct_style(pct: f64) -> Style {
    let color = if pct > 0.9 {
        theme::CLR_RED
    } else if pct > 0.7 {
        theme::CLR_AMBER
    } else if pct > 0.5 {
        theme::CLR_PROGRESS_MID
    } else {
        theme::CLR_GREEN
    };
    Style::default().fg(color).add_modifier(Modifier::BOLD)
}
