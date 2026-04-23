use anyhow::{Result, anyhow};
use std::process::Stdio;
use std::time::Instant;
use tokio::process::Command;

/// SSH remote execution configuration.
/// Uses the system `ssh` binary (no native ssh2 crate required by default).
#[derive(Debug, Clone)]
pub struct SshConfig {
    /// user@host
    pub host: String,
    /// SSH port (default 22)
    pub port: u16,
    /// Remote username
    pub user: String,
    /// Path to identity file (private key)
    pub identity_file: Option<String>,
}

impl SshConfig {
    /// Parse `user@host` or `user@host:port`.
    pub fn new(target: &str) -> Self {
        let (user_host, port) = if let Some((h, p)) = target.rsplit_once(':') {
            let port: u16 = p.parse().unwrap_or(22);
            (h, port)
        } else {
            (target, 22)
        };

        let (user, host) = if let Some((u, h)) = user_host.split_once('@') {
            (u.to_string(), h.to_string())
        } else {
            ("root".to_string(), user_host.to_string())
        };

        Self {
            host,
            port,
            user,
            identity_file: None,
        }
    }

    pub fn display(&self) -> String {
        format!("{}@{}:{}", self.user, self.host, self.port)
    }

    fn base_args(&self) -> Vec<String> {
        let mut args = vec![
            "-o".to_string(),
            "StrictHostKeyChecking=no".to_string(),
            "-o".to_string(),
            "ConnectTimeout=10".to_string(),
            "-p".to_string(),
            self.port.to_string(),
        ];
        if let Some(ref key) = self.identity_file {
            args.push("-i".to_string());
            args.push(key.clone());
        }
        args.push(format!("{}@{}", self.user, self.host));
        args
    }

    /// Test connectivity by running `true` on the remote host.
    pub async fn test_connection(&self) -> Result<String> {
        let mut cmd = Command::new("ssh");
        cmd.args(self.base_args());
        cmd.arg("true");
        cmd.stdout(Stdio::null()).stderr(Stdio::piped());

        let out = cmd.output().await?;
        if out.status.success() {
            Ok(format!("✅ SSH 连接成功: {}", self.display()))
        } else {
            let stderr = String::from_utf8_lossy(&out.stderr);
            Err(anyhow!("SSH 连接失败: {}", stderr.trim()))
        }
    }

    /// Execute a command on the remote host.
    /// Returns (stdout, stderr, exit_code, duration_ms).
    pub async fn exec(&self, command: &str) -> Result<(String, String, i32, u64)> {
        let start = Instant::now();
        let mut cmd = Command::new("ssh");
        cmd.args(self.base_args());
        cmd.arg(command);
        cmd.stdout(Stdio::piped()).stderr(Stdio::piped());

        let out = cmd.output().await?;
        let duration_ms = start.elapsed().as_millis() as u64;
        let exit_code = out.status.code().unwrap_or(-1);
        let stdout = String::from_utf8_lossy(&out.stdout).to_string();
        let stderr = String::from_utf8_lossy(&out.stderr).to_string();

        Ok((stdout, stderr, exit_code, duration_ms))
    }
}
