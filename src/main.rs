mod agent;
mod config;
mod context;
mod executor;
mod explainer;
mod image;
mod llm;
mod playbook;
mod safety;
mod tools;
mod types;
mod ui;
mod user_config;
mod voice;
mod watchdog;

use anyhow::{Result, anyhow};
use clap::{Parser, Subcommand};
use tracing::info;

use agent::planner::{DisambiguationOption, Planner, TaskPlan};
use agent::r#loop::AgentLoop;
use config::{LlmConfig, get_provider_presets};
use executor::ssh::SshConfig;
use playbook::{PlaybookManager, PlaybookSource};
use safety::audit::should_persist_input;
use user_config::{delete_config, interactive_config, show_current_config};

#[derive(Parser)]
#[command(
    name = "jij",
    about = "jij — 自然语言操作系统管理代理",
    version = "0.2.0",
    long_about = "
用自然语言管理你的 Linux/macOS/Windows 系统。

━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━
快速开始（无需配置，直接设置环境变量）：
  export ANTHROPIC_API_KEY=sk-ant-xxx   # Anthropic Claude
  export OPENAI_API_KEY=sk-xxx          # OpenAI GPT

  jij chat                  # 交互式对话（推荐）
  jij run \"查看磁盘\"         # 单条指令
  jij config --setup        # 交互式配置（可选）

支持多种 LLM Provider：
  --provider anthropic    # Claude（原生 tool_use）
  --provider openai       # GPT-4o
  --provider openrouter   # 多模型聚合
  --provider groq         # 超快推理
  --provider deepseek     # DeepSeek

环境变量（无需 config 文件，直接设置即可使用）：
  ANTHROPIC_API_KEY          # Anthropic 官方（自动检测）
  OPENAI_API_KEY             # OpenAI 官方（自动检测）
  AGENT_UNIX_LLM_API_KEY     # 通用 API Key（最高优先级）
━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━
"
)]
struct Cli {
    #[command(subcommand)]
    command: Commands,

    /// 详细日志输出
    #[arg(long, global = true)]
    verbose: bool,

    /// 运行模式：safe | normal | auto
    #[arg(long, global = true, default_value = "normal")]
    mode: String,

    /// LLM Provider 预设（anthropic/openai/openrouter/groq/deepseek/custom）
    #[arg(long, global = true)]
    provider: Option<String>,

    /// LLM 模型 ID
    #[arg(long, global = true)]
    model: Option<String>,

    /// LLM API Base URL（自定义端点）
    #[arg(long, global = true)]
    base_url: Option<String>,

    /// API Key（建议使用环境变量，CLI 传参不安全）
    #[arg(long, global = true, hide = true)]
    api_key: Option<String>,

    /// 禁用 TUI，使用传统 CLI 模式（用于无交互终端场景）
    #[arg(long, global = true)]
    no_tui: bool,

    /// SSH 远程模式：连接到远程服务器（格式: user@host 或 user@host:port）
    /// 示例: --ssh root@192.168.1.100 或 --ssh admin@server.example.com:2222
    #[arg(long, global = true)]
    ssh: Option<String>,

    /// SSH 身份文件路径（与 --ssh 一起使用）
    #[arg(long, global = true)]
    ssh_key: Option<String>,
}

#[derive(Subcommand)]
enum Commands {
    /// 进入交互式对话模式（默认 TUI，--no-tui 切换为 CLI）
    Chat,

    /// 执行单条自然语言指令
    Run {
        /// 自然语言指令
        instruction: String,
        /// Dry-Run 模式：仅预览，不实际执行
        #[arg(long)]
        dry_run: bool,
    },

    /// 查看/配置 LLM Provider
    Config {
        /// 启动交互式配置向导
        #[arg(long)]
        setup: bool,

        /// 预选某个 provider（与 --setup 一起使用）
        #[arg(long)]
        provider: Option<String>,

        /// 显示当前配置
        #[arg(long)]
        show: bool,

        /// 列出所有支持的 Provider
        #[arg(long)]
        list: bool,

        /// 删除配置文件
        #[arg(long)]
        delete: bool,
    },

    /// 查看操作历史
    History,

    /// 撤销上一次操作
    Undo,

    /// 列出保存的 Playbook
    Playbooks,

    /// 反向解释模式：读取并解释配置文件
    Explain {
        /// 要解释的文件路径（不指定则列出支持的文件）
        file: Option<String>,
    },

    /// 启动后台监控系统
    Watch {
        /// 监控持续时间（秒）
        #[arg(long, default_value = "60")]
        duration: u64,
    },

    /// 显示版本和系统信息
    Info,
}

#[derive(Debug, Clone, PartialEq, Eq)]
enum PreparedRunInstruction {
    Execute {
        display_instruction: String,
        actual_instruction: String,
    },
    Clarify {
        options: Vec<DisambiguationOption>,
    },
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct PlaybookSaveRequest<'a> {
    name: &'a str,
    step_count: usize,
}

#[derive(Debug, Clone, PartialEq, Eq)]
enum PlaybookSaveParseError {
    InvalidUsage,
    InvalidName,
    InvalidStepCount,
    ZeroStepCount,
}

#[derive(Debug, Clone, PartialEq, Eq)]
enum PlaybookCommand<'a> {
    Save(PlaybookSaveRequest<'a>),
    List { keyword: Option<&'a str> },
    Run { name: &'a str },
    Help,
}

#[derive(Debug, Clone, PartialEq, Eq)]
enum ChatInputAction {
    Skip,
    Execute { actual_input: String, plan_steps: Vec<String> },
    Clarify { options: Vec<DisambiguationOption> },
}

#[tokio::main]
async fn main() -> Result<()> {
    let cli = Cli::parse();

    // 初始化日志
    let log_level = if cli.verbose { "debug" } else { "info" };
    tracing_subscriber::fmt()
        .with_env_filter(format!("agent_unix={}", log_level))
        .init();

    info!("jij 启动，模式: {}", cli.mode);

    match cli.command {
        // ═══════════════════════════════════════════════════════
        // 不需要 LLM 的命令
        // ═══════════════════════════════════════════════════════
        Commands::Config { setup, provider, show, list, delete } => {
            if delete {
                delete_config()?;
                return Ok(());
            }

            if list {
                println!("📡 支持的 LLM Provider：");
                println!();

                for p in get_provider_presets() {
                    let default_mark = if p.name == "anthropic" { "（默认）" } else { "" };
                    println!("   • {} / {} {} — {}", p.name, p.display_name, default_mark, p.description);
                    println!(
                        "     Base URL: {}",
                        if p.base_url.is_empty() { "(需在配置时填写)" } else { &p.base_url }
                    );
                    println!(
                        "     默认模型: {}",
                        if p.default_model.is_empty() { "(需在配置时填写)" } else { &p.default_model }
                    );
                    println!("     环境变量: {}", p.suggested_env_keys.join(", "));
                    if !p.aliases.is_empty() {
                        println!("     搜索别名: {}", p.aliases.join(", "));
                    }
                    println!();
                }

                println!("💡 配置方式：");
                println!("   jij config --setup                # 交互式配置向导（推荐）");
                println!("   jij config --setup --provider glm # 预选 provider");
                println!("   jij config --show                 # 查看当前配置");
                return Ok(());
            }

            if setup {
                interactive_config(provider.as_deref())?;
                return Ok(());
            }

            if show {
                show_current_config();
                return Ok(());
            }

            println!("💡 Config 命令用法：");
            println!();
            println!("   jij config --setup                # 启动交互式配置向导（推荐）");
            println!("   jij config --setup --provider glm # 预选 provider");
            println!("   jij config --show                 # 显示当前配置");
            println!("   jij config --list                 # 列出支持的 Provider");
            println!("   jij config --delete               # 删除配置文件");
        }

        Commands::Playbooks => {
            let mut manager = PlaybookManager::new();
            manager.initialize(None)?;
            println!("{}", format_playbook_listing(&manager, None));
        }

        Commands::History => {
            show_history().await;
        }

        Commands::Undo => {
            println!("⚠️  Undo 需要在 chat 模式下执行（输入 '/undo' 命令）");
        }

        Commands::Explain { file: None } => {
            use explainer::Explainer;
            let explainer = Explainer::new();
            println!("📖 反向解释模式 — 支持的文件类型：");
            println!();
            for (desc, _path) in explainer.list_supported_files() {
                println!("   • {}", desc);
            }
            println!();
            println!("💡 使用方式：jij explain <文件路径>");
            println!("   示例：jij explain /etc/nginx/nginx.conf");
        }

        Commands::Watch { duration } => {
            use watchdog::create_watchdog_system;

            println!("🔍 启动 Watchdog 监控系统");
            println!("   监控规则：磁盘使用率（>80%/95%）、内存使用率（>85%/95%）");
            println!("   监控时长：{} 秒", duration);
            println!();

            let (watchdog, mut handler) = create_watchdog_system();

            watchdog.start();

            let handler_task = tokio::spawn(async move {
                handler.run().await;
            });

            tokio::time::sleep(std::time::Duration::from_secs(duration)).await;

            watchdog.stop();
            // Drop watchdog (and its alert_tx) so handler_task can see channel closed
            drop(watchdog);

            handler_task.await?;

            println!("\n✅ 监控已完成");
        }

        Commands::Info => {
            println!("🤖 jij v0.2.0");
            println!("   AI Hackathon 2026 · 超聚变 αFUSION 预赛");
            println!();
            println!("⏳ 正在采集系统信息…");
            let ctx = context::system_scan::scan().await;
            println!();
            println!("【系统环境】");
            println!("   操作系统：{}", ctx.os_info);
            println!("   主机名：{}", ctx.hostname);
            println!("   CPU：{}", ctx.cpu_info);
            println!("   内存：{}", ctx.memory_info);
            println!("   磁盘：{}", ctx.disk_info);
            println!("   包管理器：{}", ctx.package_manager);
            if !ctx.running_services.is_empty() {
                println!("   活跃服务：{}", ctx.running_services.join(", "));
            }
            println!();
            println!("【快速开始】");
            println!("   jij chat              # 交互式 TUI 对话");
            println!("   jij chat --no-tui     # CLI 对话模式");
            println!("   jij run \"查看磁盘\"    # 单条指令");
            println!("   jij config --setup    # 配置 LLM Provider");
        }

        // ═══════════════════════════════════════════════════════
        // 需要 LLM 的命令
        // ═══════════════════════════════════════════════════════
        _ => {
            // 启动提示：显示自动检测到的 provider
            if cli.provider.is_none() {
                if let Some(detected) = config::detect_provider_from_env() {
                    eprintln!("✅ 自动检测到 {} 配置，无需额外设置", detected);
                }
            }

            let llm_config = LlmConfig::load(
                cli.provider.as_deref(),
                cli.model.as_deref(),
                cli.base_url.as_deref(),
                cli.api_key.as_deref(),
            ).unwrap_or_else(|e| {
                eprintln!();
                eprintln!("⚠️  LLM 配置加载失败");
                eprintln!();
                eprintln!("   {}", e);
                eprintln!();
                std::process::exit(1);
            });

            info!("LLM Provider: {} @ {} (model: {})",
                llm_config.provider_kind,
                llm_config.base_url,
                llm_config.model
            );

            // 构建 SSH 配置（如果指定）
            let ssh_config = cli.ssh.as_ref().map(|target| {
                let mut cfg = SshConfig::new(target);
                if let Some(ref key) = cli.ssh_key {
                    cfg.identity_file = Some(key.clone());
                }
                cfg
            });

            // 远程模式提示
            if let Some(ref ssh) = ssh_config {
                println!("🔗 SSH 远程模式：{}", ssh.display());
                println!("⏳ 测试 SSH 连接…");
                match ssh.test_connection().await {
                    Ok(msg) => println!("{}", msg),
                    Err(e) => {
                        eprintln!("❌ SSH 连接失败: {}", e);
                        eprintln!("💡 请检查：");
                        eprintln!("   1. 目标服务器是否可达");
                        eprintln!("   2. SSH Key 是否已配置（~/.ssh/id_rsa 或 --ssh-key 指定）");
                        eprintln!("   3. 用户名是否正确（格式: user@host）");
                        std::process::exit(1);
                    }
                }
                println!();
            }

            match cli.command {
                Commands::Chat => {
                    let ctx = context::system_scan::scan().await;

                    if cli.no_tui {
                        // ── CLI fallback 模式 ─────────────────────────────
                        let mode_label = if ssh_config.is_some() { "SSH 远程模式" } else { "CLI 本地模式" };
                        println!("🤖 jij v0.2.0  [{}]", mode_label);
                        println!("   Provider: {} @ {}", llm_config.provider_kind, llm_config.base_url);
                        println!();
                        println!("【系统环境】");
                        println!("   OS：{}  主机：{}", ctx.os_info, ctx.hostname);
                        println!("   CPU：{}  内存：{}", ctx.cpu_info, ctx.memory_info);
                        println!("   磁盘：{}  包管理器：{}", ctx.disk_info, ctx.package_manager);
                        println!("   网络：{}", ctx.network_info);
                        println!();
                        println!("   输入 '/help' 查看命令，'/exit' 退出\n");
                        run_chat_loop(&llm_config, &cli.mode, ctx, ssh_config).await?;
                    } else {
                        // ── TUI 模式（默认）──────────────────────────────
                        ui::run_tui(llm_config, cli.mode.clone(), ctx, ssh_config).await?;
                    }
                }
                Commands::Run { instruction, dry_run } => {
                    let ctx = context::system_scan::scan().await;
                    run_single(&llm_config, &cli.mode, ctx, &instruction, dry_run, ssh_config).await?;
                }
                Commands::Explain { file: Some(path) } => {
                    run_explain(&llm_config, &path).await?;
                }
                Commands::History | Commands::Undo | Commands::Playbooks
                | Commands::Explain { .. } | Commands::Watch { .. }
                | Commands::Config { .. } | Commands::Info => {}
            }
        }
    }

    Ok(())
}

async fn run_chat_loop(
    llm_config: &LlmConfig,
    mode: &str,
    ctx: crate::agent::memory::SystemContext,
    ssh_config: Option<SshConfig>,
) -> Result<()> {
    use rustyline::DefaultEditor;
    use rustyline::error::ReadlineError;

    use image::{ImageProcessor, Iterm2Detector};

    let mut rl = DefaultEditor::new()?;
    let home = std::env::var("HOME")
        .or_else(|_| std::env::var("USERPROFILE"))
        .unwrap_or_else(|_| {
            if cfg!(windows) { "C:\\Temp".to_string() } else { "/tmp".to_string() }
        });
    let history_path = format!("{}/.jij/history.txt", home);
    if let Some(parent) = std::path::Path::new(&history_path).parent() {
        if let Err(err) = std::fs::create_dir_all(parent) {
            tracing::warn!("创建历史记录目录失败: {}", err);
        }
    }
    match rl.load_history(&history_path) {
        Ok(()) => {}
        Err(ReadlineError::Io(err)) if err.kind() == std::io::ErrorKind::NotFound => {}
        Err(err) => tracing::warn!("加载历史记录失败: {}", err),
    }

    // 保留副本，供 /clear 命令重建 agent
    let saved_ssh = ssh_config.clone();
    let mut agent = match ssh_config {
        Some(ssh) => AgentLoop::new_with_ssh(llm_config.clone(), mode, ctx, ssh),
        None => AgentLoop::new(llm_config.clone(), mode, ctx),
    };
    let image_processor = ImageProcessor::new();
    let iterm2_detector = Iterm2Detector::new();

    loop {
        let readline = rl.readline("👤 你：");
        match readline {
            Ok(line) => {
                let input = line.trim();
                if input.is_empty() {
                    continue;
                }
                if input == "/exit" {
                    println!("👋 再见！");
                    break;
                }
                if input == "/help" {
                    print_help_message();
                    continue;
                }
                if input == "/status" {
                    println!("\n⏳ 采集系统状态…");
                    let new_ctx = context::system_scan::scan().await;
                    println!("\n📊 系统状态速览");
                    println!("   OS：{}", new_ctx.os_info);
                    println!("   主机：{}", new_ctx.hostname);
                    println!("   CPU：{}", new_ctx.cpu_info);
                    println!("   内存：{}", new_ctx.memory_info);
                    println!("   磁盘：{}", new_ctx.disk_info);
                    println!("   包管理器：{}", new_ctx.package_manager);
                    if !new_ctx.running_services.is_empty() {
                        println!("   活跃服务：{}", new_ctx.running_services.join(", "));
                    }
                    // 主动异常检测
                    let anomalies = context::system_scan::detect_anomalies(&new_ctx);
                    if !anomalies.is_empty() {
                        println!("\n⚠️  检测到以下异常：");
                        for anomaly in &anomalies {
                            println!("   {}", anomaly);
                        }
                    }
                    println!();
                    // 同步更新 agent 的系统上下文
                    agent.memory.refresh_system_context(new_ctx);
                    continue;
                }
                if input == "/history" {
                    show_history_inline(&agent);
                    continue;
                }
                if input == "/clear" {
                    let new_ctx = context::system_scan::scan().await;
                    agent = match saved_ssh.as_ref() {
                        Some(ssh) => AgentLoop::new_with_ssh(llm_config.clone(), mode, new_ctx, ssh.clone()),
                        None => AgentLoop::new(llm_config.clone(), mode, new_ctx),
                    };
                    println!("\n🗑️  对话上下文已清除，重新开始\n");
                    continue;
                }
                if input == "/undo" {
                    handle_undo_inline(&mut agent).await;
                    continue;
                }
                if input.starts_with("/playbook") {
                    handle_playbook_inline(&mut agent, input, llm_config).await?;
                    continue;
                }
                if input == "/report" {
                    handle_report_inline().await;
                    continue;
                }

                let prepared_input = image_processor.prepare_user_input(&iterm2_detector, input);
                let clean_input = prepared_input.clean_input;
                let images = prepared_input.images;

                for notice in prepared_input.notices {
                    println!("{}", notice);
                    println!();
                }

                if !clean_input.is_empty() {
                    if should_persist_input(&clean_input) {
                        rl.add_history_entry(&clean_input)?;
                    } else {
                        rl.add_history_entry("[redacted sensitive input]")?;
                    }
                }

                let final_input = if clean_input.is_empty() && !images.is_empty() {
                    "请分析这张图片"
                } else {
                    &clean_input
                };

                match prepare_chat_input_action(llm_config, &agent.memory.system_context, final_input).await {
                    Ok(ChatInputAction::Skip) => continue,
                    Ok(ChatInputAction::Clarify { options }) => {
                        // 交互式消歧：让用户选择 A/B/C 或直接描述
                        let chosen = prompt_disambiguation(&mut rl, &options)?;
                        if let Some(description) = chosen {
                            match agent.run_with_images(&description, &[], false).await {
                                Ok(response) => println!("\n🤖 Agent：{}\n", response),
                                Err(e) => println!("\n❌ 错误: {}\n", e),
                            }
                        }
                        continue;
                    }
                    Ok(ChatInputAction::Execute { actual_input, plan_steps }) => {
                        // 显示多步执行计划
                        if !plan_steps.is_empty() {
                            println!("\n📋 执行计划（共 {} 步）：", plan_steps.len());
                            for (i, step) in plan_steps.iter().enumerate() {
                                println!("   {}. {}", i + 1, step);
                            }
                            println!();
                        }
                        match agent.run_with_images(&actual_input, &images, false).await {
                            Ok(response) => println!("\n🤖 Agent：{}\n", response),
                            Err(e) => println!("\n❌ 错误: {}\n", e),
                        }
                    }
                    Err(err) => {
                        tracing::warn!("Planner 分析失败，回退到直接执行: {}", err);
                        match agent.run_with_images(final_input, &images, false).await {
                            Ok(response) => println!("\n🤖 Agent：{}\n", response),
                            Err(e) => println!("\n❌ 错误: {}\n", e),
                        }
                    }
                }
            }
            Err(ReadlineError::Interrupted) => {
                println!("\n⚠️  已中断，输入 '/exit' 退出");
            }
            Err(ReadlineError::Eof) => {
                println!("\n👋 再见！");
                break;
            }
            Err(e) => {
                println!("\n❌ 输入错误: {}", e);
                break;
            }
        }
    }

    if let Err(err) = rl.save_history(&history_path) {
        tracing::warn!("保存历史记录失败: {}", err);
    }
    Ok(())
}

async fn run_single(
    llm_config: &LlmConfig,
    mode: &str,
    ctx: crate::agent::memory::SystemContext,
    instruction: &str,
    dry_run: bool,
    ssh_config: Option<SshConfig>,
) -> Result<()> {
    let planner = Planner::new(llm_config.clone());
    let planner_context = format_planner_context(&ctx);
    let prepared = match planner.analyze(instruction, &planner_context).await {
        Ok(plan) => prepare_run_instruction(instruction, plan),
        Err(err) => {
            tracing::warn!("Planner 分析失败，回退到直接执行: {}", err);
            PreparedRunInstruction::Execute {
                display_instruction: instruction.to_string(),
                actual_instruction: instruction.to_string(),
            }
        }
    };

    match prepared {
        PreparedRunInstruction::Execute {
            display_instruction,
            actual_instruction,
        } => {
            let mut agent = match ssh_config {
                Some(ssh) => AgentLoop::new_with_ssh(llm_config.clone(), mode, ctx, ssh),
                None => AgentLoop::new(llm_config.clone(), mode, ctx),
            };

            println!("🤖 执行: {}", display_instruction);
            if dry_run {
                println!("   [DRY-RUN 模式：仅预览]\n");
            }

            let result = agent.run(&actual_instruction, dry_run).await?;
            println!("\n{}", result);
        }
        PreparedRunInstruction::Clarify { options } => {
            print_ambiguous_options(&options);
        }
    }

    Ok(())
}

async fn run_explain(llm_config: &LlmConfig, path: &str) -> Result<()> {
    use explainer::Explainer;
    use llm::client::LlmClient;
    use serde_json::json;

    let explainer = Explainer::new();

    println!("📖 正在读取文件: {}", path);
    let content = match explainer.read_file(path) {
        Ok(c) => c,
        Err(e) => {
            println!("❌ 读取失败: {}", e);
            return Ok(());
        }
    };

    println!("   文件大小: {} 字节", content.len());

    let file_type = explainer.detect_type(path)
        .unwrap_or_else(|| "配置文件".to_string());

    println!("🤖 正在分析 {}，请稍候…\n", file_type);

    let llm = LlmClient::new(llm_config.clone());
    let prompt = explainer.build_explanation_prompt(&file_type, &content);
    let messages = vec![json!({ "role": "user", "content": prompt })];

    let system = "你是一个专业的 Linux 系统运维顾问，擅长解释配置文件和日志。\
                  请用清晰的中文解释文件内容，指出重要配置和潜在问题。";

    match llm.chat(system, &messages, &[]).await {
        Ok(crate::llm::client::LlmResponse::FinalAnswer(explanation)) => {
            println!("{}", explanation);
        }
        Ok(_) => {
            println!("⚠️  LLM 未返回文本回复");
        }
        Err(e) => {
            println!("❌ LLM 调用失败: {}", e);
        }
    }

    Ok(())
}

fn prepare_run_instruction(instruction: &str, plan: TaskPlan) -> PreparedRunInstruction {
    match plan {
        TaskPlan::Single { description } => PreparedRunInstruction::Execute {
            display_instruction: instruction.to_string(),
            actual_instruction: description,
        },
        TaskPlan::Multi {
            description,
            estimated_steps,
        } => PreparedRunInstruction::Execute {
            display_instruction: instruction.to_string(),
            actual_instruction: format_multi_step_instruction(&description, &estimated_steps),
        },
        TaskPlan::Ambiguous { options } => PreparedRunInstruction::Clarify { options },
    }
}

fn print_ambiguous_options(options: &[DisambiguationOption]) {
    println!("🤔  任务描述有歧义，请明确目标：\n");
    for option in options {
        println!("   {}. {}", option.label, option.description);
        if !option.preview.is_empty() {
            println!("      预览：{}", option.preview);
        }
    }
    println!("\n   输入字母选择（如 A），或直接重新描述需求：");
}

/// 展示消歧选项并等待用户选择，返回选定的任务描述（None 表示用户跳过）
fn prompt_disambiguation(
    rl: &mut rustyline::DefaultEditor,
    options: &[DisambiguationOption],
) -> Result<Option<String>> {
    print_ambiguous_options(options);

    match rl.readline("   选择 › ") {
        Ok(choice) => {
            let choice = choice.trim().to_uppercase();
            if choice.is_empty() {
                return Ok(None);
            }
            // 尝试匹配字母标签
            if let Some(opt) = options.iter().find(|o| o.label.to_uppercase() == choice) {
                println!("   ✓ 已选择：{}", opt.description);
                return Ok(Some(opt.description.clone()));
            }
            // 否则把用户输入作为新的任务描述
            if !choice.is_empty() {
                return Ok(Some(choice));
            }
            Ok(None)
        }
        Err(_) => Ok(None),
    }
}

fn format_planner_context(ctx: &crate::agent::memory::SystemContext) -> String {
    format!(
        "操作系统: {}\n主机名: {}\nCPU: {}\n内存: {}\n磁盘: {}\n活跃服务: {}\n包管理器: {}\n网络: {}",
        ctx.os_info,
        ctx.hostname,
        ctx.cpu_info,
        ctx.memory_info,
        ctx.disk_info,
        ctx.running_services.join(", "),
        ctx.package_manager,
        ctx.network_info,
    )
}

fn format_multi_step_instruction(description: &str, estimated_steps: &[String]) -> String {
    if estimated_steps.is_empty() {
        return description.to_string();
    }

    let steps = estimated_steps
        .iter()
        .enumerate()
        .map(|(index, step)| format!("{}. {}", index + 1, step))
        .collect::<Vec<_>>()
        .join("\n");

    format!(
        "任务目标：{}\n\n请按以下步骤逐步执行，并在每一步根据工具结果再决定下一步：\n{}",
        description, steps
    )
}

async fn prepare_chat_input_action(
    llm_config: &LlmConfig,
    system_context: &Option<crate::agent::memory::SystemContext>,
    input: &str,
) -> Result<ChatInputAction> {
    if input.trim().is_empty() {
        return Ok(ChatInputAction::Skip);
    }

    let Some(ctx) = system_context.as_ref() else {
        return Ok(ChatInputAction::Execute {
            actual_input: input.to_string(),
            plan_steps: vec![],
        });
    };

    let planner = Planner::new(llm_config.clone());
    let planner_context = format_planner_context(ctx);
    let plan = planner.analyze(input, &planner_context).await?;

    // Extract plan steps before consuming the plan
    let steps = match &plan {
        TaskPlan::Multi { estimated_steps, .. } => estimated_steps.clone(),
        _ => vec![],
    };

    Ok(match prepare_run_instruction(input, plan) {
        PreparedRunInstruction::Execute { actual_instruction, .. } => ChatInputAction::Execute {
            actual_input: actual_instruction,
            plan_steps: steps,
        },
        PreparedRunInstruction::Clarify { options } => ChatInputAction::Clarify { options },
    })
}

fn format_playbook_listing(manager: &PlaybookManager, keyword: Option<&str>) -> String {
    let matches = match keyword {
        Some(value) if !value.trim().is_empty() => manager.search(value.trim()),
        _ => manager.list(),
    };

    let mut lines = vec!["📋 已加载的 Playbook：".to_string(), String::new()];
    let stats = manager.stats();
    lines.push(format!(
        "   来源统计：内置 {}，用户 {}，项目 {}，覆盖 {}",
        stats.bundled_count, stats.user_count, stats.project_count, stats.overridden_count
    ));
    lines.push(String::new());

    if matches.is_empty() {
        if let Some(value) = keyword.filter(|value| !value.trim().is_empty()) {
            lines.push(format!("   （未找到包含 '{}' 的 Playbook）", value.trim()));
        } else {
            lines.push("   （暂无 Playbook）".to_string());
        }
    } else {
        let mut items = matches;
        items.sort_by(|a, b| a.name.cmp(&b.name));
        for pb in items {
            lines.push(format!("   • {} — {}", pb.name, pb.description));
            lines.push(format!("     步骤数：{}，运行次数：{}", pb.steps.len(), pb.run_count));
        }
    }

    lines.push(String::new());
    lines.push("💡 在 chat 模式下可用：/playbook list [关键词]、/playbook save <名称> [步数]".to_string());
    lines.join("\n")
}

fn parse_playbook_command(input: &str) -> std::result::Result<PlaybookCommand<'_>, PlaybookSaveParseError> {
    let parts: Vec<&str> = input.split_whitespace().collect();
    match parts.as_slice() {
        ["/playbook"] => Ok(PlaybookCommand::Help),
        ["/playbook", "list"] => Ok(PlaybookCommand::List { keyword: None }),
        ["/playbook", "list", keyword, ..] => Ok(PlaybookCommand::List {
            keyword: Some(keyword),
        }),
        ["/playbook", "run", name] => Ok(PlaybookCommand::Run { name }),
        _ => parse_playbook_save_request(input).map(PlaybookCommand::Save),
    }
}

async fn handle_playbook_inline(agent: &mut AgentLoop, input: &str, llm_config: &LlmConfig) -> Result<()> {
    match parse_playbook_command(input).map_err(|err| anyhow!(playbook_parse_error_message(err)))? {
        PlaybookCommand::Save(request) => {
            let available_steps = agent.memory.operations.len();
            if available_steps == 0 {
                println!("\n⚠️  当前还没有可保存的操作记录。\n");
                return Ok(());
            }

            let actual_steps = request.step_count.min(available_steps);
            let description = format!("从最近 {} 步操作保存", actual_steps);
            let playbook = agent
                .memory
                .save_as_playbook(request.name, &description, actual_steps);

            let mut manager = PlaybookManager::new();
            manager.initialize(None)?;
            let path = manager.save(&playbook, PlaybookSource::User)?;

            println!(
                "\n💾 已保存 Playbook '{}'（{} 步）\n   路径：{}\n",
                playbook.name,
                playbook.steps.len(),
                path.display()
            );
        }
        PlaybookCommand::Run { name } => {
            let mut manager = PlaybookManager::new();
            manager.initialize(None)?;
            match manager.get(name) {
                None => {
                    println!("\n⚠️  未找到 Playbook '{}'，使用 /playbook list 查看可用列表。\n", name);
                }
                Some(pb) => {
                    let step_count = pb.steps.len();
                    let description = pb.description.clone();
                    println!("\n▶️  执行 Playbook '{}'（{} 步）：{}\n", name, step_count, description);
                    let instruction = format!(
                        "请按 Playbook '{}' 执行以下 {} 个步骤，并在每步完成后汇报结果：\n{}",
                        name,
                        step_count,
                        pb.steps.iter().enumerate()
                            .map(|(i, s)| format!("{}. [{}] {}", i + 1, s.tool, s.args))
                            .collect::<Vec<_>>()
                            .join("\n")
                    );
                    match agent.run(&instruction, false).await {
                        Ok(response) => println!("\n🤖 Agent：{}\n", response),
                        Err(e) => println!("\n❌ Playbook 执行失败: {}\n", e),
                    }
                    let _ = llm_config; // suppress unused warning
                }
            }
        }
        PlaybookCommand::List { keyword } => {
            let mut manager = PlaybookManager::new();
            manager.initialize(None)?;
            println!("\n{}\n", format_playbook_listing(&manager, keyword));
        }
        PlaybookCommand::Help => {
            println!("\n💡 用法：/playbook list [关键词] | /playbook save <名称> [步数] | /playbook run <名称>\n");
        }
    }

    Ok(())
}

fn parse_playbook_save_request(input: &str) -> std::result::Result<PlaybookSaveRequest<'_>, PlaybookSaveParseError> {
    let parts: Vec<&str> = input.split_whitespace().collect();
    if parts.len() < 3 || parts[0] != "/playbook" || parts[1] != "save" {
        return Err(PlaybookSaveParseError::InvalidUsage);
    }

    let name = parts[2];
    if !is_valid_playbook_name(name) {
        return Err(PlaybookSaveParseError::InvalidName);
    }

    let step_count = match parts.get(3) {
        Some(value) => value
            .parse::<usize>()
            .map_err(|_| PlaybookSaveParseError::InvalidStepCount)?,
        None => 3,
    };

    if step_count == 0 {
        return Err(PlaybookSaveParseError::ZeroStepCount);
    }

    Ok(PlaybookSaveRequest { name, step_count })
}

fn playbook_parse_error_message(err: PlaybookSaveParseError) -> &'static str {
    match err {
        PlaybookSaveParseError::InvalidUsage => {
            "\n💡 用法：/playbook list [关键词] | /playbook save <名称> [步数]\n"
        }
        PlaybookSaveParseError::InvalidName => {
            "\n⚠️  Playbook 名称只允许字母、数字、点、下划线、连字符。\n"
        }
        PlaybookSaveParseError::InvalidStepCount => "步数必须是正整数",
        PlaybookSaveParseError::ZeroStepCount => "\n⚠️  步数必须大于 0。\n",
    }
}

fn is_valid_playbook_name(name: &str) -> bool {
    !name.is_empty()
        && name
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || matches!(c, '.' | '_' | '-'))
}

fn print_help_message() {
    println!("\n📖 jij 帮助");
    println!();
    println!("【命令列表】");
    println!("   /help    显示此帮助");
    println!("   /status  实时采集并显示系统状态");
    println!("   /history 查看操作历史");
    println!("   /undo    撤销上一步操作");
    println!("   /clear   清除对话上下文，重新开始");
    println!("   /report  生成系统健康综合报告");
    println!("   /playbook list          列出 Playbook");
    println!("   /playbook save <名称>   保存最近操作为 Playbook");
    println!("   /playbook run <名称>    执行 Playbook");
    println!("   /exit    退出");
    println!();
    println!("【示例指令】");
    println!("   · 查看磁盘使用情况");
    println!("   · 列出内存占用最高的 5 个进程");
    println!("   · 创建用户 testuser");
    println!("   · 安装并启动 nginx");
    println!("   · 把 nginx 配置改到 8080 端口并重启");
    println!();
    println!("【安全机制】");
    println!("   · 高危操作（CRITICAL）直接拒绝，无法确认");
    println!("   · 高风险操作（HIGH）需要您输入 yes 确认");
    println!("   · 可用 --mode safe 对中风险操作也要求确认");
    println!();
}

async fn handle_report_inline() {
    use chrono::Local;
    println!("\n⏳ 生成系统健康报告…");
    let ctx = context::system_scan::scan().await;
    let anomalies = context::system_scan::detect_anomalies(&ctx);
    let ts = Local::now().format("%Y-%m-%d %H:%M:%S");

    println!("\n╔══════════════════════════════════════════════════════╗");
    println!("║        系统健康综合报告  {}        ║", ts);
    println!("╠══════════════════════════════════════════════════════╣");
    println!("║  基础信息                                            ║");
    println!("╟──────────────────────────────────────────────────────╢");
    println!("  主机名：{}", ctx.hostname);
    println!("  操作系统：{}", ctx.os_info);
    println!("  包管理器：{}", ctx.package_manager);
    println!("╟──────────────────────────────────────────────────────╢");
    println!("║  资源使用                                            ║");
    println!("╟──────────────────────────────────────────────────────╢");
    println!("  CPU：{}", ctx.cpu_info);
    println!("  内存：{}", ctx.memory_info);
    println!("  磁盘：{}", ctx.disk_info);
    println!("╟──────────────────────────────────────────────────────╢");
    println!("║  网络与服务                                          ║");
    println!("╟──────────────────────────────────────────────────────╢");
    println!("  网络：{}", ctx.network_info);
    if !ctx.running_services.is_empty() {
        println!("  活跃服务：{}", ctx.running_services.join(", "));
    } else {
        println!("  活跃服务：（未检测到用户服务）");
    }
    println!("╟──────────────────────────────────────────────────────╢");
    if anomalies.is_empty() {
        println!("║  异常检测                                            ║");
        println!("╟──────────────────────────────────────────────────────╢");
        println!("  ✅ 未检测到系统异常，系统运行正常");
    } else {
        println!("║  ⚠️  异常告警                                        ║");
        println!("╟──────────────────────────────────────────────────────╢");
        for a in &anomalies {
            println!("  {}", a);
        }
    }
    println!("╚══════════════════════════════════════════════════════╝\n");
}

async fn show_history() {
    println!("📜 操作历史：在 chat 模式下输入 '/history' 查看");
}

fn show_history_inline(agent: &AgentLoop) {
    println!("\n📜 操作历史（最近 10 步）：");
    println!();

    let ops = agent.memory.operations.iter().rev().take(10).collect::<Vec<_>>();

    if ops.is_empty() {
        println!("   （无操作记录）");
    } else {
        for (i, op) in ops.iter().enumerate() {
            let success = op.result.as_ref().map(|r| r.success).unwrap_or(false);
            let can_undo = op.rollback.is_some();
            println!(
                "   {}. {} {} {}",
                i + 1,
                if success { "✅" } else { "❌" },
                op.tool_call.tool,
                if can_undo { "（可撤销）" } else { "" }
            );
        }
    }
    println!();
}

async fn handle_undo_inline(agent: &mut AgentLoop) {
    match agent.undo_last().await {
        Ok(message) => println!("\n{}\n", message),
        Err(err) => println!("\n⚠️  {}\n", err),
    }
}

#[cfg(test)]
mod tests {
    use super::{
        ChatInputAction, DisambiguationOption, PlaybookCommand, PlaybookSaveParseError,
        PreparedRunInstruction, format_multi_step_instruction, format_playbook_listing,
        parse_playbook_command, parse_playbook_save_request, prepare_run_instruction,
    };
    use crate::agent::planner::TaskPlan;
    use crate::playbook::PlaybookManager;

    #[test]
    fn prepares_single_plan_for_direct_execution() {
        let prepared = prepare_run_instruction(
            "查看磁盘",
            TaskPlan::Single {
                description: "查看磁盘使用率".to_string(),
            },
        );

        assert_eq!(
            prepared,
            PreparedRunInstruction::Execute {
                display_instruction: "查看磁盘".to_string(),
                actual_instruction: "查看磁盘使用率".to_string(),
            }
        );
    }

    #[test]
    fn prepares_multi_plan_as_stepwise_instruction() {
        let prepared = prepare_run_instruction(
            "把 nginx 配置到 8080 并重启",
            TaskPlan::Multi {
                description: "配置 nginx 到 8080 并重启".to_string(),
                estimated_steps: vec![
                    "修改配置".to_string(),
                    "校验配置".to_string(),
                    "重启服务".to_string(),
                ],
            },
        );

        assert_eq!(
            prepared,
            PreparedRunInstruction::Execute {
                display_instruction: "把 nginx 配置到 8080 并重启".to_string(),
                actual_instruction: format_multi_step_instruction(
                    "配置 nginx 到 8080 并重启",
                    &["修改配置".to_string(), "校验配置".to_string(), "重启服务".to_string()],
                ),
            }
        );
    }

    #[test]
    fn prepares_ambiguous_plan_for_clarification() {
        let options = vec![DisambiguationOption {
            label: "A".to_string(),
            description: "清理日志".to_string(),
            preview: "仅预览".to_string(),
        }];

        let prepared = prepare_run_instruction(
            "帮我清理一下",
            TaskPlan::Ambiguous {
                options: options.clone(),
            },
        );

        assert_eq!(prepared, PreparedRunInstruction::Clarify { options });
    }

    #[test]
    fn parses_playbook_save_request_with_default_step_count() {
        let request = parse_playbook_save_request("/playbook save demo").unwrap();
        assert_eq!(request.name, "demo");
        assert_eq!(request.step_count, 3);
    }

    #[test]
    fn parses_playbook_save_request_with_explicit_step_count() {
        let request = parse_playbook_save_request("/playbook save demo 5").unwrap();
        assert_eq!(request.name, "demo");
        assert_eq!(request.step_count, 5);
    }

    #[test]
    fn rejects_invalid_playbook_command_usage() {
        assert_eq!(
            parse_playbook_save_request("/playbook list"),
            Err(PlaybookSaveParseError::InvalidUsage)
        );
    }

    #[test]
    fn rejects_invalid_playbook_name() {
        assert_eq!(
            parse_playbook_save_request("/playbook save bad/name"),
            Err(PlaybookSaveParseError::InvalidName)
        );
    }

    #[test]
    fn rejects_non_numeric_step_count() {
        assert_eq!(
            parse_playbook_save_request("/playbook save demo abc"),
            Err(PlaybookSaveParseError::InvalidStepCount)
        );
    }

    #[test]
    fn rejects_zero_step_count() {
        assert_eq!(
            parse_playbook_save_request("/playbook save demo 0"),
            Err(PlaybookSaveParseError::ZeroStepCount)
        );
    }

    #[test]
    fn parses_playbook_list_command_without_keyword() {
        assert_eq!(
            parse_playbook_command("/playbook list"),
            Ok(PlaybookCommand::List { keyword: None })
        );
    }

    #[test]
    fn parses_playbook_list_command_with_keyword() {
        assert_eq!(
            parse_playbook_command("/playbook list nginx"),
            Ok(PlaybookCommand::List {
                keyword: Some("nginx"),
            })
        );
    }

    #[test]
    fn parses_bare_playbook_command_as_help() {
        assert_eq!(parse_playbook_command("/playbook"), Ok(PlaybookCommand::Help));
    }

    #[test]
    fn parses_playbook_run_command() {
        assert_eq!(
            parse_playbook_command("/playbook run system-health-check"),
            Ok(PlaybookCommand::Run { name: "system-health-check" })
        );
    }

    #[test]
    fn formats_playbook_listing_with_sorted_items() {
        let mut manager = PlaybookManager::new();
        manager.initialize(None).unwrap();

        let rendered = format_playbook_listing(&manager, None);
        let cleanup = rendered.find("cleanup-old-logs").unwrap();
        let install = rendered.find("install-web-server").unwrap();
        let health = rendered.find("system-health-check").unwrap();

        assert!(cleanup < install && install < health);
        assert!(rendered.contains("/playbook save <名称> [步数]"));
    }

    #[test]
    fn formats_playbook_listing_empty_search_state() {
        let mut manager = PlaybookManager::new();
        manager.initialize(None).unwrap();

        let rendered = format_playbook_listing(&manager, Some("missing-keyword"));
        assert!(rendered.contains("未找到包含 'missing-keyword' 的 Playbook"));
    }

    #[test]
    fn chat_input_action_execute_variant_is_equatable() {
        assert_eq!(
            ChatInputAction::Execute {
                actual_input: "查看磁盘".to_string(),
                plan_steps: vec![],
            },
            ChatInputAction::Execute {
                actual_input: "查看磁盘".to_string(),
                plan_steps: vec![],
            }
        );
    }
}
