use crate::agent::memory::SystemContext;
use chrono::Utc;
use tokio::process::Command;

/// 启动时扫描系统环境，构建 SystemContext
pub async fn scan() -> SystemContext {
    let run = |cmd: &'static str| async move {
        let out = Command::new("sh").arg("-c").arg(cmd).output().await;
        match out {
            Ok(o) => String::from_utf8_lossy(&o.stdout).trim().to_string(),
            Err(_) => "未知".to_string(),
        }
    };

    let os_info = run(
        "cat /etc/os-release 2>/dev/null | grep PRETTY_NAME | cut -d= -f2 | tr -d '\"' || uname -s",
    )
    .await;
    let hostname = run("hostname").await;
    let cpu_info =
        run("nproc && cat /proc/cpuinfo | grep 'model name' | head -1 | cut -d: -f2 | xargs").await;
    let memory_info = run("free -h | grep Mem | awk '{print $2\" total, \"$3\" used\"}'").await;
    let disk_info = run("df -h / | tail -1 | awk '{print $1\" \"$2\", \"$5\" used\"}'").await;

    let services_raw = run(
        "systemctl list-units --type=service --state=running --no-pager --no-legend 2>/dev/null | awk '{print $1}' | sed 's/.service//' | head -10"
    ).await;
    let running_services: Vec<String> = services_raw
        .lines()
        .map(|s| s.trim().to_string())
        .filter(|s| !s.is_empty())
        .collect();

    // 检测包管理器
    let package_manager = if run("which apt 2>/dev/null").await.contains("apt") {
        "apt".to_string()
    } else if run("which dnf 2>/dev/null").await.contains("dnf") {
        "dnf".to_string()
    } else if run("which yum 2>/dev/null").await.contains("yum") {
        "yum".to_string()
    } else {
        "unknown".to_string()
    };

    SystemContext {
        os_info,
        hostname,
        cpu_info,
        memory_info,
        disk_info,
        running_services,
        package_manager,
        collected_at: Utc::now(),
    }
}
