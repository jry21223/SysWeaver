use ratatui::{
    Frame,
    layout::{Constraint, Layout, Rect},
    style::{Color, Modifier, Style},
    text::{Line, Span},
    widgets::Paragraph,
};

use crate::ui::state::AppState;
use crate::ui::theme;

// 右上角轮播 tips（每条约 5 秒，80ms * 62 ≈ 5s）
const TIPS: &[&str] = &[
    "试试: 查看系统负载",
    "试试: 找出大文件 >100M",
    "试试: 列出监听端口",
    "试试: 查看最近登录记录",
    "试试: 内存占用 Top5 进程",
    "试试: 检查磁盘健康状态",
    "Ctrl+P/N  浏览输入历史",
    "Ctrl+Y    复制 Agent 回复",
    "PgUp/Dn   滚动对话历史",
    "/status   实时系统快照",
    "/report   生成健康报告",
    "/export   导出对话记录",
    "/playbook 保存常用操作",
];

const TIPS_TICKS_PER_SLIDE: u64 = 62; // 62 × 80ms ≈ 5 秒

pub fn render(f: &mut Frame, area: Rect, state: &AppState) {
    let mode_color = match state.mode.as_str() {
        "safe"   => Color::Rgb(80, 220, 120),
        "normal" => Color::Rgb(100, 160, 255),
        "auto"   => Color::Rgb(255, 200, 50),
        _        => Color::Gray,
    };

    // ── 左侧内容 ─────────────────────────────────────────────────────────
    let mut left_spans = vec![
        // 用户名徽章（左上角）
        Span::styled(
            format!(" ● {} ", state.username),
            Style::default()
                .fg(Color::Rgb(212, 160, 74))   // Tokyo Night amber
                .bg(theme::CLR_STATUSBAR)
                .add_modifier(Modifier::BOLD),
        ),
        Span::styled(" │ ", theme::style_dim()),
        Span::styled(" 🤖 jij ", Style::default()
            .fg(Color::White)
            .bg(theme::CLR_STATUSBAR)
            .add_modifier(Modifier::BOLD)),
        Span::styled(" │ ", theme::style_dim()),
        Span::styled(
            format!(" {} ", state.mode.to_uppercase()),
            Style::default().fg(Color::Black).bg(mode_color).add_modifier(Modifier::BOLD),
        ),
        Span::styled(" │ ", theme::style_dim()),
        Span::styled(
            format!(" {} ", state.provider),
            Style::default().fg(Color::Rgb(180, 180, 200)).bg(theme::CLR_STATUSBAR),
        ),
        Span::styled(" │ ", theme::style_dim()),
        Span::styled(
            format!(" sess:{} ", &state.session_id[..8.min(state.session_id.len())]),
            theme::style_dim(),
        ),
    ];

    // 若正在思考，显示步骤进度 + spinner
    if state.is_thinking {
        left_spans.push(Span::styled(" │ ", theme::style_dim()));
        if state.task_step > 0 {
            left_spans.push(Span::styled(
                format!(" Step {} ", state.task_step),
                Style::default()
                    .fg(Color::Rgb(100, 220, 255))
                    .bg(theme::CLR_STATUSBAR)
                    .add_modifier(Modifier::BOLD),
            ));
            if !state.task_hint.is_empty() {
                let hint_short: String = state.task_hint.chars().take(20).collect();
                left_spans.push(Span::styled(
                    format!("• {} ", hint_short),
                    Style::default().fg(Color::Rgb(160, 160, 200)).bg(theme::CLR_STATUSBAR),
                ));
            }
        }
        left_spans.push(Span::styled(
            format!(" {} 处理中… ", state.spinner_char()),
            Style::default()
                .fg(Color::Rgb(255, 220, 100))
                .bg(theme::CLR_STATUSBAR)
                .add_modifier(Modifier::BOLD),
        ));
    }

    // 语音 TTS 状态
    if state.voice_tts_enabled {
        left_spans.push(Span::styled(" │ ", theme::style_dim()));
        left_spans.push(Span::styled(
            " 🔊 TTS ",
            Style::default()
                .fg(Color::Black)
                .bg(Color::Rgb(80, 200, 120))
                .add_modifier(Modifier::BOLD),
        ));
    }

    // 复制成功通知
    if state.copy_notice_frames > 0 {
        left_spans.push(Span::styled(" │ ", theme::style_dim()));
        left_spans.push(Span::styled(
            " ✓ 已复制 ",
            Style::default()
                .fg(Color::Black)
                .bg(Color::Rgb(100, 220, 100))
                .add_modifier(Modifier::BOLD),
        ));
    }

    // ── 右侧内容：轮播 tips ───────────────────────────────────────────────
    let tip_idx = ((state.tips_tick / TIPS_TICKS_PER_SLIDE) as usize) % TIPS.len();
    let tip_text = TIPS[tip_idx];

    let right_spans = vec![
        Span::styled(
            format!(" 💡 {} ", tip_text),
            Style::default()
                .fg(Color::Rgb(130, 170, 130))
                .bg(theme::CLR_STATUSBAR),
        ),
    ];

    // ── 布局：左填充 / 右固定宽度 ────────────────────────────────────────
    let tip_display_width = (tip_text.chars().count() + 5) as u16; // " 💡 " + " "
    let [left_area, right_area] = Layout::horizontal([
        Constraint::Fill(1),
        Constraint::Length(tip_display_width.max(20).min(area.width / 2)),
    ])
    .areas(area);

    let left_para = Paragraph::new(Line::from(left_spans))
        .style(theme::style_statusbar());
    let right_para = Paragraph::new(Line::from(right_spans))
        .style(theme::style_statusbar());

    f.render_widget(left_para, left_area);
    f.render_widget(right_para, right_area);
}
