use ratatui::{
    Frame,
    layout::Rect,
    style::{Modifier, Style},
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
                Style::default().fg(theme::CLR_AMBER),
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
        lines.truncate(inner.height as usize);
        f.render_widget(
            Paragraph::new(lines).style(Style::default().bg(theme::CLR_BG_PANEL)),
            inner,
        );
        return;
    }

    // ── 展开态顶部：SYSTEM 标题 + ▷ 折叠按钮 ──
    let button_pad = inner.width.saturating_sub(9) as usize;
    let header_line = Line::from(vec![
        Span::styled(
            " SYSTEM ",
            Style::default()
                .fg(theme::CLR_DIM)
                .add_modifier(Modifier::BOLD),
        ),
        Span::raw(" ".repeat(button_pad)),
        Span::styled("▷", Style::default().fg(theme::CLR_AMBER)),
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

    let (mem_used_pct, mem_label) = parse_mem_pct(&ctx.memory_info);
    let (disk_used_pct, disk_label) = parse_disk_pct(&ctx.disk_info);
    let (cpu_cores, cpu_model) = parse_cpu_info(&ctx.cpu_info);

    // 紧凑模式：高度不足时省略空行
    let compact = body.height < 28;
    let sep_width = body.width.saturating_sub(2) as usize;

    let mut lines: Vec<Line> = Vec::new();

    // ── 系统状态标题 ──
    if !compact {
        lines.push(Line::from(""));
    }
    lines.push(Line::from(Span::styled(
        "  系统状态",
        Style::default().fg(theme::CLR_CYAN).add_modifier(Modifier::BOLD),
    )));

    if let Some(ref label) = state.remote_label {
        lines.push(Line::from(Span::styled(
            format!("  {}", label),
            Style::default().fg(theme::CLR_SSH),
        )));
    }

    // ── HOSTNAME ──
    if !compact { lines.push(Line::from("")); }
    lines.push(Line::from(Span::styled("  HOSTNAME", Style::default().fg(theme::CLR_DIM).add_modifier(Modifier::BOLD))));
    lines.push(Line::from(Span::styled(
        format!("  {}", ctx.hostname),
        Style::default().fg(theme::CLR_FG),
    )));
    let os_short: String = ctx.os_info.chars().take(20).collect();
    let cpu_short: String = cpu_model.chars().take(16).collect();
    lines.push(Line::from(Span::styled(
        format!("  {} · {}", os_short, if cpu_cores > 0 { format!("{}c", cpu_cores) } else { cpu_short }),
        theme::style_dim(),
    )));

    // ── CPU ──
    if !compact { lines.push(Line::from("")); }
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

    // ── MEM & DISK（紧凑模式合并显示）──
    if compact {
        lines.push(Line::from(vec![
            Span::styled("  MEM ", Style::default().fg(theme::CLR_DIM).add_modifier(Modifier::BOLD)),
            Span::styled(format!("{:.0}%", mem_used_pct * 100.0), bar_color_style(mem_used_pct)),
            Span::styled("  DSK ", Style::default().fg(theme::CLR_DIM).add_modifier(Modifier::BOLD)),
            Span::styled(format!("{:.0}%", disk_used_pct * 100.0), bar_color_style(disk_used_pct)),
        ]));
    } else {
        lines.push(Line::from(""));
        lines.push(Line::from(vec![
            Span::styled("  MEM", Style::default().fg(theme::CLR_DIM).add_modifier(Modifier::BOLD)),
            Span::styled(format!("    {}", mem_label), Style::default().fg(theme::CLR_FG)),
        ]));
        lines.push(make_colored_bar(mem_used_pct, 20, 2));

        lines.push(Line::from(""));
        lines.push(Line::from(vec![
            Span::styled("  DISK", Style::default().fg(theme::CLR_DIM).add_modifier(Modifier::BOLD)),
            Span::styled(format!("    {}", disk_label), Style::default().fg(theme::CLR_FG)),
        ]));
        lines.push(make_colored_bar(disk_used_pct, 20, 2));
    }

    // ── 服务区 ──
    lines.push(Line::from(""));
    lines.push(Line::from(Span::styled(
        "─".repeat(sep_width),
        Style::default().fg(theme::CLR_BORDER),
    )));
    lines.push(Line::from(Span::styled("  服务", Style::default().fg(theme::CLR_CYAN).add_modifier(Modifier::BOLD))));

    let svc_limit = if compact { 4 } else { 6 };
    let name_w = (body.width as usize).saturating_sub(6).max(6);

    if !state.service_status.is_empty() {
        // 设计要求：仅显示 ● + 服务名（无 CPU/MEM 列）
        for svc in state.service_status.iter().take(svc_limit) {
            // 不活跃服务（CPU=0 且 MEM=0）灰色显示
            // spec §6: ● = 活跃(GREEN)，○ = 非活跃(FG_MUTED)
            let active = svc.cpu_pct > 0.0 || svc.mem_mb > 0.0;
            lines.push(Line::from(vec![
                Span::styled(
                    if active { "  ● " } else { "  ○ " },
                    Style::default().fg(if active { theme::CLR_GREEN } else { theme::CLR_FG_MUTED }),
                ),
                Span::styled(
                    svc.name.chars().take(name_w).collect::<String>(),
                    Style::default().fg(if active { theme::CLR_FG } else { theme::CLR_FG_MUTED }),
                ),
            ]));
        }
    } else {
        for svc in ctx.running_services.iter().take(svc_limit) {
            lines.push(Line::from(vec![
                Span::styled("  ● ", Style::default().fg(theme::CLR_GREEN)),
                Span::styled(
                    svc.chars().take(name_w).collect::<String>(),
                    Style::default().fg(theme::CLR_FG),
                ),
            ]));
        }
        if ctx.running_services.is_empty() {
            lines.push(Line::from(Span::styled("  （正在采集…）", theme::style_dim())));
        }
    }

    // ── 网络区（空间充足时才显示）──
    if body.height >= 22 || !compact {
        lines.push(Line::from(""));
        lines.push(Line::from(Span::styled(
            "─".repeat(sep_width),
            Style::default().fg(theme::CLR_BORDER),
        )));
        lines.push(Line::from(Span::styled("  网络", Style::default().fg(theme::CLR_CYAN).add_modifier(Modifier::BOLD))));
        let max_w = body.width.saturating_sub(4) as usize;
        for (label, value) in parse_network_info(&ctx.network_info) {
            if value.is_empty() { continue; }
            let truncated: String = value.chars().take(max_w.saturating_sub(label.chars().count() + 1)).collect();
            lines.push(Line::from(vec![
                Span::styled(format!("  {} ", label), theme::style_dim()),
                Span::styled(truncated, Style::default().fg(theme::CLR_FG_MUTED)),
            ]));
        }
    }

    // ── 高度裁剪（防止溢出）──
    lines.truncate(body.height as usize);

    let para = Paragraph::new(lines).style(Style::default().bg(theme::CLR_BG_PANEL));
    f.render_widget(para, body);
}

/// 将 "IP:1.1.1.1 2.2.2.2 | GW:3.3.3.3 | 监听端口:80,443" 解析为
/// [(IP, "1.1.1.1 2.2.2.2"), (GW, "3.3.3.3"), (监听端口, "80,443")]
fn parse_network_info(info: &str) -> Vec<(&'static str, String)> {
    let mut ip = String::new();
    let mut gw = String::new();
    let mut ports = String::new();

    // 优先按 "|" 分段（新格式）
    let segments: Vec<&str> = if info.contains('|') {
        info.split('|').collect()
    } else {
        info.split_whitespace()
            .fold(Vec::new(), |mut acc: Vec<String>, tok| {
                if tok.contains(':') {
                    acc.push(tok.to_string());
                } else if let Some(last) = acc.last_mut() {
                    last.push(' ');
                    last.push_str(tok);
                }
                acc
            })
            .iter()
            .map(|s| Box::leak(s.clone().into_boxed_str()) as &str)
            .collect()
    };

    for seg in segments {
        let s = seg.trim();
        if let Some(rest) = s.strip_prefix("IP:").or_else(|| s.strip_prefix("IP ")) {
            ip = rest.trim().to_string();
        } else if let Some(rest) = s.strip_prefix("GW:").or_else(|| s.strip_prefix("GW ")) {
            gw = rest.trim().to_string();
        } else if let Some(rest) = s.strip_prefix("监听端口:").or_else(|| s.strip_prefix("端口:")) {
            ports = rest.trim().trim_end_matches(',').to_string();
        }
    }

    if ip.is_empty() && gw.is_empty() && ports.is_empty() {
        // 完全无法解析时，把整个串当 IP 显示
        ip = info.chars().take(40).collect();
    }

    vec![
        ("IP", ip),
        ("GW", gw),
        ("监听端口", ports),
    ]
}

fn bar_color_style(pct: f64) -> Style {
    Style::default().fg(bar_color_for(pct))
}

/// 将 CPU 采样历史转为 sparkline 字符串（▁▂▃▄▅▆▇█）
pub(crate) fn make_sparkline(samples: &[f32]) -> String {
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

/// HTML 同款变色进度条 — 按使用率阈值分段着色。
/// `indent` 控制起始空格数；调用方按各自布局指定，避免在外面再去剥离前缀。
pub(crate) fn make_colored_bar(pct: f64, width: usize, indent: usize) -> Line<'static> {
    let filled = (pct * width as f64).round() as usize;
    let empty = width.saturating_sub(filled);
    let bar_color = bar_color_for(pct);
    let mut spans: Vec<Span<'static>> = Vec::with_capacity(4);
    if indent > 0 {
        spans.push(Span::raw(" ".repeat(indent)));
    }
    spans.push(Span::styled("█".repeat(filled), Style::default().fg(bar_color)));
    spans.push(Span::styled("░".repeat(empty), Style::default().fg(theme::CLR_DIM)));
    spans.push(Span::styled(
        format!(" {:.0}%", pct * 100.0),
        Style::default().fg(bar_color),
    ));
    Line::from(spans)
}

/// 进度/百分比统一配色：>90% 红、>70% 琥珀、>50% 黄绿、其余绿
pub(crate) fn bar_color_for(pct: f64) -> ratatui::style::Color {
    if pct > 0.9 {
        theme::CLR_RED
    } else if pct > 0.7 {
        theme::CLR_AMBER
    } else if pct > 0.5 {
        theme::CLR_PROGRESS_MID
    } else {
        theme::CLR_GREEN
    }
}

/// 解析 "16.0G total, 4.2G used" → (0.26, "4.2G/16.0G")
pub(crate) fn parse_mem_pct(info: &str) -> (f64, String) {
    let parts: Vec<&str> = info.split(',').collect();
    if parts.len() >= 2 {
        let total_str = parts[0].trim().split_whitespace().next().unwrap_or("0");
        let used_str  = parts[1].trim().split_whitespace().next().unwrap_or("0");
        let total = parse_size_gb(total_str);
        let used  = parse_size_gb(used_str);
        if total > 0.0 {
            let pct = (used / total).min(1.0);
            return (pct, format!("{}/{}", used_str, total_str));
        }
    }
    (0.0, info.chars().take(20).collect())
}

/// 解析 "494G total, 20.4G free, 39% used" → (0.39, "20.4G free")
pub(crate) fn parse_disk_pct(info: &str) -> (f64, String) {
    let parts: Vec<&str> = info.split(',').collect();
    if parts.len() >= 3 {
        let total_str = parts[0].trim().split_whitespace().next().unwrap_or("0");
        let free_str  = parts[1].trim().split_whitespace().next().unwrap_or("0");
        let pct_str   = parts[2].trim().split_whitespace().next().unwrap_or("0").trim_end_matches('%');
        if let Ok(pct) = pct_str.parse::<f64>() {
            return (pct / 100.0, format!("{} free/{}", free_str, total_str));
        }
    }
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

/// 将 "4.2G" / "512M" / "1.6Gi" / "1024Mi" 转为 GB
fn parse_size_gb(s: &str) -> f64 {
    if s.is_empty() { return 0.0; }
    // 先去掉 "i" 后缀（Gi/Mi/Ki/Ti）
    let s = s.strip_suffix('i').unwrap_or(s);
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
