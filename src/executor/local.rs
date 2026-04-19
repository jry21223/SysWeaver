use anyhow::Result;
use std::time::Instant;
use tokio::process::Command;
use tokio::time::{timeout, Duration};

/// 本地命令执行器（Phase 5：SSH 远程执行的基础，暂未集成到主流程）
#[allow(dead_code)]
pub struct LocalExecutor {
    pub timeout_secs: u64,
}

#[allow(dead_code)]
pub struct ExecResult {
    pub stdout: String,
    pub stderr: String,
    pub exit_code: i32,
    pub duration_ms: u64,
}

#[allow(dead_code)]
impl LocalExecutor {
    pub fn new() -> Self {
        Self { timeout_secs: 30 }
    }

    pub async fn run(&self, command: &str, working_dir: Option<&str>) -> Result<ExecResult> {
        let dir = working_dir.unwrap_or("/");
        let start = Instant::now();

        let result = timeout(
            Duration::from_secs(self.timeout_secs),
            Command::new("sh")
                .arg("-c")
                .arg(command)
                .current_dir(dir)
                .output(),
        )
        .await;

        let duration_ms = start.elapsed().as_millis() as u64;

        match result {
            Ok(Ok(output)) => Ok(ExecResult {
                stdout: String::from_utf8_lossy(&output.stdout).to_string(),
                stderr: String::from_utf8_lossy(&output.stderr).to_string(),
                exit_code: output.status.code().unwrap_or(-1),
                duration_ms,
            }),
            Ok(Err(e)) => Err(anyhow::anyhow!("执行失败: {}", e)),
            Err(_) => Err(anyhow::anyhow!("执行超时（{}秒）", self.timeout_secs)),
        }
    }
}

impl Default for LocalExecutor {
    fn default() -> Self {
        Self::new()
    }
}
