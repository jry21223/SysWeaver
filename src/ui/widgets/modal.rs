use ratatui::{
    Frame,
    layout::{Constraint, Layout, Rect},
    style::{Color, Modifier, Style},
    text::{Line, Span},
    widgets::{Block, BorderType, Borders, Clear, Paragraph, Wrap},
};

use crate::ui::state::ModalState;
use crate::ui::theme;

pub fn render(f: &mut Frame, area: Rect, modal: &ModalState) {
    // 弹窗固定宽度，高度自适应
    let modal_width  = (area.width * 6 / 10).max(50).min(area.width - 4);
    let modal_height = 14u16.min(area.height - 4);

    let modal_area = centered_fixed(modal_width, modal_height, area);

    // 先清空背景（覆盖下层内容）
    f.render_widget(Clear, modal_area);

    // 外框 — 颜色随风险等级变化
    let border_color = match modal.risk_level {
        crate::types::risk::RiskLevel::Critical => theme::CLR_CRITICAL,
        crate::types::risk::RiskLevel::High     => theme::CLR_HIGH,
        crate::types::risk::RiskLevel::Medium   => theme::CLR_MEDIUM,
        _                                        => theme::CLR_SUCCESS,
    };

    let title_text = match modal.risk_level {
        crate::types::risk::RiskLevel::Critical => "⚠ 危险命令检测 — 已阻止",
        crate::types::risk::RiskLevel::High     => "⚠ 危险命令检测",
        crate::types::risk::RiskLevel::Medium   => "⚠ 需要确认",
        _                                        => "ℹ  确认操作",
    };

    let block = Block::default()
        .title(Span::styled(
            format!(" {} ", title_text),
            Style::default().fg(border_color).add_modifier(Modifier::BOLD),
        ))
        .borders(Borders::ALL)
        .border_type(BorderType::Double)
        .border_style(Style::default().fg(border_color))
        .style(Style::default().bg(theme::bg_for_risk(&modal.risk_level)));

    let inner = block.inner(modal_area);
    f.render_widget(block, modal_area);

    // ── 内容布局：信息区 / 按钮区 ────────────────────────────────────────
    let [info_area, btn_area] = Layout::vertical([
        Constraint::Fill(1),
        Constraint::Length(3),
    ])
    .areas(inner);

    // ── 信息区 ────────────────────────────────────────────────────────────
    let mut info_lines: Vec<Line> = vec![
        Line::from(""),
        Line::from(vec![
            Span::styled("  工具：", theme::style_dim()),
            Span::styled(modal.tool.clone(), theme::style_tool()),
        ]),
    ];

    if !modal.command_preview.is_empty() {
        let cmd_short: String = modal.command_preview.chars().take(60).collect();
        info_lines.push(Line::from(vec![
            Span::styled("  命令：", theme::style_dim()),
            Span::styled(cmd_short, Style::default().fg(theme::CLR_RED).add_modifier(Modifier::BOLD)),
        ]));
    }

    info_lines.push(Line::from(""));
    info_lines.push(Line::from(vec![
        Span::styled("  风险：", theme::style_dim()),
        Span::styled(modal.reason.clone(), theme::style_for_risk(&modal.risk_level)),
    ]));
    info_lines.push(Line::from(vec![
        Span::styled("  影响：", theme::style_dim()),
        Span::styled(modal.impact.clone(), Style::default().fg(theme::CLR_IMPACT)),
    ]));

    if let Some(alt) = &modal.alternative {
        info_lines.push(Line::from(vec![
            Span::styled("  建议：", theme::style_dim()),
            Span::styled(alt.clone(), Style::default().fg(theme::CLR_SUGGESTION)),
        ]));
    }

    f.render_widget(
        Paragraph::new(info_lines).wrap(Wrap { trim: false }),
        info_area,
    );

    // ── 按钮区 ────────────────────────────────────────────────────────────
    let btn_block = Block::default()
        .borders(Borders::TOP)
        .border_style(Style::default().fg(theme::CLR_BTN_BORDER));
    let btn_inner = btn_block.inner(btn_area);
    f.render_widget(btn_block, btn_area);

    // Critical 级别：只有 N（不允许确认）
    let is_critical = matches!(modal.risk_level, crate::types::risk::RiskLevel::Critical);

    let (yes_style, no_style) = if is_critical {
        (
            // Critical：Yes 按钮置灰不可选
            Style::default().fg(Color::DarkGray).bg(theme::CLR_BTN_BG_DARK),
            Style::default().fg(Color::White).bg(border_color).add_modifier(Modifier::BOLD),
        )
    } else if modal.selected_yes {
        (
            Style::default().fg(Color::Black).bg(theme::CLR_SUCCESS).add_modifier(Modifier::BOLD),
            Style::default().fg(theme::CLR_BTN_FG_INACTIVE).bg(theme::CLR_BTN_BG_INACTIVE),
        )
    } else {
        (
            Style::default().fg(theme::CLR_BTN_FG_INACTIVE).bg(theme::CLR_BTN_BG_INACTIVE),
            Style::default().fg(Color::Black).bg(border_color).add_modifier(Modifier::BOLD),
        )
    };

    let buttons = if is_critical {
        Line::from(vec![
            Span::raw("  "),
            Span::styled(" 此操作已被强制阻止，无法执行 ", Style::default().fg(Color::Gray)),
            Span::raw("    "),
            Span::styled(" N 关闭 ", no_style),
        ])
    } else {
        Line::from(vec![
            Span::raw("  "),
            Span::styled(" Y 确认执行 ", yes_style),
            Span::raw("    "),
            Span::styled(" N 取消 ", no_style),
            Span::styled("    Tab 切换", theme::style_dim()),
        ])
    };

    f.render_widget(Paragraph::new(buttons), btn_inner);
}

fn centered_fixed(width: u16, height: u16, area: Rect) -> Rect {
    let x = area.x + area.width.saturating_sub(width) / 2;
    let y = area.y + area.height.saturating_sub(height) / 2;
    Rect {
        x,
        y,
        width: width.min(area.width),
        height: height.min(area.height),
    }
}
