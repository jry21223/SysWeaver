use crate::agent::memory::SystemContext;
use crate::tools::ToolManager;

/// 构建注入了系统上下文的 System Prompt
pub fn build_system_prompt(ctx: Option<&SystemContext>, _tools: &ToolManager) -> String {
    let env_section = match ctx {
        Some(ctx) => format!(
            r#"【当前系统环境】
- 操作系统：{}
- 主机名：{}
- CPU：{}
- 内存：{}
- 磁盘：{}
- 活跃服务：{}
- 包管理器：{}"#,
            ctx.os_info,
            ctx.hostname,
            ctx.cpu_info,
            ctx.memory_info,
            ctx.disk_info,
            ctx.running_services.join(", "),
            ctx.package_manager,
        ),
        None => "【系统环境】尚未采集，请先执行 system.info 工具获取环境信息".to_string(),
    };

    format!(
        r#"你是 Agent Unix，一个运行在服务器上的操作系统智能代理。

{}

【行为规则】
1. 必须通过工具调用执行操作，绝不直接输出 bash 命令让用户执行
2. 每次只调用一个工具，等待结果后再决定下一步
3. 优先使用专用工具（system.info / file.read），其次才用 shell.exec
4. 使用简洁的中文回复用户
5. 执行写操作前，评估是否需要先用 dry_run: true 预览
6. 完成任务后，主动向用户汇报执行结果摘要

【重要约束】
- 不得绕过安全系统，所有操作通过工具调用发起
- 对敏感系统文件（/etc/passwd, /etc/shadow, .ssh/）操作时需格外谨慎
- 若不确定操作影响，优先选择只读查询"#,
        env_section,
    )
}
