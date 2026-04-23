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

    let os_info = if cfg!(target_os = "macos") {
        run("sw_vers | awk '/ProductName/{n=$2}/ProductVersion/{v=$2}END{print n\" \"v}'").await
    } else {
        run("cat /etc/os-release 2>/dev/null | grep PRETTY_NAME | cut -d= -f2 | tr -d '\"' || uname -s").await
    };

    let hostname = run("hostname").await;

    let cpu_info = if cfg!(target_os = "macos") {
        run("cores=$(sysctl -n hw.logicalcpu); model=$(sysctl -n machdep.cpu.brand_string 2>/dev/null || sysctl -n hw.model); echo \"${cores} cores, ${model}\"").await
    } else {
        run("cores=$(nproc); model=$(grep 'model name' /proc/cpuinfo 2>/dev/null | head -1 | cut -d: -f2 | xargs); echo \"${cores} cores, ${model:-unknown}\"").await
    };

    // 内存：macOS 使用 vm_stat + sysctl，格式需符合 "Xtotal, Yused"
    let memory_info = if cfg!(target_os = "macos") {
        run(concat!(
            "PAGE=$(sysctl -n hw.pagesize); TOTAL=$(sysctl -n hw.memsize); ",
            "vm_stat | awk -v page=$PAGE -v total=$TOTAL '",
            "/Pages active/{gsub(/\\./, \"\", $3); a=$3+0}",
            "/Pages wired down/{gsub(/\\./, \"\", $4); w=$4+0}",
            "/Pages occupied by compressor/{gsub(/\\./, \"\", $5); c=$5+0}",
            "END{printf \"%.1fG total, %.1fG used\", total/1073741824, (a+w+c)*page/1073741824}'"
        )).await
    } else {
        run("free -h | grep Mem | awk '{print $2\" total, \"$3\" used\"}'").await
    };

    // 磁盘：macOS 用 diskutil 获取 APFS 容器真实用量；df 在 APFS 只报告 root 快照卷
    let disk_info = if cfg!(target_os = "macos") {
        run(
            "diskutil info / 2>/dev/null | awk '\
             /Container Total Space/ { total = $4 + 0 } \
             /Container Free Space/ { free = $4 + 0 } \
             END { \
               if (total > 0) { \
                 used = total - free; pct = used / total * 100; \
                 printf \"%.0fG total, %.1fG free, %.0f%% used\", total, free, pct \
               } \
             }'",
        )
        .await
    } else {
        run("df -k / | tail -1 | awk '{printf \"%.0fG total, %.1fG free, %s used\", $2/976562.5, $4/976562.5, $5}'").await
    };

    // 服务列表（macOS 过滤 Apple 内部服务，只保留可识别的第三方/系统服务）
    let services_raw = if cfg!(target_os = "macos") {
        run("launchctl list 2>/dev/null | awk 'NF>=3 && $1~/^[0-9]+$/{print $3}' | grep -vE '^(com\\.apple\\.|application\\.com\\.apple\\.)' | grep -vE '^application\\.' | head -10").await
    } else {
        run("systemctl list-units --type=service --state=running --no-pager --no-legend 2>/dev/null | awk '{print $1}' | sed 's/.service//' | head -10").await
    };
    let running_services: Vec<String> = services_raw
        .lines()
        .map(|s| s.trim().to_string())
        .filter(|s| !s.is_empty())
        .collect();

    // 检测包管理器
    let package_manager = if cfg!(target_os = "macos") {
        if run("which brew 2>/dev/null").await.contains("brew") {
            "brew".to_string()
        } else {
            "unknown".to_string()
        }
    } else if run("which apt 2>/dev/null").await.contains("apt") {
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
