mod agent;
mod config;
mod context;
mod executor;
mod llm;
mod playbook;
mod safety;
mod tools;
mod types;

use anyhow::Result;
use clap::{Parser, Subcommand};
use tracing::info;

use agent::r#loop::AgentLoop;
use config::LlmConfig;

#[derive(Parser)]
#[command(
    name = "agent-unix",
    about = "Agent Unix — 自然语言操作系统管理代理",
    version = "0.1.0",
    long_about = "用自然语言管理你的 Linux 服务器。\n\n示例:\n  agent-unix chat\n  agent-unix run \"查看磁盘使用情况\"\n  agent-unix run --dry-run \"清理 30 天前的日志\""
)]
struct Cli {
    #[command(subcommand)]
    command: Commands,

    /// 详细日志输出
    #[arg(long, global = true)]
    verbose: bool,

    /// 运行模式：safe（所有写操作需确认）| normal（默认）| auto（仅 CRITICAL 需确认）
    #[arg(long, global = true, default_value = "normal")]
    mode: String,

    /// LLM Provider: anthropic | openai-compatible
    #[arg(long, global = true)]
    provider: Option<String>,

    /// LLM 模型 ID（如 claude-sonnet-4-5, gpt-4o）
    #[arg(long, global = true)]
    model: Option<String>,

    /// LLM API Base URL（自定义端点）
    #[arg(long, global = true)]
    base_url: Option<String>,
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
    /// 查看操作历史
    History,
    /// 撤销上一次操作
    Undo,
    /// 列出保存的 Playbook
    Playbooks,
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
        // Playbooks 和 History 不需要 LLM 配置
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

        // 其他命令需要 LLM 配置
        _ => {
            // 加载 LLM 配置（CLI > ENV > 默认）
            let llm_config = LlmConfig::load(
                cli.provider.as_deref(),
                cli.model.as_deref(),
                cli.base_url.as_deref(),
            ).unwrap_or_else(|e| {
                eprintln!("⚠️  配置加载失败: {}", e);
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
                    println!("   正在采集系统环境...");

                    let ctx = context::system_scan::scan().await;
                    println!("   ✅ 系统：{} @ {}", ctx.os_info, ctx.hostname);
                    println!("   输入 'exit' 退出，'undo' 撤销上一步操作\n");

                    run_chat_loop(&llm_config, &cli.mode, ctx).await?;
                }
                Commands::Run {
                    instruction,
                    dry_run,
                } => {
                    let ctx = context::system_scan::scan().await;
                    run_single(&llm_config, &cli.mode, ctx, &instruction, dry_run).await?;
                }
                Commands::History | Commands::Undo | Commands::Playbooks => {}
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
        match rl.readline("你 › ") {
            Ok(line) => {
                let input = line.trim().to_string();
                if input.is_empty() {
                    continue;
                }
                let _ = rl.add_history_entry(&input);

                match input.as_str() {
                    "exit" | "quit" | "q" => {
                        println!("再见！");
                        break;
                    }
                    "undo" => {
                        handle_undo(&mut agent).await;
                        continue;
                    }
                    "history" => {
                        show_agent_history(&agent);
                        continue;
                    }
                    _ => {}
                }

                print!("\nAgent › ");
                match agent.run(&input, false).await {
                    Ok(reply) => println!("{}\n", reply),
                    Err(e) => eprintln!("❌ 执行出错: {}\n", e),
                }
            }
            Err(ReadlineError::Interrupted) | Err(ReadlineError::Eof) => {
                println!("\n再见！");
                break;
            }
            Err(e) => {
                eprintln!("输入错误: {}", e);
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

    if dry_run {
        println!("🔍 DRY-RUN 模式：仅预览，不实际执行\n");
    }

    println!("Agent › ");
    match agent.run(instruction, dry_run).await {
        Ok(reply) => println!("{}", reply),
        Err(e) => {
            eprintln!("❌ 执行出错: {}", e);
            std::process::exit(1);
        }
    }

    Ok(())
}

async fn show_history() {
    let log_dir = format!(
        "{}/.agent-unix",
        std::env::var("HOME").unwrap_or_else(|_| "/tmp".to_string())
    );
    let pattern = format!("{}/audit-*.jsonl", log_dir);

    match glob_audit_files(&pattern).await {
        Some(entries) if !entries.is_empty() => {
            println!("📋 操作历史（最近 20 条）：\n");
            for entry in entries.iter().rev().take(20) {
                let ts = entry["ts"].as_str().unwrap_or("?");
                let tool = entry["tool"].as_str().unwrap_or("?");
                let risk = entry["risk"].as_str().unwrap_or("?");
                let success = entry["success"].as_bool();
                let blocked = entry["blocked"].as_bool().unwrap_or(false);

                let status = if blocked {
                    "🚨 拦截"
                } else {
                    match success {
                        Some(true) => "✅",
                        Some(false) => "❌",
                        None => "⏭️ 取消",
                    }
                };
                println!(
                    "  {} {} [{}] {} {}",
                    status,
                    &ts[..16],
                    risk,
                    tool,
                    entry["args"]
                        .to_string()
                        .chars()
                        .take(60)
                        .collect::<String>()
                );
            }
        }
        _ => println!("暂无操作历史记录。"),
    }
}

async fn glob_audit_files(pattern: &str) -> Option<Vec<serde_json::Value>> {
    use tokio::fs::File;
    use tokio::io::{AsyncBufReadExt, BufReader};

    // 简单获取当日日志文件
    let log_path = pattern.replace("*", &chrono::Local::now().format("%Y%m%d").to_string());
    let file = File::open(&log_path).await.ok()?;
    let reader = BufReader::new(file);
    let mut lines = reader.lines();
    let mut entries = Vec::new();

    while let Ok(Some(line)) = lines.next_line().await {
        if let Ok(v) = serde_json::from_str::<serde_json::Value>(&line) {
            entries.push(v);
        }
    }

    Some(entries)
}

fn show_agent_history(agent: &AgentLoop) {
    let ops = &agent.memory.operations;
    if ops.is_empty() {
        println!("本次会话暂无操作记录。");
        return;
    }
    println!("\n📋 本次会话操作记录：");
    for (i, op) in ops.iter().enumerate() {
        let success = op.result.as_ref().map(|r| r.success).unwrap_or(false);
        let can_undo = op.rollback.is_some();
        println!(
            "  {}. {} {} {}",
            i + 1,
            if success { "✅" } else { "❌" },
            op.tool_call.tool,
            if can_undo { "（可撤销）" } else { "" }
        );
    }
}

async fn handle_undo(agent: &mut AgentLoop) {
    match agent.memory.last_undoable() {
        None => println!("⚠️  没有可撤销的操作"),
        Some(op) => {
            let rollback = op.rollback.clone().unwrap();
            println!("↩️  撤销：{}", rollback.description);
            if rollback.has_side_effects {
                println!("   注意：此回滚操作可能有副作用");
            }
            for cmd in &rollback.commands {
                println!("   执行: {}", cmd);
                // 通过 Agent 执行回滚命令
                let instruction = format!("执行回滚命令: {}", cmd);
                match agent.run(&instruction, false).await {
                    Ok(_) => println!("   ✅ 完成"),
                    Err(e) => println!("   ❌ 失败: {}", e),
                }
            }
        }
    }
}
