use anyhow::{Result, anyhow};

/// Voice engine for TTS/STT (stub — uses system `say`/`espeak` when available).
pub struct VoiceEngine {
    pub tts_enabled: bool,
    pub stt_enabled: bool,
}

impl VoiceEngine {
    pub fn new() -> Self {
        Self {
            tts_enabled: false,
            stt_enabled: false,
        }
    }

    pub fn status_summary(&self) -> String {
        format!(
            "🔊 TTS: {}  🎙️ STT: {}",
            if self.tts_enabled { "开启" } else { "关闭" },
            if self.stt_enabled { "开启" } else { "关闭" }
        )
    }

    /// Speak text via the system TTS command (`say` on macOS, `espeak` on Linux).
    pub async fn speak(&self, text: &str) -> Result<()> {
        if !self.tts_enabled {
            return Ok(());
        }

        // Strip ANSI escape codes / markdown before speaking
        let plain = strip_markup(text);

        #[cfg(target_os = "macos")]
        {
            tokio::process::Command::new("say")
                .arg(&plain)
                .output()
                .await
                .ok();
        }

        #[cfg(not(target_os = "macos"))]
        {
            // Try espeak; silently ignore if not installed
            tokio::process::Command::new("espeak")
                .arg("-v")
                .arg("zh")
                .arg(&plain)
                .output()
                .await
                .ok();
        }

        Ok(())
    }

    /// 开始录音，返回 (child_process, wav_path)。
    /// macOS 使用 `rec` (sox)，Linux 使用 `arecord`。
    /// 若工具未安装，返回 Err。
    pub fn start_recording() -> Result<(tokio::process::Child, String)> {
        let ts = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap_or_default()
            .as_millis();
        let path = format!("/tmp/jij_voice_{}.wav", ts);

        let child = if cfg!(target_os = "macos") {
            tokio::process::Command::new("rec")
                .args(["-r", "16000", "-c", "1", "-b", "16", path.as_str()])
                .stdout(std::process::Stdio::null())
                .stderr(std::process::Stdio::null())
                .spawn()
                .map_err(|e| anyhow!("rec 未安装或启动失败（需 brew install sox）: {}", e))?
        } else {
            tokio::process::Command::new("arecord")
                .args(["-r", "16000", "-c", "1", "-f", "S16_LE", path.as_str()])
                .stdout(std::process::Stdio::null())
                .stderr(std::process::Stdio::null())
                .spawn()
                .map_err(|e| anyhow!("arecord 未安装或启动失败（需 alsa-utils）: {}", e))?
        };

        Ok((child, path))
    }

    /// 停止录音，调用本地 whisper CLI 转写，返回转写文本。
    pub async fn stop_and_transcribe(
        mut child: tokio::process::Child,
        wav_path: &str,
    ) -> Result<String> {
        // 优雅终止录音进程
        child.kill().await.ok();
        let _ = child.wait().await;

        // whisper 默认输出到 <filename>.txt（放在与输入文件相同的目录）
        let output_dir = "/tmp";
        let stem = std::path::Path::new(wav_path)
            .file_stem()
            .and_then(|s| s.to_str())
            .unwrap_or("jij_voice");
        let txt_path = format!("{}/{}.txt", output_dir, stem);

        let whisper_out = tokio::process::Command::new("whisper")
            .args([
                wav_path,
                "--model", "tiny",
                "--language", "zh",
                "--output_format", "txt",
                "--output_dir", output_dir,
            ])
            .output()
            .await
            .map_err(|e| anyhow!("whisper 未安装: {e}（pip install openai-whisper）"))?;

        if !whisper_out.status.success() {
            let stderr = String::from_utf8_lossy(&whisper_out.stderr);
            let _ = tokio::fs::remove_file(wav_path).await;
            return Err(anyhow!("whisper 转写失败: {}", stderr.trim()));
        }

        let text = tokio::fs::read_to_string(&txt_path)
            .await
            .map_err(|e| anyhow!("读取转写结果失败: {}", e))?;

        // 清理临时文件
        let _ = tokio::fs::remove_file(wav_path).await;
        let _ = tokio::fs::remove_file(&txt_path).await;

        Ok(text.trim().to_string())
    }
}  // end impl VoiceEngine

impl Default for VoiceEngine {
    fn default() -> Self {
        Self::new()
    }
}

fn strip_markup(s: &str) -> String {
    // Remove common markdown and ANSI sequences for TTS readability
    let mut out = String::with_capacity(s.len());
    let mut chars = s.chars().peekable();
    while let Some(c) = chars.next() {
        match c {
            '\x1b' => {
                // ANSI escape — skip until 'm'
                for ch in chars.by_ref() {
                    if ch == 'm' {
                        break;
                    }
                }
            }
            '*' | '`' | '#' | '_' => {}
            _ => out.push(c),
        }
    }
    out
}
