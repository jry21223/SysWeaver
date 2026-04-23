use ratatui::{
    Frame,
    layout::{Alignment, Constraint, Layout},
    style::{Color, Modifier, Style},
    text::{Line, Span},
    widgets::Paragraph,
};


use super::state::AppState;
use super::widgets as ui_widgets;

/// 最小终端尺寸
const MIN_WIDTH: u16  = 80;
const MIN_HEIGHT: u16 = 20;

/// 顶层 draw 入口 — 由 TuiApp 每帧调用
pub fn draw(f: &mut Frame, state: &AppState) {
    let area = f.area();

    // 终端太小时显示提示
    if area.width < MIN_WIDTH || area.height < MIN_HEIGHT {
        let msg = Paragraph::new(vec![
            Line::from(""),
            Line::from(Span::styled(
                "⚠️  终端窗口太小",
                Style::default().fg(Color::Yellow).add_modifier(Modifier::BOLD),
            )),
            Line::from(Span::styled(
                format!("最小尺寸：{}×{}，当前：{}×{}", MIN_WIDTH, MIN_HEIGHT, area.width, area.height),
                Style::default().fg(Color::Gray),
            )),
            Line::from(""),
            Line::from(Span::styled(
                "请放大终端后重试",
                Style::default().fg(Color::White),
            )),
        ])
        .alignment(Alignment::Center);
        f.render_widget(msg, area);
        return;
    }

    // ── 垂直切分：状态栏(1) / 主体(fill) / 输入区(4) ────────────────────
    let [statusbar_area, main_area, input_area] = Layout::vertical([
        Constraint::Length(1),
        Constraint::Fill(1),
        Constraint::Length(4),
    ])
    .areas(area);

    // ── 主体水平切：聊天区(65%) / 状态面板(35%) ─────────────────────────
    let [chat_area, status_area] = Layout::horizontal([
        Constraint::Percentage(65),
        Constraint::Percentage(35),
    ])
    .areas(main_area);

    // ── 渲染各区 ─────────────────────────────────────────────────────────
    ui_widgets::statusbar::render(f, statusbar_area, state);
    ui_widgets::chat::render(f, chat_area, state);
    ui_widgets::status::render(f, status_area, state);
    ui_widgets::input::render(f, input_area, state);

    // ── 弹窗最后渲染（覆盖其他内容）─────────────────────────────────────
    if let Some(modal) = &state.modal {
        ui_widgets::modal::render(f, area, modal);
    }
}

