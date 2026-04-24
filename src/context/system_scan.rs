use crate::agent::memory::SystemContext;
use crate::executor::ssh::SshConfig;
use crate::ui::state::ServiceInfo;
use chrono::Utc;
use tokio::process::Command;

/// 启动时扫描系统环境，构建 SystemContext
pub async fn scan() -> SystemContext {
    let run = |cmd: &'static str| async move {
        let out = if cfg!(windows) {
            // 在子进程中也切换到 UTF-8（CP 65001），防止中文命令输出乱码
            let utf8_cmd = format!("chcp 65001 >NUL 2>&1 & {}", cmd);
            Command::new("cmd").args(["/C", utf8_cmd.as_str()]).output().await
        } else {
            Command::new("sh").arg("-c").arg(cmd).output().await
        };
        match out {
            Ok(o) => String::from_utf8_lossy(&o.stdout).trim().to_string(),
            Err(_) => "未知".to_string(),
        }
    };

    let os_info = if cfg!(target_os = "macos") {
        run("sw_vers | awk '/ProductName/{n=$2}/ProductVersion/{v=$2}END{print n\" \"v}'").await
    } else if cfg!(windows) {
        run("ver").await
    } else {
        run("cat /etc/os-release 2>/dev/null | grep PRETTY_NAME | cut -d= -f2 | tr -d '\"' || uname -s").await
    };

    let hostname = run("hostname").await;

    let cpu_info = if cfg!(target_os = "macos") {
        run("cores=$(sysctl -n hw.logicalcpu); model=$(sysctl -n machdep.cpu.brand_string 2>/dev/null || sysctl -n hw.model); echo \"${cores} cores, ${model}\"").await
    } else if cfg!(windows) {
        run("wmic cpu get Name,NumberOfLogicalProcessors /format:value").await
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
    } else if cfg!(windows) {
        {
            let raw = run("wmic OS get TotalVisibleMemorySize,FreePhysicalMemory /value 2>NUL").await;
            let mut total_kb = 0u64;
            let mut free_kb = 0u64;
            for l in raw.lines() {
                if let Some(v) = l.strip_prefix("TotalVisibleMemorySize=") {
                    total_kb = v.trim().parse().unwrap_or(0);
                } else if let Some(v) = l.strip_prefix("FreePhysicalMemory=") {
                    free_kb = v.trim().parse().unwrap_or(0);
                }
            }
            let total_gb = total_kb as f64 / 1_048_576.0;
            let used_gb = total_kb.saturating_sub(free_kb) as f64 / 1_048_576.0;
            format!("{:.1}G total, {:.1}G used", total_gb, used_gb)
        }
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
    } else if cfg!(windows) {
        {
            let raw = run("wmic logicaldisk where DeviceID='C:' get Size,FreeSpace /value 2>NUL").await;
            let mut size = 0u64;
            let mut free = 0u64;
            for l in raw.lines() {
                if let Some(v) = l.strip_prefix("Size=") {
                    size = v.trim().parse().unwrap_or(0);
                } else if let Some(v) = l.strip_prefix("FreeSpace=") {
                    free = v.trim().parse().unwrap_or(0);
                }
            }
            let size_gb = size as f64 / 1_073_741_824.0;
            let free_gb = free as f64 / 1_073_741_824.0;
            let pct = if size > 0 { size.saturating_sub(free) as f64 / size as f64 * 100.0 } else { 0.0 };
            format!("{:.0}G total, {:.1}G free, {:.0}% used", size_gb, free_gb, pct)
        }
    } else {
        run("df -k / | tail -1 | awk '{printf \"%.0fG total, %.1fG free, %s used\", $2/976562.5, $4/976562.5, $5}'").await
    };

    // 服务列表（macOS 过滤 Apple 内部服务，只保留可识别的第三方/系统服务）
    let services_raw = if cfg!(target_os = "macos") {
        run("launchctl list 2>/dev/null | awk 'NF>=3 && $1~/^[0-9]+$/{print $3}' | grep -vE '^(com\\.apple\\.|application\\.com\\.apple\\.)' | grep -vE '^application\\.' | head -10").await
    } else if cfg!(windows) {
        run("sc query state= running 2>NUL | findstr SERVICE_NAME").await
            .lines()
            .map(|l| l.trim_start_matches("SERVICE_NAME:").trim().to_string())
            .filter(|s| !s.is_empty())
            .take(10)
            .collect::<Vec<_>>()
            .join("\n")
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
    } else if cfg!(windows) {
        if run("winget --version 2>NUL").await.contains('.') {
            "winget".to_string()
        } else if run("choco --version 2>NUL").await.contains('.') {
            "choco".to_string()
        } else if run("scoop --version 2>NUL").await.contains("scoop") {
            "scoop".to_string()
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

    // 网络信息：本机 IP 及监听端口
    let network_info = if cfg!(target_os = "macos") {
        run("ips=$(ifconfig | grep 'inet ' | grep -v '127.0.0.1' | awk '{print $2}' | head -3 | tr '\\n' ' '); ports=$(lsof -iTCP -sTCP:LISTEN -P 2>/dev/null | awk 'NR>1{print $9}' | sed 's/.*://' | sort -nu | head -8 | tr '\\n' ','); echo \"IP:${ips:-127.0.0.1} 监听端口:${ports:-无}\"").await
    } else if cfg!(windows) {
        {
            let raw = run("ipconfig 2>NUL | findstr \"IPv4\"").await;
            let ips: Vec<String> = raw
                .lines()
                .filter_map(|l| l.split(':').last().map(|s| s.trim().to_string()))
                .filter(|s| !s.is_empty() && s != "127.0.0.1")
                .take(3)
                .collect();
            let ip_str = if ips.is_empty() { "127.0.0.1".to_string() } else { ips.join(" ") };
            format!("IP:{}", ip_str)
        }
    } else {
        run("ips=$(ip addr show 2>/dev/null | grep 'inet ' | grep -v '127.0.0.1' | awk '{print $2}' | cut -d/ -f1 | head -3 | tr '\\n' ' '); ports=$(ss -tlnp 2>/dev/null | awk 'NR>1{print $4}' | awk -F: '{print $NF}' | sort -nu | head -8 | tr '\\n' ','); echo \"IP:${ips:-127.0.0.1} 监听端口:${ports:-无}\"").await
    };

    SystemContext {
        os_info,
        hostname,
        cpu_info,
        memory_info,
        disk_info,
        running_services,
        package_manager,
        network_info,
        collected_at: Utc::now(),
    }
}

/// 通过 SSH 扫描远程主机的系统环境，返回 SystemContext
pub async fn scan_remote(ssh: &SshConfig) -> SystemContext {
    let exec = |cmd: String| {
        let ssh = ssh.clone();
        async move {
            match ssh.exec(&cmd).await {
                Ok((stdout, _, _, _)) => stdout.trim().to_string(),
                Err(_) => "未知".to_string(),
            }
        }
    };

    let os_info = exec("cat /etc/os-release 2>/dev/null | grep PRETTY_NAME | cut -d= -f2 | tr -d '\"' || uname -s".to_string()).await;
    let hostname = exec("hostname".to_string()).await;

    let cpu_info = exec("cores=$(nproc 2>/dev/null || sysctl -n hw.logicalcpu 2>/dev/null || echo 0); model=$(grep 'model name' /proc/cpuinfo 2>/dev/null | head -1 | cut -d: -f2 | xargs || sysctl -n machdep.cpu.brand_string 2>/dev/null || echo unknown); echo \"${cores} cores, ${model}\"".to_string()).await;

    let memory_info = exec("free -h 2>/dev/null | grep Mem | awk '{print $2\" total, \"$3\" used\"}' || vm_stat 2>/dev/null | head -5".to_string()).await;

    let disk_info = exec("df -k / 2>/dev/null | tail -1 | awk '{printf \"%.0fG total, %.1fG free, %s used\", $2/976562.5, $4/976562.5, $5}'".to_string()).await;

    let services_raw = exec("systemctl list-units --type=service --state=running --no-pager --no-legend 2>/dev/null | awk '{print $1}' | sed 's/.service//' | head -10 || launchctl list 2>/dev/null | awk 'NF>=3 && $1~/^[0-9]+$/{print $3}' | grep -vE '^com\\.apple\\.' | head -10".to_string()).await;
    let running_services: Vec<String> = services_raw
        .lines()
        .map(|s| s.trim().to_string())
        .filter(|s| !s.is_empty())
        .collect();

    let network_info = exec("ips=$(ip addr show 2>/dev/null | grep 'inet ' | grep -v '127.0.0.1' | awk '{print $2}' | cut -d/ -f1 | head -3 | tr '\\n' ' '); ports=$(ss -tlnp 2>/dev/null | awk 'NR>1{print $4}' | awk -F: '{print $NF}' | sort -nu | head -6 | tr '\\n' ','); echo \"IP:${ips:-127.0.0.1} 监听端口:${ports:-无}\"".to_string()).await;

    let package_manager = {
        let pm_raw = exec("which apt dnf yum brew 2>/dev/null | head -1".to_string()).await;
        if pm_raw.contains("apt") { "apt".to_string() }
        else if pm_raw.contains("dnf") { "dnf".to_string() }
        else if pm_raw.contains("yum") { "yum".to_string() }
        else if pm_raw.contains("brew") { "brew".to_string() }
        else { "unknown".to_string() }
    };

    SystemContext {
        os_info,
        hostname,
        cpu_info,
        memory_info,
        disk_info,
        running_services,
        package_manager,
        network_info,
        collected_at: Utc::now(),
    }
}

/// 获取本地系统真实运行的服务列表，按 CPU 占用排序。
/// 输出格式：cpu_pct|mem_mb|service_name
pub async fn get_service_status() -> Vec<ServiceInfo> {
    let cmd = if cfg!(target_os = "macos") {
        // macOS：launchctl list 取 PID + label，过滤掉 Apple 内部服务，
        // 然后用 ps 批量获取 CPU/MEM
        concat!(
            "launchctl list 2>/dev/null",
            " | awk 'NF>=3 && $1~/^[0-9]/ && !/com\\.apple\\./",
            " && !/application\\./{print $1, $3}'",
            " | head -20",
            " | while read pid label; do",
            "   stats=$(ps -p \"$pid\" -o pcpu=,rss= 2>/dev/null | xargs);",
            "   [ -z \"$stats\" ] && continue;",
            "   cpu=$(echo $stats | awk '{print $1}');",
            "   mem=$(echo $stats | awk '{printf \"%.0f\", $2/1024}');",
            "   name=$(echo $label | awk -F. '{print $NF}');",
            "   echo \"${cpu:-0}|${mem:-0}|$name\";",
            " done",
            " | sort -t'|' -k1 -rn | head -8"
        )
    } else {
        // Linux：systemctl 取运行中服务 → MainPID → ps 获取 CPU/MEM
        concat!(
            "systemctl list-units --type=service --state=running",
            " --no-pager --no-legend 2>/dev/null",
            " | awk '{name=$1; gsub(/\\.service$/,\"\",name); print name}'",
            " | head -20",
            " | while read n; do",
            "   pid=$(systemctl show \"${n}.service\"",
            "         --property=MainPID --value 2>/dev/null | tr -d '[:space:]');",
            "   [ \"${pid:-0}\" -gt 0 ] 2>/dev/null || continue;",
            "   stats=$(ps -p \"$pid\" -o pcpu=,rss= 2>/dev/null | tail -1 | xargs);",
            "   [ -z \"$stats\" ] && continue;",
            "   cpu=$(echo $stats | awk '{print $1}');",
            "   mem=$(echo $stats | awk '{printf \"%.0f\", $2/1024}');",
            "   echo \"${cpu:-0}|${mem:-0}|$n\";",
            " done",
            " | sort -t'|' -k1 -rn | head -8"
        )
    };

    let out = Command::new("sh").arg("-c").arg(cmd).output().await;
    parse_service_lines(out.ok().as_ref().map(|o| String::from_utf8_lossy(&o.stdout).into_owned()).as_deref().unwrap_or(""))
}

/// 通过 SSH 获取远程主机的真实运行服务，按 CPU 排序
pub async fn get_remote_service_status(ssh: &SshConfig) -> Vec<ServiceInfo> {
    // 单次 SSH 连接执行，兼容 Linux/macOS 远程
    let cmd = concat!(
        "if command -v systemctl >/dev/null 2>&1; then",
        "  systemctl list-units --type=service --state=running --no-pager --no-legend 2>/dev/null",
        "  | awk '{name=$1; gsub(/\\.service$/,\"\",name); print name}' | head -20",
        "  | while read n; do",
        "      pid=$(systemctl show \"${n}.service\" --property=MainPID --value 2>/dev/null | tr -d '[:space:]');",
        "      [ \"${pid:-0}\" -gt 0 ] 2>/dev/null || continue;",
        "      stats=$(ps -p \"$pid\" -o pcpu=,rss= 2>/dev/null | tail -1 | xargs);",
        "      [ -z \"$stats\" ] && continue;",
        "      cpu=$(echo $stats | awk '{print $1}');",
        "      mem=$(echo $stats | awk '{printf \"%.0f\", $2/1024}');",
        "      echo \"${cpu:-0}|${mem:-0}|$n\";",
        "  done",
        "else",
        "  launchctl list 2>/dev/null",
        "  | awk 'NF>=3 && $1~/^[0-9]/ && !/com\\.apple\\./{print $1, $3}' | head -20",
        "  | while read pid label; do",
        "      stats=$(ps -p \"$pid\" -o pcpu=,rss= 2>/dev/null | xargs);",
        "      [ -z \"$stats\" ] && continue;",
        "      cpu=$(echo $stats | awk '{print $1}');",
        "      mem=$(echo $stats | awk '{printf \"%.0f\", $2/1024}');",
        "      name=$(echo $label | awk -F. '{print $NF}');",
        "      echo \"${cpu:-0}|${mem:-0}|$name\";",
        "  done",
        "fi",
        " | sort -t'|' -k1 -rn | head -8"
    );

    match ssh.exec(cmd).await {
        Ok((stdout, _, _, _)) => parse_service_lines(&stdout),
        Err(_) => Vec::new(),
    }
}

/// 解析 "cpu|mem_mb|name" 格式的行，返回 ServiceInfo 列表
fn parse_service_lines(output: &str) -> Vec<ServiceInfo> {
    output
        .lines()
        .filter_map(|line| {
            let line = line.trim();
            let parts: Vec<&str> = line.splitn(3, '|').collect();
            if parts.len() != 3 { return None; }
            let cpu_pct: f32 = parts[0].trim().parse().ok()?;
            let mem_mb: f32 = parts[1].trim().parse().unwrap_or(0.0);
            let name = parts[2].trim().to_string();
            if name.is_empty() { return None; }
            Some(ServiceInfo { name, cpu_pct, mem_mb })
        })
        .collect()
}

/// 基于系统上下文做主动异常检测，返回告警消息列表
pub fn detect_anomalies(ctx: &SystemContext) -> Vec<String> {
    let mut alerts = Vec::new();

    // 磁盘使用率检测
    if let Some(pct) = extract_percent(&ctx.disk_info) {
        if pct >= 90 {
            alerts.push(format!("🔴 磁盘严重不足（{}% 已用），请立即清理", pct));
        } else if pct >= 80 {
            alerts.push(format!("🟡 磁盘空间紧张（{}% 已用），建议及时清理", pct));
        }
    }

    // 内存使用率检测
    if let Some(mem_pct) = extract_memory_percent(&ctx.memory_info) {
        if mem_pct >= 90 {
            alerts.push(format!("🔴 内存严重不足（{}% 已用），系统可能出现性能问题", mem_pct));
        } else if mem_pct >= 80 {
            alerts.push(format!("🟡 内存使用率偏高（{}%），建议排查占用进程", mem_pct));
        }
    }

    alerts
}

/// 从磁盘信息字符串中提取使用百分比（如 "47% used" → 47）
fn extract_percent(info: &str) -> Option<u64> {
    let pct_pos = info.find('%')?;
    let before = &info[..pct_pos];
    let num_start = before.rfind(|c: char| !c.is_ascii_digit()).map(|p| p + 1).unwrap_or(0);
    before[num_start..].parse::<u64>().ok()
}

/// 从内存信息字符串中估算使用百分比
fn extract_memory_percent(info: &str) -> Option<u64> {
    let lower = info.to_lowercase();
    let total = extract_size_gb(&lower, "total")?;
    let used = extract_size_gb(&lower, "used")?;
    if total > 0.0 {
        Some((used / total * 100.0) as u64)
    } else {
        None
    }
}

/// 从形如 "4.2G used" 或 "4200M total" 的字符串中提取 GB 值
fn extract_size_gb(info: &str, keyword: &str) -> Option<f64> {
    let pos = info.find(keyword)?;
    let before = info[..pos].trim_end();
    let unit_pos = before.rfind(|c: char| matches!(c, 'g' | 'm' | 'k' | 't' | 'b'))?;
    let unit = before.chars().nth(unit_pos)?;
    let num_str = &before[..unit_pos].trim();
    let num_start = num_str.rfind(|c: char| !c.is_ascii_digit() && c != '.').map(|p| p + 1).unwrap_or(0);
    let val: f64 = num_str[num_start..].parse().ok()?;
    let gb = match unit {
        'g' => val,
        'm' => val / 1024.0,
        'k' => val / 1024.0 / 1024.0,
        't' => val * 1024.0,
        _ => val,
    };
    Some(gb)
}
