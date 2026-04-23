use ratatui::{
    Frame,
    layout::{Constraint, Layout, Rect},
    style::{Color, Modifier, Style},
    text::{Line, Span},
    widgets::{Block, BorderType, Borders, Gauge, List, ListItem, Paragraph},
};

use crate::ui::state::AppState;
use crate::ui::theme;

pub fn render(f: &mut Frame, area: Rect, state: &AppState) {
    let block = Block::default()
        .title(Span::styled(" 📊 系统状态 ", theme::style_border()))
        .borders(Borders::ALL)
        .border_type(BorderType::Rounded)
        .border_style(theme::style_border())
        .style(Style::default().bg(theme::CLR_BG_PANEL));

    let inner = block.inner(area);
    f.render_widget(block, area);

    // 无系统信息时显示 loading
    let Some(ctx) = &state.system_ctx else {
        let loading = Paragraph::new(Span::styled(
            "\n  正在扫描系统…",
            Style::default().fg(Color::Gray).add_modifier(Modifier::ITALIC),
        ));
        f.render_widget(loading, inner);
        return;
    };

    // ── 垂直切分：系统信息区 / 操作历史区 ────────────────────────────────
    let history_height = (state.ops_history.len() as u16 + 3).min(inner.height / 2);

    let [sys_area, hist_area] = Layout::vertical([
        Constraint::Fill(1),
        Constraint::Length(history_height),
    ])
    .areas(inner);

    render_sys_info(f, sys_area, ctx, state);
    render_ops_history(f, hist_area, state);
}

fn render_sys_info(
    f: &mut Frame,
    area: Rect,
    ctx: &crate::agent::memory::SystemContext,
    _state: &AppState,
) {
    // 解析 memory_info：格式 "16.0G total, 4.2G used"
    let (mem_used_pct, mem_label) = parse_mem_pct(&ctx.memory_info);
    // 解析 disk_info：格式 "/dev/sda1 80G, 45% used"
    let (disk_used_pct, disk_label) = parse_disk_pct(&ctx.disk_info);
    // 解析 cpu_info：格式 "8\nIntel Core i7..." 或 "8Intel..."
    let (cpu_cores, cpu_model) = parse_cpu_info(&ctx.cpu_info);

    let rows: &[(&str, f64, String)] = &[
        ("MEM ", mem_used_pct, mem_label),
        ("DISK", disk_used_pct, disk_label),
    ];

    // 上方：hostname + OS
    let info_height = 4u16;
    let gauge_height = (rows.len() as u16) * 2 + 1;

    if area.height < info_height + gauge_height {
        // 空间不足，只显示文字
        let text = Paragraph::new(vec![
            Line::from(Span::styled(
                format!(" {} @ {}", ctx.os_info.chars().take(20).collect::<String>(), ctx.hostname),
                Style::default().fg(Color::Rgb(180, 180, 220)),
            )),
            Line::from(Span::styled(format!(" MEM: {}", ctx.memory_info), theme::style_dim())),
            Line::from(Span::styled(format!(" DSK: {}", ctx.disk_info), theme::style_dim())),
        ]);
        f.render_widget(text, area);
        return;
    }

    let [header_area, gauges_area, services_area] = Layout::vertical([
        Constraint::Length(info_height),
        Constraint::Length(gauge_height),
        Constraint::Fill(1),
    ])
    .areas(area);

    // ── 主机信息 ─────────────────────────────────────────────────────────
    let os_short: String = ctx.os_info.chars().take(26).collect();
    let cpu_line = if cpu_cores > 0 {
        format!("  CPU ×{}  {}", cpu_cores, cpu_model.chars().take(18).collect::<String>())
    } else {
        format!("  CPU  {}", cpu_model.chars().take(22).collect::<String>())
    };
    let header = Paragraph::new(vec![
        Line::from(""),
        Line::from(Span::styled(
            format!("  {}", ctx.hostname),
            Style::default().fg(Color::White).add_modifier(Modifier::BOLD),
        )),
        Line::from(Span::styled(
            format!("  {}", os_short),
            Style::default().fg(Color::Rgb(160, 160, 200)),
        )),
        Line::from(Span::styled(cpu_line, theme::style_dim())),
    ]);
    f.render_widget(header, header_area);

    // ── 进度条 ────────────────────────────────────────────────────────────
    let gauge_rows: Vec<Constraint> = rows.iter().flat_map(|_| {
        vec![Constraint::Length(1), Constraint::Length(1)]
    }).chain(std::iter::once(Constraint::Length(1))).collect();

    let gauge_areas = Layout::vertical(gauge_rows).split(gauges_area);

    for (i, (label, pct, text)) in rows.iter().enumerate() {
        let label_area = gauge_areas[i * 2];
        let bar_area   = gauge_areas[i * 2 + 1];

        let color = theme::gauge_color_by_pct(*pct);
        let pct_u16 = (*pct * 100.0).round() as u16;

        // label + 百分比文字
        f.render_widget(
            Paragraph::new(Line::from(vec![
                Span::styled(format!("  {} ", label), theme::style_dim()),
                Span::styled(text.clone(), Style::default().fg(color)),
            ])),
            label_area,
        );

        // Gauge bar
        let gauge = Gauge::default()
            .gauge_style(
                Style::default()
                    .fg(color)
                    .bg(theme::CLR_GAUGE_BG),
            )
            .percent(pct_u16.min(100))
            .label("");
        f.render_widget(gauge, bar_area);
    }

    // ── 活跃服务列表 ────────────────────────────────────────────────────
    if !ctx.running_services.is_empty() && services_area.height > 2 {
        let svc_block = Block::default()
            .title(Span::styled(" 服务 ", theme::style_dim()))
            .borders(Borders::TOP)
            .border_style(theme::style_border());

        let svc_inner = svc_block.inner(services_area);
        f.render_widget(svc_block, services_area);

        let items: Vec<ListItem> = ctx
            .running_services
            .iter()
            .take(svc_inner.height as usize)
            .map(|s| {
                ListItem::new(Line::from(vec![
                    Span::styled("  ● ", theme::style_success()),
                    Span::styled(
                        s.chars().take(svc_inner.width.saturating_sub(4) as usize).collect::<String>(),
                        Style::default().fg(Color::Rgb(180, 200, 180)),
                    ),
                ]))
            })
            .collect();

        f.render_widget(List::new(items), svc_inner);
    }
}

fn render_ops_history(f: &mut Frame, area: Rect, state: &AppState) {
    if area.height < 3 {
        return;
    }

    let block = Block::default()
        .title(Span::styled(" 📋 最近操作 ", theme::style_dim()))
        .borders(Borders::TOP)
        .border_style(theme::style_border());

    let inner = block.inner(area);
    f.render_widget(block, area);

    if state.ops_history.is_empty() {
        f.render_widget(
            Paragraph::new(Span::styled("  (暂无操作记录)", theme::style_dim())),
            inner,
        );
        return;
    }

    let items: Vec<ListItem> = state
        .ops_history
        .iter()
        .rev()
        .take(inner.height as usize)
        .map(|op| {
            let icon = if op.success { "✅" } else { "❌" };
            let tool_short: String = op.tool.chars().take(inner.width.saturating_sub(6) as usize).collect();
            ListItem::new(Line::from(vec![
                Span::raw(format!("  {} ", icon)),
                Span::styled(tool_short, theme::style_dim()),
            ]))
        })
        .collect();

    f.render_widget(List::new(items), inner);
}

/// 解析 "16.0G total, 4.2G used" → (0.26, "4.2G/16.0G")
fn parse_mem_pct(info: &str) -> (f64, String) {
    // 尝试从 "Xg total, Yg used" 解析
    let parts: Vec<&str> = info.split(',').collect();
    if parts.len() >= 2 {
        let total_str = parts[0].trim().split_whitespace().next().unwrap_or("0");
        let used_str  = parts[1].trim().split_whitespace().next().unwrap_or("0");

        let total = parse_size_gb(total_str);
        let used  = parse_size_gb(used_str);

        if total > 0.0 {
            let pct = (used / total).min(1.0);
            return (pct, format!("{}/{} ({:.0}%)", used_str, total_str, pct * 100.0));
        }
    }
    (0.0, info.chars().take(20).collect())
}

/// 解析 "494G total, 20.4G free, 39% used" → (0.39, "20.4G free/494G (39%)")
fn parse_disk_pct(info: &str) -> (f64, String) {
    // 格式："{total} total, {free} free, {pct}% used"
    let parts: Vec<&str> = info.split(',').collect();
    if parts.len() >= 3 {
        let total_str = parts[0].trim().split_whitespace().next().unwrap_or("0");
        let free_str  = parts[1].trim().split_whitespace().next().unwrap_or("0");
        let pct_str   = parts[2].trim().split_whitespace().next().unwrap_or("0").trim_end_matches('%');
        if let Ok(pct) = pct_str.parse::<f64>() {
            return (pct / 100.0, format!("{}free/{} ({:.0}%)", free_str, total_str, pct));
        }
    }
    // 回退：找第一个 "XX%" 模式
    for part in info.split_whitespace() {
        let trimmed = part.trim_end_matches('%');
        if let Ok(n) = trimmed.parse::<f64>() {
            if n >= 0.0 && n <= 100.0 {
                return (n / 100.0, format!("{:.0}%", n));
            }
        }
    }
    (0.0, info.chars().take(20).collect())
}

/// 解析 cpu_info：`nproc` 输出 + CPU model name，返回 (核心数, 型号简称)
fn parse_cpu_info(info: &str) -> (usize, String) {
    let mut cores: usize = 0;
    let mut model = String::new();

    for line in info.lines() {
        let trimmed = line.trim();
        if trimmed.is_empty() { continue; }
        if cores == 0 {
            if let Ok(n) = trimmed.parse::<usize>() {
                cores = n;
                continue;
            }
        }
        if model.is_empty() {
            model = trimmed.to_string();
        }
    }

    // nproc && model 合在一行时（如 "8Intel Core..."）
    if cores == 0 && !info.is_empty() {
        let digits: String = info.chars().take_while(|c| c.is_ascii_digit()).collect();
        if !digits.is_empty() {
            cores = digits.parse().unwrap_or(0);
            model = info[digits.len()..].trim().to_string();
        }
    }

    if model.is_empty() {
        model = info.chars().take(20).collect();
    }

    (cores, model)
}

/// 将 "4.2G" / "512M" / "1024K" 转为 GB
fn parse_size_gb(s: &str) -> f64 {
    if s.is_empty() {
        return 0.0;
    }
    let (num_part, unit) = if s.ends_with(|c: char| c.is_alphabetic()) {
        let n = &s[..s.len() - 1];
        let u = &s[s.len() - 1..];
        (n, u)
    } else {
        (s, "G")
    };

    let n: f64 = num_part.parse().unwrap_or(0.0);
    match unit.to_uppercase().as_str() {
        "T" => n * 1024.0,
        "G" => n,
        "M" => n / 1024.0,
        "K" => n / 1024.0 / 1024.0,
        _   => n,
    }
}
