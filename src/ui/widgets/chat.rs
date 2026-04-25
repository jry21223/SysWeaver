use ratatui::{
    Frame,
    layout::Rect,
    style::{Color, Modifier, Style},
    text::{Line, Span},
    widgets::{Paragraph, Wrap},
};
use unicode_width::UnicodeWidthStr;

use crate::ui::state::{AppState, ChatLine};
use crate::ui::theme;

/// 清理控制字符：\t → 两个空格，\r 丢弃，其他控制字符丢弃
/// 防止 crossterm 把 \t 直接发送到终端触发 tab-stop 导致乱码
fn sanitize_output(s: &str) -> String {
    let mut out = String::with_capacity(s.len() + 4);
    for c in s.chars() {
        match c {
            '\t' => { out.push(' '); out.push(' '); }
            '\r' => {}
            c if c.is_control() => {}
            c => out.push(c),
        }
    }
    out
}

pub fn render(f: &mut Frame, area: Rect, state: &AppState) {
    // 无边框设计（匹配 JSX：只有内容区，无外部边框）
    let inner = area;

    // 将所有 ChatLine 转为 ratatui Line 列表
    let mut lines: Vec<Line> = Vec::new();

    for msg in &state.messages {
        match msg {
            ChatLine::UserMsg(text) => {
                // JSX: 用户消息用 › 符号，琥珀色，文字与符号同行（flex row）
                let prefix_style = Style::default().fg(theme::CLR_AMBER).add_modifier(Modifier::BOLD);
                let body_style = Style::default().fg(theme::CLR_FG);
                let text_lines: Vec<&str> = text.lines().collect();
                if text_lines.is_empty() {
                    lines.push(Line::from(vec![Span::styled("› ", prefix_style)]));
                } else {
                    lines.push(Line::from(vec![
                        Span::styled("› ", prefix_style),
                        Span::styled(text_lines[0].to_string(), body_style),
                    ]));
                    for l in &text_lines[1..] {
                        lines.push(Line::from(vec![
                            Span::raw("  "),
                            Span::styled(l.to_string(), body_style),
                        ]));
                    }
                }
                lines.push(Line::from(""));
            }

            ChatLine::AgentMsg(text) => {
                // JSX: Agent 消息用 ◆ 符号，青色，文字与符号同行
                let prefix_style = Style::default().fg(theme::CLR_CYAN);
                let body_style = Style::default().fg(theme::CLR_FG);
                let text_lines: Vec<&str> = text.lines().collect();
                if text_lines.is_empty() {
                    lines.push(Line::from(vec![Span::styled("◆ ", prefix_style)]));
                } else {
                    lines.push(Line::from(vec![
                        Span::styled("◆ ", prefix_style),
                        Span::styled(sanitize_output(text_lines[0]), body_style),
                    ]));
                    for l in &text_lines[1..] {
                        lines.push(Line::from(vec![
                            Span::raw("  "),
                            Span::styled(sanitize_output(l), body_style),
                        ]));
                    }
                }
                lines.push(Line::from(""));
            }

            ChatLine::ToolCallLine { step, tool, args, dry_run } => {
                let dry_tag = if *dry_run {
                    Span::styled(" [DRY-RUN]", theme::style_dryrun())
                } else {
                    Span::raw("")
                };

                // 截断 args 避免过长
                let args_display = if args.width() > 60 {
                    format!("{}…", &args[..args.char_indices().nth(57).map(|(i,_)| i).unwrap_or(args.len())])
                } else {
                    args.clone()
                };

                // 工具调用：琥珀色图标 + 步骤号
                lines.push(Line::from(vec![
                    Span::styled(format!("  ◇ Step {}: ", step), Style::default().fg(theme::CLR_AMBER)),
                    Span::styled(tool.clone(), Style::default().fg(theme::CLR_CYAN).add_modifier(Modifier::UNDERLINED)),
                    Span::styled(format!("({})", args_display), theme::style_dim()),
                    dry_tag,
                ]));
            }

            ChatLine::ToolResultLine { success, preview, duration_ms } => {
                // JSX: 成功用 ● 绿色，失败用 ● 红色
                let icon = if *success { "  ● " } else { "  ● " };
                let icon_style = if *success {
                    Style::default().fg(theme::CLR_GREEN)
                } else {
                    Style::default().fg(theme::CLR_RED)
                };
                let text_style = if *success {
                    Style::default().fg(theme::CLR_FG_MUTED)
                } else {
                    theme::style_error()
                };
                let preview_lines: Vec<&str> = preview.lines().collect();
                let last_idx = preview_lines.len().saturating_sub(1);
                if preview_lines.is_empty() {
                    lines.push(Line::from(vec![
                        Span::styled(icon, icon_style),
                        Span::styled(format!("({}ms)", duration_ms), theme::style_dim()),
                    ]));
                } else {
                    for (i, pl) in preview_lines.iter().enumerate() {
                        let prefix = if i == 0 { icon } else { "      " };
                        let duration_span = if i == last_idx {
                            Span::styled(format!("  ({}ms)", duration_ms), theme::style_dim())
                        } else {
                            Span::raw("")
                        };
                        lines.push(Line::from(vec![
                            Span::styled(prefix, if i == 0 { icon_style } else { Style::default() }),
                            Span::styled(sanitize_output(pl), text_style),
                            duration_span,
                        ]));
                    }
                }
            }

            ChatLine::ErrorLine(msg) => {
                lines.push(Line::from(vec![
                    Span::styled("  ● ", Style::default().fg(theme::CLR_RED)),
                    Span::styled(sanitize_output(msg), theme::style_error()),
                ]));
                lines.push(Line::from(""));
            }

            ChatLine::Separator => {
                let sep = "─".repeat(inner.width.saturating_sub(4) as usize);
                lines.push(Line::from(Span::styled(sep, theme::style_dim())));
            }

            ChatLine::WatchdogAlert { severity, message } => {
                lines.push(Line::from(vec![
                    Span::styled(format!("  {} ", severity), Style::default().fg(Color::Rgb(255, 180, 50)).add_modifier(Modifier::BOLD)),
                    Span::styled(sanitize_output(message), Style::default().fg(Color::Rgb(255, 220, 100))),
                ]));
                lines.push(Line::from(""));
            }
        }
    }

    // spinner 行（thinking 状态）- JSX 风格：⠋⠙⠹⠸⠼⠴⠦⠧⠇⠏ + "思考中..."
    if state.is_thinking {
        lines.push(Line::from(vec![
            Span::styled(
                format!("{} 思考中…", state.spinner_char()),
                Style::default()
                    .fg(theme::CLR_CYAN),
            ),
        ]));
    }

    // 空对话时显示欢迎提示（匹配 JSX 简洁风格）
    if state.messages.is_empty() && !state.is_thinking {
        let welcome = vec![
            Line::from(""),
            Line::from(Span::styled(
                "  agent-unix 已就绪。用自然语言描述你的需求。",
                Style::default().fg(theme::CLR_FG_MUTED),
            )),
            Line::from(""),
            Line::from(Span::styled("  示例：", theme::style_dim())),
            Line::from(Span::styled("    • 查看磁盘使用情况", theme::style_dim())),
            Line::from(Span::styled("    • 列出内存占用最高的进程", theme::style_dim())),
            Line::from(Span::styled("    • 把 nginx 配置到 8080 端口", theme::style_dim())),
            Line::from(""),
        ];
        let para = Paragraph::new(welcome)
            .wrap(Wrap { trim: false });
        f.render_widget(para, inner);
        return;
    }

    // 计算每个 Line 折行后的实际视觉行数（Wrap 模式下一个 Line 可占多行）
    // 加 5 的安全余量补偿 CJK/emoji 宽度在不同终端下的细微差异
    let visible_height = inner.height as usize;
    let inner_width = inner.width as usize;

    let total_visual_rows: usize = if inner_width == 0 {
        lines.len()
    } else {
        lines.iter().map(|line| {
            let w = line.width();
            if w == 0 { 1 } else { (w + inner_width - 1) / inner_width }
        }).sum::<usize>().saturating_add(5).max(1)
    };

    let bottom_scroll = total_visual_rows.saturating_sub(visible_height);

    // 分离 Paragraph 滚动值和提示用滚动值
    // 自动滚底时用 u16::MAX → ratatui 自动 clamp 到内容底部，消除手动计算误差
    let (para_scroll, hint_scroll) = if state.scroll_offset == usize::MAX {
        (u16::MAX, bottom_scroll)
    } else if state.scroll_offset > total_visual_rows {
        let steps = usize::MAX - state.scroll_offset;
        let s = bottom_scroll.saturating_sub(steps);
        (s.min(u16::MAX as usize) as u16, s)
    } else {
        let s = state.scroll_offset.min(bottom_scroll);
        (s.min(u16::MAX as usize) as u16, s)
    };

    let para = Paragraph::new(lines)
        .wrap(Wrap { trim: false })
        .scroll((para_scroll, 0));

    f.render_widget(para, inner);

    // 右下角显示滚动位置（如果不在底部）
    if hint_scroll + visible_height < total_visual_rows {
        let remaining = total_visual_rows.saturating_sub(hint_scroll + visible_height);
        let hint = format!(" ↓{} ", remaining);
        let hint_area = Rect {
            x: area.x + area.width.saturating_sub(hint.len() as u16 + 2),
            y: area.y + area.height - 1,
            width: hint.len() as u16 + 2,
            height: 1,
        };
        let hint_widget = Paragraph::new(Span::styled(hint, theme::style_dim()));
        f.render_widget(hint_widget, hint_area);
    }
}