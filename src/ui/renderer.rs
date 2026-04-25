use ratatui::{
    Frame,
    layout::{Alignment, Constraint, Layout},
    style::{Modifier, Style},
    text::{Line, Span},
    widgets::{Block, Paragraph},
};


use super::state::{ActiveTab, AppState};
use super::theme;
use super::widgets as ui_widgets;

/// 最小终端尺寸
const MIN_WIDTH: u16  = 80;
const MIN_HEIGHT: u16 = 20;

/// 顶层 draw 入口 — 由 TuiApp 每帧调用
pub fn draw(f: &mut Frame, state: &AppState) {
    let area = f.area();

    // 铺满整帧背景，防止 Ghostty 等终端的默认底色透出
    f.render_widget(Block::default().style(Style::default().bg(theme::CLR_BG)), area);

    // 终端太小时显示提示
    if area.width < MIN_WIDTH || area.height < MIN_HEIGHT {
        let msg = Paragraph::new(vec![
            Line::from(""),
            Line::from(Span::styled(
                "⚠️  终端窗口太小",
                Style::default().fg(theme::CLR_AMBER).add_modifier(Modifier::BOLD),
            )),
            Line::from(Span::styled(
                format!("最小尺寸：{}×{}，当前：{}×{}", MIN_WIDTH, MIN_HEIGHT, area.width, area.height),
                Style::default().fg(theme::CLR_FG_MUTED),
            )),
            Line::from(""),
            Line::from(Span::styled(
                "请放大终端后重试",
                Style::default().fg(theme::CLR_FG),
            )),
        ])
        .alignment(Alignment::Center);
        f.render_widget(msg, area);
        return;
    }

    // ── 垂直切分：状态栏(1) / TabBar(2) / 主体(fill) / 输入区(3) ─────────────
    let [statusbar_area, tabbar_area, main_area, input_area] = Layout::vertical([
        Constraint::Length(1),      // StatusBar
        Constraint::Length(2),      // TabBar (1 row for text + 1 row for active underline)
        Constraint::Fill(1),        // 主内容区
        Constraint::Length(2),      // InputBar（分隔线 + 输入行，紧贴终端底部）
    ])
    .areas(area);

    // ── 主体水平切：聊天区 / 状态面板 ─────────────────────────────────────
    // 设计稿：侧栏 260px(≈34 chars) 展开 / 36px(≈4 chars) 折叠
    // 终端窄时按比例退化避免主区被挤压
    let side_width: u16 = if state.side_collapsed {
        4
    } else if main_area.width < 90 {
        24
    } else if main_area.width >= 130 {
        34
    } else {
        30
    };
    let [chat_area, divider_area, status_area] = Layout::horizontal([
        Constraint::Fill(1),
        Constraint::Length(1),
        Constraint::Length(side_width),
    ])
    .areas(main_area);

    // 主区与侧栏之间的竖向分隔线（设计：border-left: 1px solid BORDER）
    let divider_line: Vec<Line> = (0..divider_area.height)
        .map(|_| Line::from(Span::styled("│", Style::default().fg(theme::CLR_BORDER))))
        .collect();
    f.render_widget(
        Paragraph::new(divider_line).style(Style::default().bg(theme::CLR_BG)),
        divider_area,
    );

    // ── 渲染各区 ─────────────────────────────────────────────────────────
    ui_widgets::statusbar::render(f, statusbar_area, state);
    ui_widgets::tabbar::render(f, tabbar_area, &state.active_tab);

    // 标签页内容切换
    match state.active_tab {
        ActiveTab::Chat => ui_widgets::chat::render(f, chat_area, state),
        ActiveTab::Monitor => ui_widgets::monitor::render(f, chat_area, state),
        ActiveTab::History => ui_widgets::history::render(f, chat_area, state),
    }

    // 侧栏面板
    ui_widgets::status::render(f, status_area, state);

    // 输入栏仅在对话标签页显示
    if state.active_tab == ActiveTab::Chat {
        ui_widgets::input::render(f, input_area, state);
    }

    // ── 弹窗最后渲染（覆盖其他内容）─────────────────────────────────────
    if let Some(modal) = &state.modal {
        ui_widgets::modal::render(f, area, modal);
    }
}