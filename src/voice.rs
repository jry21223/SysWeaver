use anyhow::Result;

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
}

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
