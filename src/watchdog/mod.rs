use anyhow::Result;
use std::sync::Arc;
use std::time::Duration;
use tokio::sync::mpsc::{channel, Sender, Receiver};
use tokio::sync::watch;
use tokio::time::interval;

/// Watchdog 监控系统：后台监控资源使用，异常时发送告警
pub struct Watchdog {
    /// 监控规则
    rules: Vec<MonitorRule>,
    /// 告警发送通道
    alert_tx: Sender<Alert>,
    /// 运行状态
    running: Arc<std::sync::atomic::AtomicBool>,
    /// 关闭信号发送端（broadcast via watch channel）
    shutdown_tx: watch::Sender<bool>,
}

/// 监控规则
#[derive(Debug, Clone)]
pub struct MonitorRule {
    /// 规则名称
    pub name: String,
    /// 监控指标
    pub metric: MetricType,
    /// 阈值
    pub threshold: f64,
    /// 检查间隔（秒）
    pub interval_secs: u64,
    /// 告警级别
    pub severity: AlertSeverity,
}

/// 监控指标类型
#[derive(Debug, Clone)]
#[allow(dead_code)] // 完整指标集，部分变体在当前版本未激活
pub enum MetricType {
    /// 磁盘使用率（百分比）
    DiskUsage { mount_point: String },
    /// 内存使用率（百分比）
    MemoryUsage,
    /// CPU 使用率（百分比）
    CpuUsage,
    /// 进程是否存在
    ProcessRunning { process_name: String },
    /// 服务是否运行
    ServiceRunning { service_name: String },
}

/// 告警级别
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[allow(dead_code)] // Info 级别预留用于未来通知功能
pub enum AlertSeverity {
    Info,
    Warning,
    Critical,
}

impl std::fmt::Display for AlertSeverity {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            AlertSeverity::Info => write!(f, "INFO"),
            AlertSeverity::Warning => write!(f, "WARNING"),
            AlertSeverity::Critical => write!(f, "CRITICAL"),
        }
    }
}

/// 告警消息
#[derive(Debug, Clone)]
pub struct Alert {
    #[allow(dead_code)] // 供告警日志和 UI 显示使用
    pub rule_name: String,
    /// 告警级别
    pub severity: AlertSeverity,
    /// 当前值
    pub current_value: f64,
    /// 阈值
    pub threshold: f64,
    /// 告警时间
    pub timestamp: chrono::DateTime<chrono::Utc>,
    /// 告警消息
    pub message: String,
}

impl Watchdog {
    #[allow(dead_code)] // 供 watchdog 启动命令调用
    pub fn new(alert_tx: Sender<Alert>) -> Self {
        let (shutdown_tx, _) = watch::channel(false);
        Self {
            rules: Vec::new(),
            alert_tx,
            running: Arc::new(std::sync::atomic::AtomicBool::new(false)),
            shutdown_tx,
        }
    }

    /// 创建带默认规则的 Watchdog
    pub fn with_default_rules(alert_tx: Sender<Alert>) -> Self {
        let (shutdown_tx, _) = watch::channel(false);
        let rules = vec![
            MonitorRule {
                name: "磁盘空间告警".to_string(),
                metric: MetricType::DiskUsage {
                    mount_point: "/".to_string(),
                },
                threshold: 80.0,
                interval_secs: 60,
                severity: AlertSeverity::Warning,
            },
            MonitorRule {
                name: "磁盘严重告警".to_string(),
                metric: MetricType::DiskUsage {
                    mount_point: "/".to_string(),
                },
                threshold: 95.0,
                interval_secs: 60,
                severity: AlertSeverity::Critical,
            },
            MonitorRule {
                name: "内存告警".to_string(),
                metric: MetricType::MemoryUsage,
                threshold: 85.0,
                interval_secs: 30,
                severity: AlertSeverity::Warning,
            },
            MonitorRule {
                name: "内存严重告警".to_string(),
                metric: MetricType::MemoryUsage,
                threshold: 95.0,
                interval_secs: 30,
                severity: AlertSeverity::Critical,
            },
        ];

        Self {
            rules,
            alert_tx,
            running: Arc::new(std::sync::atomic::AtomicBool::new(false)),
            shutdown_tx,
        }
    }

    #[allow(dead_code)] // 供动态添加自定义监控规则使用
    pub fn add_rule(&mut self, rule: MonitorRule) {
        self.rules.push(rule);
    }

    /// 启动后台监控
    pub fn start(&self) {
        self.running.store(true, std::sync::atomic::Ordering::SeqCst);

        for rule in self.rules.clone() {
            let tx = self.alert_tx.clone();
            let running = self.running.clone();
            let mut shutdown_rx = self.shutdown_tx.subscribe();

            tokio::spawn(async move {
                let mut timer = interval(Duration::from_secs(rule.interval_secs));

                loop {
                    tokio::select! {
                        // 关闭信号：立即退出
                        _ = shutdown_rx.changed() => {
                            tracing::debug!("监控规则 '{}' 收到关闭信号", rule.name);
                            break;
                        }
                        // 定时检查
                        _ = timer.tick() => {
                            if !running.load(std::sync::atomic::Ordering::SeqCst) {
                                break;
                            }
                            if let Ok(value) = check_metric(&rule.metric) {
                                if should_alert(value, rule.threshold, &rule.metric) {
                                    let alert = Alert {
                                        rule_name: rule.name.clone(),
                                        severity: rule.severity,
                                        current_value: value,
                                        threshold: rule.threshold,
                                        timestamp: chrono::Utc::now(),
                                        message: format_alert_message(&rule, value),
                                    };
                                    if tx.send(alert).await.is_err() {
                                        tracing::warn!("告警通道已关闭");
                                        break;
                                    }
                                }
                            }
                        }
                    }
                }

                tracing::debug!("监控规则 '{}' 已停止", rule.name);
            });
        }

        tracing::info!("Watchdog 监控已启动，规则数: {}", self.rules.len());
    }

    /// 停止监控（立即通知所有监控任务退出）
    pub fn stop(&self) {
        self.running.store(false, std::sync::atomic::Ordering::SeqCst);
        // 发送关闭信号，让所有 spawn 的任务立即退出 select!
        let _ = self.shutdown_tx.send(true);
        tracing::info!("Watchdog 监控已停止");
    }
}

/// 检查指标当前值
fn check_metric(metric: &MetricType) -> Result<f64> {
    match metric {
        MetricType::DiskUsage { mount_point } => {
            // 使用 df 命令获取磁盘使用率
            let output = std::process::Command::new("df")
                .arg("-h")
                .arg(mount_point)
                .output()?;

            let stdout = String::from_utf8_lossy(&output.stdout);
            // 解析 df 输出：Filesystem Size Used Avail Use% Mountedon
            for line in stdout.lines().skip(1) {
                let parts: Vec<&str> = line.split_whitespace().collect();
                if parts.len() >= 5 {
                    let use_percent = parts[4].replace('%', "");
                    if let Ok(value) = use_percent.parse::<f64>() {
                        return Ok(value);
                    }
                }
            }
            Err(anyhow::anyhow!("无法解析磁盘使用率"))
        }
        MetricType::MemoryUsage => {
            // 使用 free 命令获取内存使用率
            let output = std::process::Command::new("free")
                .arg("-m")
                .output()?;

            let stdout = String::from_utf8_lossy(&output.stdout);
            // 解析 free 输出
            for line in stdout.lines() {
                if line.starts_with("Mem:") {
                    let parts: Vec<&str> = line.split_whitespace().collect();
                    if parts.len() >= 3 {
                        let total: f64 = parts[1].parse()?;
                        let used: f64 = parts[2].parse()?;
                        if total > 0.0 {
                            return Ok((used / total) * 100.0);
                        }
                    }
                }
            }
            Err(anyhow::anyhow!("无法解析内存使用率"))
        }
        MetricType::CpuUsage => {
            // 简化实现：使用 top 命令（实际应该用更精确的方法）
            let output = std::process::Command::new("sh")
                .arg("-c")
                .arg("top -bn1 | grep 'Cpu(s)' | awk '{print $2}'")
                .output()?;

            let stdout = String::from_utf8_lossy(&output.stdout).trim().to_string();
            stdout.parse::<f64>().map_err(|e| anyhow::anyhow!("CPU 解析失败: {}", e))
        }
        MetricType::ProcessRunning { process_name } => {
            // 检查进程是否存在
            let output = std::process::Command::new("pgrep")
                .arg("-x")
                .arg(process_name)
                .output()?;

            // 存在返回 1，不存在返回 0
            Ok(if output.status.success() { 1.0 } else { 0.0 })
        }
        MetricType::ServiceRunning { service_name } => {
            // 检查服务状态
            let output = std::process::Command::new("systemctl")
                .arg("is-active")
                .arg(service_name)
                .output()?;

            let status = String::from_utf8_lossy(&output.stdout).trim().to_string();
            Ok(if status == "active" { 1.0 } else { 0.0 })
        }
    }
}

/// 判断是否需要发送告警
fn should_alert(value: f64, threshold: f64, metric: &MetricType) -> bool {
    match metric {
        // 使用率类指标：超过阈值告警
        MetricType::DiskUsage { .. } |
        MetricType::MemoryUsage |
        MetricType::CpuUsage => value >= threshold,
        // 存在类指标：低于阈值告警（期望值为 1）
        MetricType::ProcessRunning { .. } |
        MetricType::ServiceRunning { .. } => value < threshold,
    }
}

/// 格式化告警消息
fn format_alert_message(rule: &MonitorRule, value: f64) -> String {
    match &rule.metric {
        MetricType::DiskUsage { mount_point } => {
            format!(
                "磁盘 {} 使用率已达 {:.1}%，超过阈值 {:.1}%",
                mount_point, value, rule.threshold
            )
        }
        MetricType::MemoryUsage => {
            format!(
                "内存使用率已达 {:.1}%，超过阈值 {:.1}%",
                value, rule.threshold
            )
        }
        MetricType::CpuUsage => {
            format!(
                "CPU 使用率已达 {:.1}%，超过阈值 {:.1}%",
                value, rule.threshold
            )
        }
        MetricType::ProcessRunning { process_name } => {
            format!(
                "进程 '{}' 已停止运行",
                process_name
            )
        }
        MetricType::ServiceRunning { service_name } => {
            format!(
                "服务 '{}' 已停止运行",
                service_name
            )
        }
    }
}

/// 告警处理器：接收并处理告警消息
pub struct AlertHandler {
    pub alert_rx: Receiver<Alert>,
}

impl AlertHandler {
    pub fn new(alert_rx: Receiver<Alert>) -> Self {
        Self { alert_rx }
    }

    /// 启动告警处理循环
    pub async fn run(&mut self) {
        while let Some(alert) = self.alert_rx.recv().await {
            self.handle_alert(alert);
        }
        tracing::debug!("告警处理器已停止");
    }

    /// 处理单个告警
    fn handle_alert(&self, alert: Alert) {
        let severity_icon = match alert.severity {
            AlertSeverity::Info => "ℹ️",
            AlertSeverity::Warning => "⚠️",
            AlertSeverity::Critical => "🚨",
        };

        println!(
            "\n{} [{}] {}\n   当前值: {:.1}%，阈值: {:.1}%\n   时间: {}\n",
            severity_icon,
            alert.severity,
            alert.message,
            alert.current_value,
            alert.threshold,
            alert.timestamp.format("%H:%M:%S")
        );

        // 记录到日志
        match alert.severity {
            AlertSeverity::Critical => tracing::error!("{}", alert.message),
            AlertSeverity::Warning => tracing::warn!("{}", alert.message),
            AlertSeverity::Info => tracing::info!("{}", alert.message),
        }
    }
}

/// 创建 Watchdog 系统（包含发送器和接收器）
pub fn create_watchdog_system() -> (Watchdog, AlertHandler) {
    let (alert_tx, alert_rx) = channel(100);

    let watchdog = Watchdog::with_default_rules(alert_tx);
    let handler = AlertHandler::new(alert_rx);

    (watchdog, handler)
}