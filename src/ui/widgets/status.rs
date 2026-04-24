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
    let inner = area;

    // ── 折叠态：窄竖条 ──
    if state.side_collapsed {
        let mut lines = vec![
            Line::from(""),
            Line::from(Span::styled(
                "◁",
                Style::default().fg(theme::CLR_FG_MUTED),
            )),
            Line::from(""),
            Line::from(Span::styled(
                "系",
                Style::default().fg(theme::CLR_CYAN),
            )),
            Line::from(Span::styled(
                "统",
                Style::default().fg(theme::CLR_CYAN),
            )),
        ];
        // 截断以适应高度
        lines.truncate(inner.height as usize);
        let collapsed_para = Paragraph::new(lines)
            .style(Style::default().bg(theme::CLR_BG_PANEL));
        f.render_widget(collapsed_para, inner);
        return;
    }

    // ── 展开态顶部：SYSTEM 标题 + ▷ 折叠按钮 ──
    let header_line = Line::from(vec![
        Span::styled(
            " SYSTEM ",
            Style::default()
                .fg(theme::CLR_DIM)
                .add_modifier(Modifier::BOLD),
        ),
        Span::raw(" ".repeat(inner.width.saturating_sub(9) as usize)),
        Span::styled("▷", Style::default().fg(theme::CLR_FG_MUTED)),
    ]);
    f.render_widget(
        Paragraph::new(header_line).style(Style::default().bg(theme::CLR_BG_PANEL)),
        Rect { x: inner.x, y: inner.y, width: inner.width, height: 1 },
    );

    // 主体内容区域（跳过标题行）
    let body = Rect {
        x: inner.x,
        y: inner.y + 1,
        width: inner.width,
        height: inner.height.saturating_sub(1),
    };

    // 无系统信息时显示 loading
    let Some(ctx) = &state.system_ctx else {
        let loading = Paragraph::new(vec![
            Line::from(""),
            Line::from(Span::styled(
                "  SYSTEM",
                Style::default().fg(theme::CLR_CYAN).add_modifier(Modifier::BOLD),
            )),
            Line::from(Span::styled("  正在扫描…", theme::style_dim())),
        ]);
        f.render_widget(loading, body);
        return;
    };

    // 解析 memory_info 和 disk_info
    let (mem_used_pct, mem_label) = parse_mem_pct(&ctx.memory_info);
    let (disk_used_pct, disk_label) = parse_disk_pct(&ctx.disk_info);
    let (cpu_cores, cpu_model) = parse_cpu_info(&ctx.cpu_info);

    // ── 构建面板内容（匹配 JSX SystemPanel）──────────────────────────────
    let mut lines: Vec<Line> = Vec::new();

    // 标题：系统状态（青色粗体）
    lines.push(Line::from(""));
    lines.push(Line::from(Span::styled(
        "  系统状态",
        Style::default().fg(theme::CLR_CYAN).add_modifier(Modifier::BOLD),
    )));

    // SSH 远程标注
    if let Some(ref label) = state.remote_label {
        lines.push(Line::from(Span::styled(
            format!("  {}", label),
            Style::default().fg(Color::Rgb(80, 180, 255)),
        )));
        lines.push(Line::from(""));
    }

    // HOSTNAME 区
    lines.push(Line::from(""));
    lines.push(Line::from(Span::styled("  HOSTNAME", Style::default().fg(theme::CLR_DIM).add_modifier(Modifier::BOLD))));
    lines.push(Line::from(Span::styled(
        format!("  {}", ctx.hostname),
        Style::default().fg(theme::CLR_FG),
    )));
    // OS + CPU 概要
    let os_short: String = ctx.os_info.chars().take(20).collect();
    let cpu_short: String = cpu_model.chars().take(16).collect();
    lines.push(Line::from(Span::styled(
        format!("  {} · {}", os_short, if cpu_cores > 0 { format!("{}c", cpu_cores) } else { cpu_short }),
        theme::style_dim(),
    )));

    // CPU 区 — 基于实际采样的 sparkline
    lines.push(Line::from(""));
    lines.push(Line::from(Span::styled("  CPU", Style::default().fg(theme::CLR_DIM).add_modifier(Modifier::BOLD))));

    if state.cpu_history.len() >= 2 {
        let spark = make_sparkline(&state.cpu_history);
        lines.push(Line::from(Span::styled(
            format!("  {}", spark),
            Style::default().fg(theme::CLR_CYAN),
        )));
        let avg = state.cpu_history.iter().sum::<f32>() / state.cpu_history.len() as f32;
        lines.push(Line::from(Span::styled(
            format!("  avg {:.0}% · peak {:.0}%", avg, state.cpu_peak_pct),
            theme::style_dim(),
        )));
    } else {
        lines.push(Line::from(Span::styled("  采集数据中…", theme::style_dim())));
    }

    // MEM 区（HTML 同款变色进度条）
    lines.push(Line::from(""));
    lines.push(Line::from(vec![
        Span::styled("  MEM", Style::default().fg(theme::CLR_DIM).add_modifier(Modifier::BOLD)),
        Span::styled(format!("    {}", mem_label), Style::default().fg(theme::CLR_FG)),
    ]));
    lines.push(make_colored_bar(mem_used_pct, 20));

    // DISK 区
    lines.push(Line::from(""));
    lines.push(Line::from(vec![
        Span::styled("  DISK", Style::default().fg(theme::CLR_DIM).add_modifier(Modifier::BOLD)),
        Span::styled(format!("    {}", disk_label), Style::default().fg(theme::CLR_FG)),
    ]));
    lines.push(make_colored_bar(disk_used_pct, 20));

    // 服务区（分隔线）
    lines.push(Line::from(""));
    let sep_width = body.width.saturating_sub(2) as usize;
    lines.push(Line::from(Span::styled(
        "─".repeat(sep_width),
        Style::default().fg(theme::CLR_BORDER),
    )));

    lines.push(Line::from(Span::styled("  服务", Style::default().fg(theme::CLR_CYAN).add_modifier(Modifier::BOLD))));

    // 服务列表（绿点活跃，灰点不活跃）
    let svc_max_width = body.width.saturating_sub(8) as usize;
    for svc in ctx.running_services.iter().take(4) {
        let is_active = true; // 简化：都视为活跃
        lines.push(Line::from(vec![
            Span::styled("  ● ", Style::default().fg(if is_active { theme::CLR_GREEN } else { theme::CLR_DIM })),
            Span::styled(
                svc.chars().take(svc_max_width).collect::<String>(),
                Style::default().fg(if is_active { theme::CLR_FG } else { theme::CLR_FG_MUTED }),
            ),
        ]));
    }

    // 网络区
    lines.push(Line::from(""));
    lines.push(Line::from(Span::styled(
        "─".repeat(sep_width),
        Style::default().fg(theme::CLR_BORDER),
    )));

    lines.push(Line::from(Span::styled("  网络", Style::default().fg(theme::CLR_CYAN).add_modifier(Modifier::BOLD))));
    // 解析 network_info（格式："IP x.x.x.x / GW x.x.x.x"）
    lines.push(Line::from(Span::styled(
        format!("  {}", ctx.network_info.chars().take(30).collect::<String>()),
        theme::style_dim(),
    )));

    // ── 渲染 ───────────────────────────────────────────────────────────────
    let para = Paragraph::new(lines).style(Style::default().bg(theme::CLR_BG_PANEL));
    f.render_widget(para, body);
}

/// 将 CPU 采样历史转为 sparkline 字符串（▁▂▃▄▅▆▇█）
fn make_sparkline(samples: &[f32]) -> String {
    const CHARS: &[char] = &['▁', '▂', '▃', '▄', '▅', '▆', '▇', '█'];
    let max = samples.iter().cloned().fold(0.0f32, f32::max).max(1.0);
    samples
        .iter()
        .map(|&v| {
            let idx = ((v / max) * (CHARS.len() - 1) as f32).round() as usize;
            CHARS[idx.min(CHARS.len() - 1)]
        })
        .collect()
}

/// HTML 同款变色进度条 — 按使用率阈值分段着色
///  >90% 红色 · >70% 琥珀 · >50% 黄绿 · 其余绿色
fn make_colored_bar(pct: f64, width: usize) -> Line<'static> {
    let filled = (pct * width as f64).round() as usize;
    let empty = width - filled;

    let bar_color = if pct > 0.9 {
        theme::CLR_RED
    } else if pct > 0.7 {
        theme::CLR_AMBER
    } else if pct > 0.5 {
        Color::Rgb(190, 210, 80) // 黄绿，对应 JSX oklch(0.75 0.16 110)
    } else {
        theme::CLR_GREEN
    };

    Line::from(vec![
        Span::raw("  "),
        Span::styled("█".repeat(filled), Style::default().fg(bar_color)),
        Span::styled("░".repeat(empty), Style::default().fg(theme::CLR_DIM)),
        Span::styled(
            format!(" {:.0}%", pct * 100.0),
            Style::default().fg(bar_color),
        ),
    ])
}

/// 解析 "16.0G total, 4.2G used" → (0.26, "4.2G/16.0G")
fn parse_mem_pct(info: &str) -> (f64, String) {
    let parts: Vec<&str> = info.split(',').collect();
    if parts.len() >= 2 {
        let total_str = parts[0].trim().split_whitespace().next().unwrap_or("0");
        let used_str  = parts[1].trim().split_whitespace().next().unwrap_or("0");

        let total = parse_size_gb(total_str);
        let used  = parse_size_gb(used_str);

        if total > 0.0 {
            let pct = (used / total).min(1.0);
            return (pct, format!("{}/{}G", used_str, total_str));
        }
    }
    (0.0, info.chars().take(20).collect())
}

/// 解析 "494G total, 20.4G free, 39% used" → (0.39, "20.4G free")
fn parse_disk_pct(info: &str) -> (f64, String) {
    let parts: Vec<&str> = info.split(',').collect();
    if parts.len() >= 3 {
        let total_str = parts[0].trim().split_whitespace().next().unwrap_or("0");
        let free_str  = parts[1].trim().split_whitespace().next().unwrap_or("0");
        let pct_str   = parts[2].trim().split_whitespace().next().unwrap_or("0").trim_end_matches('%');
        if let Ok(pct) = pct_str.parse::<f64>() {
            return (pct / 100.0, format!("{} free/{}", free_str, total_str));
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

/// 解析 cpu_info
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

    // nproc && model 合在一行时
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

/// 将 "4.2G" / "512M" 转为 GB
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