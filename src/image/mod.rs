mod security;

pub use security::{ImageAuditRecord, ImageSecurityScanner};

use anyhow::Result;
use base64::{engine::general_purpose::STANDARD, Engine};
use std::path::Path;

/// 图片信息
#[derive(Debug, Clone)]
pub struct ImageInfo {
    pub path: Option<String>,
    pub base64_data: String,
    pub mime_type: String,
    pub original_size: usize,
}

/// 图片信息
#[derive(Debug, Clone)]
pub struct PreparedUserInput {
    pub clean_input: String,
    pub images: Vec<ImageInfo>,
    pub notices: Vec<String>,
}

/// 图片处理器
pub struct ImageProcessor {
    /// 支持的图片格式
    supported_formats: Vec<&'static str>,
}

impl Default for ImageProcessor {
    fn default() -> Self {
        Self::new()
    }
}

impl ImageProcessor {
    pub fn new() -> Self {
        Self {
            supported_formats: vec!["png", "jpg", "jpeg", "gif", "webp", "bmp"],
        }
    }

    /// 从文件路径加载图片
    pub fn load_from_path(&self, path: &str) -> Result<ImageInfo> {
        let file_path = Path::new(path);

        // 检查文件是否存在
        if !file_path.exists() {
            return Err(anyhow::anyhow!("图片文件不存在: {}", path));
        }

        // 检查格式
        let extension = file_path
            .extension()
            .and_then(|e| e.to_str())
            .map(|e| e.to_lowercase())
            .unwrap_or_else(|| "unknown".to_string());

        if !self.supported_formats.contains(&extension.as_str()) {
            return Err(anyhow::anyhow!(
                "不支持图片格式: {}。支持: {}",
                extension,
                self.supported_formats.join(", ")
            ));
        }

        // 检查大小（限制 20MB）
        let metadata = std::fs::metadata(file_path)?;
        let size = metadata.len();
        if size > 20 * 1024 * 1024 {
            return Err(anyhow::anyhow!(
                "图片文件过大（{}MB），最大支持 20MB",
                size / 1024 / 1024
            ));
        }

        // 读取并编码
        let content = std::fs::read(file_path)?;
        let base64_data = STANDARD.encode(&content);

        // 确定 MIME 类型
        let mime_type = self.get_mime_type(&extension);

        Ok(ImageInfo {
            path: Some(path.to_string()),
            base64_data,
            mime_type,
            original_size: size as usize,
        })
    }

    /// 从 Base64 数据创建图片信息
    pub fn from_base64(&self, base64_data: &str, mime_type: &str) -> Result<ImageInfo> {
        // 验证 Base64
        let decoded = STANDARD.decode(base64_data)
            .map_err(|e| anyhow::anyhow!("Base64 解码失败: {}", e))?;

        Ok(ImageInfo {
            path: None,
            base64_data: base64_data.to_string(),
            mime_type: mime_type.to_string(),
            original_size: decoded.len(),
        })
    }

    /// 从用户输入中提取图片并清理文本
    pub fn prepare_user_input(&self, detector: &Iterm2Detector, input: &str) -> PreparedUserInput {
        if let Some(base64_data) = detector.detect_image_paste(input) {
            let clean_input = detector.clean_input(input);
            let mut notices = vec!["📷 检测到图片粘贴（iTerm2）".to_string()];
            let size_estimate = base64_data.len() / 4 * 3;
            let image_info = self
                .from_base64(&base64_data, "image/png")
                .unwrap_or_else(|_| ImageInfo {
                    path: None,
                    base64_data,
                    mime_type: "image/png".to_string(),
                    original_size: size_estimate,
                });
            notices.push(self.display_summary(&image_info));
            return PreparedUserInput {
                clean_input,
                images: vec![image_info],
                notices,
            };
        }

        let image_paths = self.extract_image_paths(input);
        if image_paths.is_empty() {
            return PreparedUserInput {
                clean_input: input.to_string(),
                images: Vec::new(),
                notices: Vec::new(),
            };
        }

        let mut images = Vec::new();
        let mut notices = Vec::new();
        for path in &image_paths {
            match self.load_from_path(path) {
                Ok(image) => {
                    notices.push(format!("📷 加载图片: {}", path));
                    notices.push(self.display_summary(&image));
                    images.push(image);
                }
                Err(err) => {
                    notices.push(format!("⚠️  图片加载失败: {} - {}", path, err));
                }
            }
        }

        let clean_input = image_paths.iter().fold(input.to_string(), |acc, path| {
            acc.replace(path, "")
                .replace(&format!("图片: {}", path), "")
                .replace(&format!("图片路径: {}", path), "")
        });

        PreparedUserInput {
            clean_input: clean_input.trim().to_string(),
            images,
            notices,
        }
    }

    /// 从用户输入中提取图片路径
    /// 支持：/path/to/image.png 或 "图片路径: /path/to/image.png"
    pub fn extract_image_paths(&self, input: &str) -> Vec<String> {
        let mut paths = Vec::new();

        // 匹配模式：单独的文件路径或 "图片: xxx" 格式
        for word in input.split_whitespace() {
            // 检查是否是图片路径
            if self.is_image_path(word) {
                paths.push(word.to_string());
            }
        }

        // 特殊格式：图片: /path 或 图片路径: /path
        if input.contains("图片:") || input.contains("图片路径:") {
            for line in input.lines() {
                if line.contains("图片:") || line.contains("图片路径:") {
                    let parts: Vec<&str> = line.splitn(2, ':').collect();
                    if parts.len() == 2 {
                        let path = parts[1].trim();
                        if self.is_image_path(path) {
                            paths.push(path.to_string());
                        }
                    }
                }
            }
        }

        paths
    }

    /// 检查是否是图片路径
    fn is_image_path(&self, path: &str) -> bool {
        let path_lower = path.to_lowercase();
        self.supported_formats.iter().any(|fmt| path_lower.ends_with(fmt))
    }

    /// 获取 MIME 类型
    fn get_mime_type(&self, extension: &str) -> String {
        match extension {
            "png" => "image/png",
            "jpg" | "jpeg" => "image/jpeg",
            "gif" => "image/gif",
            "webp" => "image/webp",
            "bmp" => "image/bmp",
            _ => "image/png", // 默认
        }.to_string()
    }

    #[allow(dead_code)] // 供多模态 LLM 调用时格式化图像内容
    pub fn to_anthropic_content(&self, image: &ImageInfo) -> serde_json::Value {
        serde_json::json!({
            "type": "image",
            "source": {
                "type": "base64",
                "media_type": image.mime_type,
                "data": image.base64_data
            }
        })
    }

    #[allow(dead_code)] // 供 OpenAI-compatible provider 格式化图像内容
    pub fn to_openai_content(&self, image: &ImageInfo) -> serde_json::Value {
        serde_json::json!({
            "type": "image_url",
            "image_url": {
                "url": format!("data:{};base64,{}", image.mime_type, image.base64_data)
            }
        })
    }

    /// 显示图片摘要
    pub fn display_summary(&self, image: &ImageInfo) -> String {
        let size_kb = image.original_size / 1024;
        let base64_kb = image.base64_data.len() / 1024;

        format!(
            "📷 图片信息：\n   格式: {}\n   原始大小: {}KB\n   Base64大小: {}KB\n   来源: {}",
            image.mime_type,
            size_kb,
            base64_kb,
            image.path.as_deref().unwrap_or("粘贴")
        )
    }
}

/// iTerm2 图片粘贴检测器
pub struct Iterm2Detector {
    /// iTerm2 图片粘贴标记
    iterm2_marker: &'static str,
}

impl Default for Iterm2Detector {
    fn default() -> Self {
        Self::new()
    }
}

impl Iterm2Detector {
    pub fn new() -> Self {
        // iTerm2 使用特殊的 OSC 序列标记图片粘贴
        // 实际格式是：ESC ] 1337 ; File = [options] : base64 data BEL
        Self {
            iterm2_marker: "\x1b]1337;File=",
        }
    }

    /// 检测输入是否包含 iTerm2 图片粘贴
    pub fn detect_image_paste(&self, input: &str) -> Option<String> {
        // 检查 iTerm2 标记
        if input.contains(self.iterm2_marker) {
            // 提取 base64 数据（简化处理）
            // iTerm2 格式：ESC]1337;File=inline=1;width=100%:base64_dataBEL
            let start = input.find(self.iterm2_marker)?;
            let rest = &input[start + self.iterm2_marker.len()..];

            // 找到冒号后的数据
            if let Some(colon_pos) = rest.find(':') {
                let data_part = &rest[colon_pos + 1..];
                // 找到结束标记（BEL = \x07 或 ST = \x1b\\）
                let end_pos = data_part.find('\x07')
                    .or_else(|| data_part.find("\x1b\\"))
                    .unwrap_or(data_part.len());

                let base64_data = &data_part[..end_pos];
                return Some(base64_data.to_string());
            }
        }
        None
    }

    /// 清理 iTerm2 标记，保留文本内容
    pub fn clean_input(&self, input: &str) -> String {
        if input.contains(self.iterm2_marker) {
            // 移除 iTerm2 图片标记序列
            let mut result = input.to_string();
            while let Some(start) = result.find(self.iterm2_marker) {
                // 找到结束标记
                let rest = &result[start + self.iterm2_marker.len()..];
                let end_pos = rest.find('\x07')
                    .or_else(|| rest.find("\x1b\\"))
                    .unwrap_or(rest.len());
                result.replace_range(start..start + self.iterm2_marker.len() + end_pos + 1, "");
            }
            result.trim().to_string()
        } else {
            input.to_string()
        }
    }
}