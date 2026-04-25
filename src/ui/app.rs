use std::io;
use std::time::Duration;

use anyhow::Result;
use crossterm::{
    event::{
        DisableMouseCapture, EnableMouseCapture, Event, EventStream, KeyCode, KeyModifiers,
        MouseEventKind,
    },
    execute,
    terminal::{EnterAlternateScreen, LeaveAlternateScreen, disable_raw_mode, enable_raw_mode},
};
use futures::StreamExt;
use ratatui::{Terminal, backend::CrosstermBackend};
use tokio::sync::mpsc;
use uuid::Uuid;

use crate::agent::memory::SystemContext;
use crate::agent::r#loop::AgentLoop;
use crate::config::LlmConfig;
use crate::context::system_scan;
use crate::executor::ssh::SshConfig;
use crate::ui::{
    AgentEvent,
    renderer,
    state::{ActiveTab, AppState, ChatLine},
};
use crate::voice::VoiceEngine;
use crate::watchdog::{AlertSeverity, create_watchdog_system};

// 录音状态（local to run_tui）
struct RecordingState {
    child: tokio::process::Child,
    wav_path: String,
}

/// TUI 入口：初始化终端 → 启动 agent task → 运行事件循环
pub async fn run_tui(
    llm_config: LlmConfig,
    mode: String,
    ctx: SystemContext,
    ssh_config: Option<SshConfig>,
) -> Result<()> {
    // ── 终端初始化 ────────────────────────────────────────────────────────
    enable_raw_mode()?;
    let mut stdout = io::stdout();
    execute!(stdout, EnterAlternateScreen, EnableMouseCapture)?;
    let backend = CrosstermBackend::new(stdout);
    let mut terminal = Terminal::new(backend)?;

    // ── panic hook：崩溃时也要恢复终端 ───────────────────────────────────
    let original_hook = std::panic::take_hook();
    std::panic::set_hook(Box::new(move |info| {
        let mut stderr = io::stderr();
        let _ = execute!(stderr, LeaveAlternateScreen, DisableMouseCapture);
        let _ = disable_raw_mode();
        original_hook(info);
    }));

    // ── 通信 channel ─────────────────────────────────────────────────────
    // agent → TUI
    let (event_tx, mut event_rx) = mpsc::channel::<AgentEvent>(64);
    // TUI 用户输入 → agent
    let (input_tx, mut input_rx) = mpsc::channel::<String>(8);

    // ── AppState 初始化 ───────────────────────────────────────────────────
    let provider_short = llm_config.model.chars().take(24).collect::<String>();
    let session_id = Uuid::new_v4().to_string();
    let username = std::env::var("USER")
        .or_else(|_| std::env::var("LOGNAME"))
        .or_else(|_| std::env::var("USERNAME"))
        .unwrap_or_else(|_| "user".to_string());

    // ── SSH 模式：先做一次同步远程扫描，让 Agent 系统提示词使用远程上下文 ──
    // 否则 LLM 会以为自己运行在用户的本机上（如 MacBook）
    let effective_ctx = if let Some(ref ssh) = ssh_config {
        match tokio::time::timeout(
            Duration::from_secs(15),
            crate::context::system_scan::scan_remote(ssh),
        ).await {
            Ok(remote) if !remote.hostname.is_empty() && remote.hostname != "未知" => remote,
            _ => ctx.clone(),  // 远程扫描失败时回退到本地 ctx（避免阻塞启动）
        }
    } else {
        ctx.clone()
    };

    let mut state = AppState::new(mode.clone(), provider_short, session_id.clone(), username);
    state.system_ctx = Some(effective_ctx.clone());
    if let Some(ref ssh) = ssh_config {
        state.is_remote = true;
        state.remote_label = Some(format!("🔗 {} (SSH)", ssh.display()));
    }

    // 启动欢迎消息（展示系统环境感知能力）
    let mode_badge = if let Some(ref ssh) = ssh_config {
        format!("🔗 SSH 远程 · {}", ssh.display())
    } else {
        "💻 本地模式".to_string()
    };
    let version = env!("CARGO_PKG_VERSION");
    let ctx = effective_ctx.clone();  // shadow: 后续欢迎/agent 都使用 effective ctx
    let welcome = format!(
        "╭─────────────────────────────────────────────────────────╮\n\
         │  🤖  sysweaver v{:<8}   {:<30}  │\n\
         │      AI Hackathon 2026 · 超聚变 αFUSION 预赛           │\n\
         ╰─────────────────────────────────────────────────────────╯\n\n\
         ┌─ 📊 系统环境 ─────────────────────────────────────────────\n\
         │  OS         {}\n\
         │  主机名      {}\n\
         │  CPU        {}\n\
         │  内存        {}\n\
         │  磁盘        {}\n\
         │  网络        {}\n\
         │  包管理器    {}\n\
         └───────────────────────────────────────────────────────────\n\n\
         ┌─ 💡 常用命令 ─────────────────────────────────────────────\n\
         │  /help         查看完整帮助\n\
         │  /status       实时系统状态 & 异常检测\n\
         │  /report       生成系统健康综合报告\n\
         │  /history      操作历史（含可撤销标记）\n\
         │  /undo         撤销上一步操作\n\
         │  /playbook     Playbook 管理（save / list / run）\n\
         │  /export       导出完整对话到 Markdown\n\
         │  /voice tts    开启语音朗读\n\
         │  /clear        清除对话上下文\n\
         │  /exit         退出\n\
         └───────────────────────────────────────────────────────────\n\n\
         ┌─ ⌨️  快捷键 ──────────────────────────────────────────────\n\
         │  Ctrl+Y  复制最后一条回复   Ctrl+P/N  浏览历史\n\
         │  PgUp/Dn 滚动对话区        End       滚到底部\n\
         └───────────────────────────────────────────────────────────\n\n\
         ✨ 请用自然语言描述您的需求，例如：\n\
           · 查看磁盘使用情况\n\
           · 列出内存占用最高的 5 个进程\n\
           · 把 nginx 配置改到 8080 端口并重启",
        version,
        mode_badge,
        ctx.os_info,
        ctx.hostname,
        ctx.cpu_info,
        ctx.memory_info,
        ctx.disk_info,
        ctx.network_info,
        ctx.package_manager,
    );
    state.push_line(ChatLine::AgentMsg(welcome));
    state.push_line(ChatLine::Separator);

    // ── 启动 Agent Task ───────────────────────────────────────────────────
    let agent_event_tx = event_tx.clone();
    let agent_llm = llm_config.clone();
    let agent_mode = mode.clone();
    let agent_ctx = ctx.clone();

    let agent_ssh = ssh_config.clone();
    tokio::spawn(async move {
        // 保留副本，供 /clear 命令重建 agent
        let saved_llm = agent_llm.clone();
        let saved_mode = agent_mode.clone();
        let saved_ssh = agent_ssh.clone();
        let is_ssh = saved_ssh.is_some();

        let mut agent = match agent_ssh {
            Some(ssh) => AgentLoop::new_with_tui_and_ssh(
                agent_llm,
                &agent_mode,
                agent_ctx,
                agent_event_tx.clone(),
                ssh,
            ),
            None => AgentLoop::new_with_tui(
                agent_llm,
                &agent_mode,
                agent_ctx,
                agent_event_tx.clone(),
            ),
        };
        let mut voice = VoiceEngine::new();

        loop {
            let input = match input_rx.recv().await {
                Some(s) => s,
                None => break, // TUI 关闭
            };

            // 斜线命令
            match input.trim() {
                "/voice" | "/voice status" => {
                    let status = voice.status_summary();
                    let _ = agent_event_tx.send(AgentEvent::AgentReply(status)).await;
                    continue;
                }
                "/voice tts" => {
                    voice.tts_enabled = !voice.tts_enabled;
                    let msg = if voice.tts_enabled {
                        "🔊 语音朗读已开启（Agent 回复将被朗读）\n   关闭：/voice tts".to_string()
                    } else {
                        "🔇 语音朗读已关闭\n   开启：/voice tts".to_string()
                    };
                    let _ = agent_event_tx.send(AgentEvent::VoiceTtsToggle(voice.tts_enabled)).await;
                    let _ = agent_event_tx.send(AgentEvent::AgentReply(msg)).await;
                    continue;
                }
                "/voice off" => {
                    voice.tts_enabled = false;
                    voice.stt_enabled = false;
                    let _ = agent_event_tx.send(AgentEvent::VoiceTtsToggle(false)).await;
                    let _ = agent_event_tx.send(AgentEvent::AgentReply("🔇 所有语音功能已关闭".to_string())).await;
                    continue;
                }
                "/help" => {
                    let help = "📖 sysweaver 帮助\n\n\
                        【命令列表】\n\
                        /help           显示此帮助\n\
                        /status         查看当前系统状态\n\
                        /history        查看操作历史\n\
                        /undo           撤销上一步操作\n\
                        /clear          清除对话上下文，重新开始\n\
                        /export         导出完整对话历史到文件（可复制）\n\
                        /voice          语音功能状态\n\
                        /voice tts      开启/关闭语音朗读\n\
                        /voice off      关闭所有语音功能\n\
                        /report         生成系统健康综合报告\n\
                        /playbook list          列出 Playbook\n\
                        /playbook save <名称>   保存最近操作为 Playbook\n\
                        /playbook run <名称>    执行 Playbook\n\
                        /exit           退出\n\n\
                        【快捷键】\n\
                        Ctrl+Y   复制最后一条 Agent 回复\n\
                        Ctrl+P/N 浏览历史输入\n\
                        PgUp/Dn  滚动对话区\n\n\
                        【示例指令】\n\
                        · 查看磁盘使用情况\n\
                        · 列出内存占用最高的 5 个进程\n\
                        · 创建用户 testuser\n\
                        · 安装并启动 nginx\n\
                        · 把 nginx 配置改到 8080 端口并重启\n\n\
                        【安全模式】\n\
                        · 高危操作（CRITICAL）将被直接拒绝\n\
                        · 高风险操作（HIGH）需要您确认才执行\n\
                        · 可用 --mode safe 对中等风险也要求确认".to_string();
                    let _ = agent_event_tx.send(AgentEvent::AgentReply(help)).await;
                    continue;
                }
                "/status" => {
                    let _ = agent_event_tx.send(AgentEvent::Thinking).await;
                    let new_ctx = crate::context::system_scan::scan().await;
                    let anomalies = crate::context::system_scan::detect_anomalies(&new_ctx);
                    let anomaly_section = if anomalies.is_empty() {
                        "  ✅ 系统运行正常，未检测到异常".to_string()
                    } else {
                        format!("\n⚠️  检测到异常：\n{}", anomalies.iter().map(|a| format!("  {}", a)).collect::<Vec<_>>().join("\n"))
                    };
                    let status = format!(
                        "📊 系统状态速览\n\n  OS：{}\n  主机：{}\n  CPU：{}\n  内存：{}\n  磁盘：{}\n  包管理器：{}\n  活跃服务：{}\n{}",
                        new_ctx.os_info,
                        new_ctx.hostname,
                        new_ctx.cpu_info,
                        new_ctx.memory_info,
                        new_ctx.disk_info,
                        new_ctx.package_manager,
                        if new_ctx.running_services.is_empty() {
                            "（无）".to_string()
                        } else {
                            new_ctx.running_services.join(", ")
                        },
                        anomaly_section,
                    );
                    let _ = agent_event_tx.send(AgentEvent::SystemUpdate(new_ctx)).await;
                    let _ = agent_event_tx.send(AgentEvent::AgentReply(status)).await;
                    continue;
                }
                "/history" => {
                    let history = agent.get_history_summary();
                    let _ = agent_event_tx
                        .send(AgentEvent::AgentReply(history))
                        .await;
                    continue;
                }
                "/undo" => {
                    let msg = match agent.undo_last().await {
                        Ok(m)  => m,
                        Err(e) => format!("⚠️  {}", e),
                    };
                    let _ = agent_event_tx.send(AgentEvent::AgentReply(msg)).await;
                    continue;
                }
                "/report" => {
                    let _ = agent_event_tx.send(AgentEvent::Thinking).await;
                    let ctx = crate::context::system_scan::scan().await;
                    let anomalies = crate::context::system_scan::detect_anomalies(&ctx);
                    let anomaly_section = if anomalies.is_empty() {
                        "  ✅ 未检测到系统异常".to_string()
                    } else {
                        format!("⚠️  告警：\n{}", anomalies.iter().map(|a| format!("  {}", a)).collect::<Vec<_>>().join("\n"))
                    };
                    let report = format!(
                        "📋 系统健康综合报告\n\
                        ──────────────────────────────────\n\
                        基础信息\n\
                          主机名：{}\n  操作系统：{}\n  包管理器：{}\n\
                        ──────────────────────────────────\n\
                        资源使用\n\
                          CPU：{}\n  内存：{}\n  磁盘：{}\n\
                        ──────────────────────────────────\n\
                        网络与服务\n\
                          网络：{}\n  活跃服务：{}\n\
                        ──────────────────────────────────\n\
                        异常检测\n  {}",
                        ctx.hostname, ctx.os_info, ctx.package_manager,
                        ctx.cpu_info, ctx.memory_info, ctx.disk_info,
                        ctx.network_info,
                        if ctx.running_services.is_empty() { "（无）".to_string() } else { ctx.running_services.join(", ") },
                        anomaly_section,
                    );
                    let _ = agent_event_tx.send(AgentEvent::SystemUpdate(ctx)).await;
                    let _ = agent_event_tx.send(AgentEvent::AgentReply(report)).await;
                    continue;
                }
                "/clear" => {
                    let new_ctx = crate::context::system_scan::scan().await;
                    agent = match saved_ssh.as_ref() {
                        Some(ssh) => AgentLoop::new_with_tui_and_ssh(
                            saved_llm.clone(),
                            &saved_mode,
                            new_ctx,
                            agent_event_tx.clone(),
                            ssh.clone(),
                        ),
                        None => AgentLoop::new_with_tui(
                            saved_llm.clone(),
                            &saved_mode,
                            new_ctx,
                            agent_event_tx.clone(),
                        ),
                    };
                    let _ = agent_event_tx.send(AgentEvent::AgentReply("🗑️ 对话上下文已清除，重新开始".to_string())).await;
                    continue;
                }
                _ => {}
            }

            // 普通指令：交给 AgentLoop
            let _ = agent_event_tx.send(AgentEvent::Thinking).await;
            match agent.run(&input, false).await {
                Ok(reply) => {
                    // TTS 朗读（不阻塞主循环）
                    if voice.tts_enabled {
                        let tts_text = reply.clone();
                        let mut tts_engine = VoiceEngine::new();
                        tts_engine.tts_enabled = true;
                        tokio::spawn(async move {
                            let _ = tts_engine.speak(&tts_text).await;
                        });
                    }
                    let _ = agent_event_tx.send(AgentEvent::AgentReply(reply)).await;
                    // 刷新系统状态（SSH 模式下跳过本地扫描）
                    if !is_ssh {
                        let new_ctx = system_scan::scan().await;
                        let _ = agent_event_tx.send(AgentEvent::SystemUpdate(new_ctx)).await;
                    }
                }
                Err(e) => {
                    let _ = agent_event_tx
                        .send(AgentEvent::Error(e.to_string()))
                        .await;
                }
            }
        }
    });

    // ── 启动 Watchdog 后台监控（TUI 模式下把告警推送到事件流）────────────
    let watchdog_event_tx = event_tx.clone();
    tokio::spawn(async move {
        let (watchdog, mut handler) = create_watchdog_system();
        watchdog.start();
        while let Some(alert) = handler.alert_rx.recv().await {
            let severity = match alert.severity {
                AlertSeverity::Critical => "🚨 CRITICAL".to_string(),
                AlertSeverity::Warning  => "⚠️  WARNING".to_string(),
                AlertSeverity::Info     => "ℹ️  INFO".to_string(),
            };
            let _ = watchdog_event_tx.send(AgentEvent::WatchdogAlert {
                severity,
                message: alert.message,
            }).await;
        }
    });

    // ── 服务状态初始化（系统信息已在启动时同步扫描完成）─────────────────
    if let Some(ref ssh) = ssh_config {
        let ssh_scan = ssh.clone();
        let svc_tx = event_tx.clone();
        tokio::spawn(async move {
            let svcs = system_scan::get_remote_service_status(&ssh_scan).await;
            let _ = svc_tx.send(AgentEvent::ServiceStatusUpdate(svcs)).await;
        });
    } else {
        let svc_tx = event_tx.clone();
        tokio::spawn(async move {
            let svcs = system_scan::get_service_status().await;
            let _ = svc_tx.send(AgentEvent::ServiceStatusUpdate(svcs)).await;
        });
    }

    // ── 主事件循环 ────────────────────────────────────────────────────────
    let mut event_stream = EventStream::new();
    let mut render_tick = tokio::time::interval(Duration::from_millis(33)); // ~30fps
    let mut spinner_tick = tokio::time::interval(Duration::from_millis(80));
    // SSH/本地系统信息每 30s 刷新一次
    let mut system_refresh_tick = tokio::time::interval(Duration::from_secs(30));
    system_refresh_tick.tick().await; // 跳过第一个立即触发
    // 进程列表 + CPU 采样：每 2s 在后台 spawn_blocking 跑 ps/top，结果通过 event 通道回传
    // 这样事件循环不会被 ps aux + top -l1 阻塞数百毫秒
    let mut proc_refresh_tick = tokio::time::interval(Duration::from_secs(2));
    // 录音状态（按 F5 启动/停止）
    let mut recording: Option<RecordingState> = None;

    let result = loop {
        tokio::select! {
            // 渲染帧（仅 dirty 或有动画需要时重绘）
            _ = render_tick.tick() => {
                let needs_render = state.dirty
                    || state.is_thinking
                    || state.voice_recording
                    || state.copy_notice_frames > 0;
                if needs_render {
                    if let Err(e) = terminal.draw(|f| renderer::draw(f, &state)) {
                        break Err(e.into());
                    }
                    state.dirty = false;
                }
            }

            // Spinner 动画 + 复制通知倒计时（不再做任何阻塞 IO）
            _ = spinner_tick.tick() => {
                if state.is_thinking {
                    state.tick_spinner();
                }
                if state.copy_notice_frames > 0 {
                    state.tick_copy_notice();
                }
            }

            // 后台进程/CPU 采样：spawn_blocking 跑 ps/top，避免阻塞事件循环
            _ = proc_refresh_tick.tick() => {
                let tx = event_tx.clone();
                tokio::spawn(async move {
                    let res = tokio::task::spawn_blocking(|| {
                        let procs = crate::ui::state::fetch_process_list();
                        let cpu = crate::ui::state::sample_cpu_usage();
                        (procs, cpu)
                    }).await;
                    if let Ok((procs, cpu)) = res {
                        let _ = tx.send(AgentEvent::ProcessListUpdate(procs)).await;
                        let _ = tx.send(AgentEvent::CpuSampleUpdate(cpu)).await;
                    }
                });
            }

            // 30s 系统信息刷新（SSH 或本地）
            _ = system_refresh_tick.tick() => {
                let tx = event_tx.clone();
                if let Some(ref ssh) = ssh_config {
                    let ssh = ssh.clone();
                    tokio::spawn(async move {
                        let ctx = system_scan::scan_remote(&ssh).await;
                        let svcs = system_scan::get_remote_service_status(&ssh).await;
                        let _ = tx.send(AgentEvent::SystemUpdate(ctx)).await;
                        let _ = tx.send(AgentEvent::ServiceStatusUpdate(svcs)).await;
                    });
                } else {
                    tokio::spawn(async move {
                        let ctx = system_scan::scan().await;
                        let svcs = system_scan::get_service_status().await;
                        let _ = tx.send(AgentEvent::SystemUpdate(ctx)).await;
                        let _ = tx.send(AgentEvent::ServiceStatusUpdate(svcs)).await;
                    });
                }
            }

            // 键盘/终端事件
            Some(Ok(event)) = event_stream.next() => {
                let term_size = terminal.size().unwrap_or_default();
                match handle_event(&mut state, event, &input_tx, &event_tx, &mut recording, term_size).await {
                    EventResult::Quit  => break Ok(()),
                    EventResult::Continue => {}
                }
            }

            // Agent 事件
            Some(agent_event) = event_rx.recv() => {
                handle_agent_event(&mut state, agent_event);
            }

            else => break Ok(()),
        }
    };

    // ── 终端恢复 ─────────────────────────────────────────────────────────
    // 恢复 panic hook
    let _ = std::panic::take_hook();
    std::panic::set_hook(Box::new(|_| {}));

    disable_raw_mode()?;
    execute!(
        terminal.backend_mut(),
        LeaveAlternateScreen,
        DisableMouseCapture
    )?;
    terminal.show_cursor()?;

    result
}

enum EventResult {
    Continue,
    Quit,
}

/// 处理键盘/终端事件
async fn handle_event(
    state: &mut AppState,
    event: Event,
    input_tx: &mpsc::Sender<String>,
    event_tx: &mpsc::Sender<AgentEvent>,
    recording: &mut Option<RecordingState>,
    term_size: ratatui::layout::Size,
) -> EventResult {
    match event {
        Event::Key(key) => {
            // ── 弹窗模式 ─────────────────────────────────────────────────
            if state.modal.is_some() {
                match key.code {
                    KeyCode::Char('y') | KeyCode::Char('Y') => {
                        let is_critical = state.modal.as_ref()
                            .map(|m| matches!(m.risk_level, crate::types::risk::RiskLevel::Critical))
                            .unwrap_or(false);
                        if !is_critical {
                            state.close_modal(true);
                            state.push_line(ChatLine::ToolResultLine {
                                success: true,
                                preview: "用户已确认，继续执行".to_string(),
                                duration_ms: 0,
                            });
                        }
                    }
                    KeyCode::Char('n') | KeyCode::Char('N') | KeyCode::Esc => {
                        state.close_modal(false);
                        state.push_line(ChatLine::ToolResultLine {
                            success: false,
                            preview: "操作已取消".to_string(),
                            duration_ms: 0,
                        });
                    }
                    KeyCode::Tab => {
                        if let Some(m) = &mut state.modal {
                            if !matches!(m.risk_level, crate::types::risk::RiskLevel::Critical) {
                                m.selected_yes = !m.selected_yes;
                            }
                        }
                    }
                    KeyCode::Enter => {
                        let should_confirm = state.modal.as_ref().map(|m| {
                            m.selected_yes
                                && !matches!(m.risk_level, crate::types::risk::RiskLevel::Critical)
                        });
                        if let Some(confirmed) = should_confirm {
                            state.close_modal(confirmed);
                        }
                    }
                    _ => {}
                }
                return EventResult::Continue;
            }

            // ── 普通模式 ─────────────────────────────────────────────────
            match (key.modifiers, key.code) {
                // 退出
                (KeyModifiers::CONTROL, KeyCode::Char('c'))
                | (KeyModifiers::NONE, KeyCode::Esc) => {
                    return EventResult::Quit;
                }

                // 历史导航（readline 风格：Ctrl+P 上一条，Ctrl+N 下一条）
                (KeyModifiers::CONTROL, KeyCode::Char('p')) => {
                    if !state.is_thinking {
                        state.history_prev();
                    }
                }
                (KeyModifiers::CONTROL, KeyCode::Char('n')) => {
                    if !state.is_thinking {
                        state.history_next();
                    }
                }

                // Ctrl+Y：复制最后一条 Agent 回复到剪贴板
                (KeyModifiers::CONTROL, KeyCode::Char('y')) => {
                    if state.copy_last_reply_to_clipboard() {
                        state.copy_notice_frames = 40; // ~2.6s at 60fps
                    }
                }

                // Tab：循环切换标签页
                (KeyModifiers::NONE, KeyCode::Tab) => {
                    state.active_tab = state.active_tab.next();
                }

                // Ctrl+B：折叠/展开右侧面板
                (KeyModifiers::CONTROL, KeyCode::Char('b')) => {
                    state.side_collapsed = !state.side_collapsed;
                    state.mark_dirty();
                }

                // F5：切换语音输入（录音开始/停止并转写）
                (KeyModifiers::NONE, KeyCode::F(5)) => {
                    if state.is_thinking || state.modal.is_some() {
                        return EventResult::Continue;
                    }
                    if recording.is_none() {
                        // 开始录音
                        match VoiceEngine::start_recording() {
                            Ok((child, path)) => {
                                *recording = Some(RecordingState { child, wav_path: path });
                                state.voice_recording = true;
                                state.mark_dirty();
                            }
                            Err(e) => {
                                state.push_line(ChatLine::ErrorLine(format!("语音输入启动失败: {}", e)));
                            }
                        }
                    } else {
                        // 停止录音并转写
                        if let Some(rec_state) = recording.take() {
                            state.voice_recording = false;
                            state.mark_dirty();
                            let tx = event_tx.clone();
                            tokio::spawn(async move {
                                match VoiceEngine::stop_and_transcribe(rec_state.child, &rec_state.wav_path).await {
                                    Ok(text) if !text.is_empty() => {
                                        let _ = tx.send(AgentEvent::VoiceTranscription(text)).await;
                                    }
                                    Ok(_) => {
                                        let _ = tx.send(AgentEvent::Error("语音转写结果为空，请重试".to_string())).await;
                                    }
                                    Err(e) => {
                                        let _ = tx.send(AgentEvent::Error(format!("语音转写失败: {}", e))).await;
                                    }
                                }
                            });
                        }
                    }
                }

                // 发送输入
                (KeyModifiers::NONE, KeyCode::Enter) => {
                    if state.is_thinking {
                        return EventResult::Continue;
                    }
                    let text = state.take_input();
                    if text.is_empty() {
                        return EventResult::Continue;
                    }
                    if text.trim() == "/exit" {
                        return EventResult::Quit;
                    }
                    // /export：导出对话历史到文件（直接在 TUI 层处理，无需 agent）
                    if text.trim() == "/export" {
                        let result = state.export_to_file();
                        let msg = match result {
                            Ok(path) => format!("📄 对话已导出到：{}\n   可在文件管理器中打开并复制文本", path),
                            Err(e) => format!("❌ 导出失败：{}", e),
                        };
                        state.push_line(ChatLine::AgentMsg(msg));
                        return EventResult::Continue;
                    }
                    // 显示用户消息
                    state.push_line(ChatLine::UserMsg(text.clone()));
                    state.is_thinking = true;
                    // 发给 agent task
                    let _ = input_tx.send(text).await;
                }

                // 退格
                (KeyModifiers::NONE, KeyCode::Backspace) => {
                    state.delete_before_cursor();
                }

                // 光标移动
                (KeyModifiers::NONE, KeyCode::Left) => state.cursor_left(),
                (KeyModifiers::NONE, KeyCode::Right) => state.cursor_right(),

                // 滚动
                (KeyModifiers::NONE, KeyCode::PageUp) => state.scroll_up(5),
                (KeyModifiers::NONE, KeyCode::PageDown) => state.scroll_down(5),
                (KeyModifiers::NONE, KeyCode::Up) => state.scroll_up(1),
                (KeyModifiers::NONE, KeyCode::Down) => state.scroll_down(1),

                // 滚到底
                (KeyModifiers::NONE, KeyCode::End) => state.scroll_to_bottom(),

                // 普通字符输入
                (KeyModifiers::NONE | KeyModifiers::SHIFT, KeyCode::Char(c)) => {
                    if !state.is_thinking && !state.voice_recording {
                        state.insert_char(c);
                    }
                }

                _ => {}
            }
        }

        Event::Mouse(mouse_event) => {
            match mouse_event.kind {
                MouseEventKind::ScrollUp   => state.scroll_up(3),
                MouseEventKind::ScrollDown => state.scroll_down(3),
                MouseEventKind::Down(_) => {
                    let col = mouse_event.column;
                    let row = mouse_event.row;
                    let total_w: u16 = term_size.width;
                    // 布局复刻 renderer.rs 的垂直切分：
                    //   statusbar(1) → tabbar(2) → main(fill) → input(2)
                    // tabbar 占 row 1..=2，main 起始于 row 3
                    let tabbar_top: u16 = 1;
                    let tabbar_bot: u16 = 2;
                    let main_y: u16 = 3;

                    // ── ① 点击 TabBar：按三等分切换标签页 ─────────────
                    if row >= tabbar_top && row <= tabbar_bot {
                        let third = total_w / 3;
                        let new_tab = if col < third {
                            ActiveTab::Chat
                        } else if col < third * 2 {
                            ActiveTab::Monitor
                        } else {
                            ActiveTab::History
                        };
                        if state.active_tab != new_tab {
                            state.active_tab = new_tab;
                            state.scroll_to_bottom();
                            state.mark_dirty();
                        }
                    }
                    // ── ② 点击右侧面板的折叠/展开按钮 ─────────────────
                    else if state.side_collapsed {
                        // 折叠时：右侧 4 列竖条 → 整个区域可点击展开
                        let strip_x = total_w.saturating_sub(4);
                        if col >= strip_x && row >= main_y {
                            state.side_collapsed = false;
                            state.mark_dirty();
                        }
                    } else {
                        // 展开时：▷ 按钮位于 status 面板第一行最右一格（main_y）
                        let button_x = total_w.saturating_sub(1);
                        if col == button_x && row == main_y {
                            state.side_collapsed = true;
                            state.mark_dirty();
                        }
                    }
                }
                _ => {}
            }
        }

        Event::Resize(_, _) => { /* 自动重绘 */ }
        _ => {}
    }

    EventResult::Continue
}

/// 处理来自 agent 的事件，更新 AppState
fn handle_agent_event(state: &mut AppState, event: AgentEvent) {
    match event {
        AgentEvent::Thinking => {
            state.is_thinking = true;
        }

        AgentEvent::ToolCall { step, tool, args, dry_run } => {
            state.push_line(ChatLine::ToolCallLine { step, tool, args, dry_run });
        }

        AgentEvent::ToolResult { success, preview, duration_ms } => {
            // 先回扫匹配最新一次 ToolCall 拿 tool + args（避免和 &mut borrow 冲突）
            let (tool_name, args) = state.messages.iter().rev().find_map(|m| {
                if let ChatLine::ToolCallLine { tool, args, .. } = m {
                    Some((tool.clone(), args.clone()))
                } else {
                    None
                }
            }).unwrap_or_default();

            let args_summary = summarize_args(&args);
            let undoable = is_undoable(&tool_name, &args, success);
            state.push_op(tool_name, args_summary, success, duration_ms, undoable);
            state.push_line(ChatLine::ToolResultLine { success, preview, duration_ms });
        }

        AgentEvent::AgentReply(text) => {
            state.is_thinking = false;
            state.push_line(ChatLine::AgentMsg(text));
            state.push_line(ChatLine::Separator);
        }

        AgentEvent::Error(msg) => {
            state.is_thinking = false;
            state.push_line(ChatLine::ErrorLine(msg));
            state.push_line(ChatLine::Separator);
        }

        AgentEvent::RiskPrompt {
            tool, command_preview, risk_level, reason, impact, alternative, confirm_tx,
        } => {
            state.is_thinking = false;
            state.show_modal(tool, command_preview, risk_level, reason, impact, alternative, confirm_tx);
        }

        AgentEvent::SystemUpdate(ctx) => {
            state.system_ctx = Some(ctx);
        }

        AgentEvent::WatchdogAlert { severity, message } => {
            state.push_line(ChatLine::WatchdogAlert { severity, message });
        }

        AgentEvent::StepProgress { step, task_hint } => {
            state.task_step = step;
            state.task_hint = task_hint;
        }

        AgentEvent::VoiceTtsToggle(enabled) => {
            state.voice_tts_enabled = enabled;
        }

        AgentEvent::ServiceStatusUpdate(svcs) => {
            state.service_status = svcs;
            state.mark_dirty();
        }

        AgentEvent::VoiceTranscription(text) => {
            state.input = text;
            state.cursor_pos = state.input.chars().count();
            state.mark_dirty();
        }

        AgentEvent::ProcessListUpdate(procs) => {
            state.apply_process_list(procs);
        }

        AgentEvent::CpuSampleUpdate(cpu) => {
            state.apply_cpu_sample(cpu);
        }

    }
}

/// 把工具参数压成单行 ≤60 chars 摘要，给历史 Tab 显示。
fn summarize_args(args: &str) -> String {
    let flat: String = args.chars()
        .map(|c| if c == '\n' || c == '\r' || c == '\t' { ' ' } else { c })
        .collect();
    let trimmed = flat.trim();
    if trimmed.chars().count() <= 60 {
        trimmed.to_string()
    } else {
        let head: String = trimmed.chars().take(60).collect();
        format!("{}…", head)
    }
}

/// 简单 allow-list 判断这条工具调用是否产生了可撤销的副作用。
/// 不接 agent::memory::Memory.last_undoable() — 保持改动外科手术化。
fn is_undoable(tool: &str, args: &str, success: bool) -> bool {
    if !success {
        return false;
    }
    match tool {
        "file.write" | "service.stop" => true,
        "shell.exec" => {
            if args.contains("--dry-run") {
                return false;
            }
            !is_read_only_shell(args)
        }
        _ => false,
    }
}

/// 这些只读 shell 命令不留痕，不需要标记 ↺。
fn is_read_only_shell(args: &str) -> bool {
    const RO_PREFIXES: &[&str] = &[
        "df", "du", "ls", "cat", "ps", "top", "free", "uname",
        "whoami", "which", "lsof", "netstat", "grep", "head", "tail",
        "wc", "stat", "echo", "hostname", "id", "uptime", "date",
    ];
    let mut tokens = args.split_whitespace();
    let mut first = tokens.next().unwrap_or("");
    if first == "sudo" {
        first = tokens.next().unwrap_or("");
    }
    // strip absolute path: /usr/bin/df → df
    let first = first.rsplit('/').next().unwrap_or(first);
    RO_PREFIXES.iter().any(|p| first == *p)
}
