mod agent;
mod config;
mod context;
mod executor;
mod explainer;
mod llm;
mod playbook;
mod safety;
mod tools;
mod types;
mod watchdog;

use anyhow::Result;
use clap::{Parser, Subcommand};
use tracing::info;

use agent::r#loop::AgentLoop;
use config::{LlmConfig, get_provider_presets, find_preset};

#[derive(Parser)]
#[command(
    name = "agent-unix",
    about = "Agent Unix — 自然语言操作系统管理代理",
    version = "0.1.0",
    long_about = "
用自然语言管理你的 Linux/macOS 系统。

━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━
快速开始：
  agent-unix chat                  # 交互式对话（推荐）
  agent-unix run \"查看磁盘\"         # 单条指令
  agent-unix config --preset anthropic  # 快速配置

支持多种 LLM Provider：
  --provider anthropic    # Claude（原生 tool_use）
  --provider openai       # GPT-4o
  --provider openrouter   # 多模型聚合
  --provider groq         # 超快推理
  --provider deepseek     # DeepSeek

环境变量：
  AGENT_UNIX_LLM_API_KEY     # 通用 API Key（优先）
  ANTHROPIC_API_KEY          # Anthropic 官方
  OPENAI_API_KEY             # OpenAI 官方
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
}

#[derive(Subcommand)]
enum Commands {
    /// 进入交互式对话模式（推荐）
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
        /// 使用预设 Provider（anthropic/openai/openrouter/groq/deepseek）
        #[arg(long)]
        preset: Option<String>,

        /// 显示当前配置
        #[arg(long)]
        show: bool,

        /// 列出所有支持的 Provider
        #[arg(long)]
        list: bool,
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

#[tokio::main]
async fn main() -> Result<()> {
    let cli = Cli::parse();

    // 初始化日志
    let log_level = if cli.verbose { "debug" } else { "info" };
    tracing_subscriber::fmt()
        .with_env_filter(format!("agent_unix={}", log_level))
        .init();

    info!("Agent Unix 启动，模式: {}", cli.mode);

    match cli.command {
        // ═══════════════════════════════════════════════════════
        // 不需要 LLM 的命令
        // ═══════════════════════════════════════════════════════
        Commands::Config { preset, show, list } => {
            if list {
                println!("📡 支持的 LLM Provider：");
                println!();

                for p in get_provider_presets() {
                    let default_mark = if p.name == "anthropic" { "（默认）" } else { "" };
                    println!("   • {} {} — {}", p.name, default_mark, p.description);
                    println!("     Base URL: {}", p.base_url);
                    println!("     默认模型: {}", p.default_model);
                    println!("     环境变量: {}", p.env_key_name);
                    println!();
                }

                println!("💡 快速配置示例：");
                println!("   agent-unix config --preset openai");
                println!("   export OPENAI_API_KEY=sk-xxx");
                println!("   agent-unix chat");
                return Ok(());
            }

            if let Some(preset_name) = preset {
                if let Some(p) = find_preset(&preset_name) {
                    println!("✅ 已选择 Provider: {}", p.name);
                    println!();
                    println!("   Provider: {}", p.provider_kind);
                    println!("   Base URL: {}", p.base_url);
                    println!("   默认模型: {}", p.default_model);
                    println!();
                    println!("🔑 请设置 API Key：");
                    println!("   export {}=你的key", p.env_key_name);
                    println!();
                    println!("或使用通用变量：");
                    println!("   export AGENT_UNIX_LLM_API_KEY=你的key");
                    println!();
                    println!("然后运行：");
                    println!("   agent-unix chat");
                } else {
                    println!("❌ 未知的预设: {}", preset_name);
                    println!("   支持的预设: anthropic, openai, openrouter, groq, deepseek");
                    println!("   运行 'agent-unix config --list' 查看完整列表");
                }
                return Ok(());
            }

            if show {
                // 尝试加载当前配置
                match LlmConfig::load(
                    cli.provider.as_deref(),
                    cli.model.as_deref(),
                    cli.base_url.as_deref(),
                    cli.api_key.as_deref(),
                ) {
                    Ok(config) => {
                        println!("📡 当前 LLM 配置：");
                        println!();
                        println!("   Provider: {}", config.provider_kind);
                        println!("   Base URL: {}", config.base_url);
                        println!("   Model: {}", config.model);
                        println!("   API Key: {}...", config.api_key().chars().take(8).collect::<String>());
                        println!();
                        println!("✅ 配置有效，可以开始对话");
                    }
                    Err(e) => {
                        println!("⚠️  配置加载失败：");
                        println!();
                        println!("   {}", e);
                        println!();
                        println!("💡 解决方法：");
                        println!("   1. 运行 'agent-unix config --preset anthropic'");
                        println!("   2. 设置 API Key: export ANTHROPIC_API_KEY=sk-xxx");
                    }
                }
                return Ok(());
            }

            // 默认显示帮助
            println!("💡 Config 命令用法：");
            println!();
            println!("   agent-unix config --list         # 列出支持的 Provider");
            println!("   agent-unix config --preset openai  # 选择预设");
            println!("   agent-unix config --show         # 显示当前配置");
        }

        Commands::Playbooks => {
            use playbook::PlaybookManager;

            let mut manager = PlaybookManager::new();
            manager.initialize(None)?;

            println!("📋 已加载的 Playbook：");
            println!();

            let stats = manager.stats();
            println!("   来源统计：内置 {}，用户 {}，项目 {}，覆盖 {}",
                stats.bundled_count, stats.user_count, stats.project_count, stats.overridden_count);
            println!();

            for pb in manager.list() {
                println!("   • {} — {}", pb.name, pb.description);
                println!("     步骤数：{}，运行次数：{}", pb.steps.len(), pb.run_count);
            }

            if manager.list().is_empty() {
                println!("   （暂无 Playbook）");
            }

            println!();
            println!("💡 在 chat 模式下完成多步任务后，可保存为新 Playbook");
        }

        Commands::History => {
            show_history().await;
        }

        Commands::Undo => {
            println!("⚠️  Undo 需要在 chat 模式下执行（输入 'undo' 命令）");
        }

        Commands::Explain { file } => {
            use explainer::Explainer;

            let explainer = Explainer::new();

            match file {
                None => {
                    println!("📖 反向解释模式 — 支持的文件类型：");
                    println!();

                    for (desc, _path) in explainer.list_supported_files() {
                        println!("   • {}", desc);
                    }

                    println!();
                    println!("💡 使用方式：agent-unix explain <文件路径>");
                    println!("   示例：agent-unix explain /etc/nginx/nginx.conf");
                }
                Some(path) => {
                    println!("📖 正在读取文件: {}", path);

                    match explainer.read_file(&path) {
                        Ok(content) => {
                            println!("   文件大小: {} 字节", content.len());
                            println!();

                            let file_type = explainer.detect_type(&path)
                                .unwrap_or_else(|| "配置文件".to_string());

                            println!("📝 文件内容（前 500 字符）：");
                            println!("{}", content.chars().take(500).collect::<String>());
                            println!();

                            let prompt = explainer.build_explanation_prompt(&file_type, &content);
                            println!("💡 可将以下提示词发给 Agent 进行解释：");
                            println!();
                            println!("{}", prompt.lines().take(10).collect::<Vec<_>>().join("\n"));
                        }
                        Err(e) => {
                            println!("❌ 读取失败: {}", e);
                        }
                    }
                }
            }
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

            handler_task.await?;

            println!("\n✅ 监控已完成");
        }

        Commands::Info => {
            println!("🤖 Agent Unix v0.1.0");
            println!();
            println!("   AI Hackathon 2026 · 超聚变 αFUSION 预赛");
            println!();
            println!("系统信息：");
            println!("   OS: {}", std::env::consts::OS);
            println!("   Arch: {}", std::env::consts::ARCH);
            println!("   Rust: 1.75+");
        }

        // ═══════════════════════════════════════════════════════
        // 需要 LLM 的命令
        // ═══════════════════════════════════════════════════════
        _ => {
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
                eprintln!("💡 快速配置：");
                eprintln!("   1. agent-unix config --preset anthropic");
                eprintln!("   2. export ANTHROPIC_API_KEY=sk-ant-xxx");
                eprintln!("   3. agent-unix chat");
                eprintln!();
                std::process::exit(1);
            });

            info!("LLM Provider: {} @ {} (model: {})",
                llm_config.provider_kind,
                llm_config.base_url,
                llm_config.model
            );

            match cli.command {
                Commands::Chat => {
                    println!("🤖 Agent Unix v0.1.0");
                    println!("   Provider: {} @ {}", llm_config.provider_kind, llm_config.base_url);
                    println!("   正在采集系统环境...");

                    let ctx = context::system_scan::scan().await;
                    println!("   ✅ 系统：{} @ {}", ctx.os_info, ctx.hostname);
                    println!("   输入 'exit' 退出，'undo' 撤销，'history' 历史\n");

                    run_chat_loop(&llm_config, &cli.mode, ctx).await?;
                }
                Commands::Run { instruction, dry_run } => {
                    let ctx = context::system_scan::scan().await;
                    run_single(&llm_config, &cli.mode, ctx, &instruction, dry_run).await?;
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
) -> Result<()> {
    use rustyline::error::ReadlineError;
    use rustyline::DefaultEditor;

    let mut rl = DefaultEditor::new()?;
    let history_path = format!(
        "{}/.agent-unix/history.txt",
        std::env::var("HOME").unwrap_or_else(|_| "/tmp".to_string())
    );
    let _ = rl.load_history(&history_path);

    let mut agent = AgentLoop::new(llm_config.clone(), mode, ctx);

    loop {
        let readline = rl.readline("👤 你：");
        match readline {
            Ok(line) => {
                let input = line.trim();
                if input.is_empty() {
                    continue;
                }
                if input == "exit" || input == "quit" {
                    println!("👋 再见！");
                    break;
                }
                if input == "history" {
                    show_history_inline(&agent);
                    continue;
                }
                if input == "undo" {
                    handle_undo_inline(&mut agent);
                    continue;
                }

                rl.add_history_entry(input)?;

                match agent.run(input, false).await {
                    Ok(response) => println!("\n🤖 Agent：{}\n", response),
                    Err(e) => println!("\n❌ 错误: {}\n", e),
                }
            }
            Err(ReadlineError::Interrupted) => {
                println!("\n⚠️  已中断，输入 'exit' 退出");
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

    let _ = rl.save_history(&history_path);
    Ok(())
}

async fn run_single(
    llm_config: &LlmConfig,
    mode: &str,
    ctx: crate::agent::memory::SystemContext,
    instruction: &str,
    dry_run: bool,
) -> Result<()> {
    let mut agent = AgentLoop::new(llm_config.clone(), mode, ctx);

    println!("🤖 执行: {}", instruction);
    if dry_run {
        println!("   [DRY-RUN 模式：仅预览]\n");
    }

    let result = agent.run(instruction, dry_run).await?;
    println!("\n{}", result);

    Ok(())
}

async fn show_history() {
    println!("📜 操作历史：在 chat 模式下输入 'history' 查看");
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

fn handle_undo_inline(agent: &mut AgentLoop) {

    match agent.memory.last_undoable() {
        None => println!("\n⚠️  没有可撤销的操作\n"),
        Some(op) => {
            let rollback = op.rollback.clone().unwrap();
            println!("\n↩️  撤销：{}", rollback.description);
            if rollback.has_side_effects {
                println!("   注意：此回滚操作可能有副作用");
            }
            for cmd in &rollback.commands {
                println!("   执行: {}", cmd);
            }
            println!();
        }
    }
}