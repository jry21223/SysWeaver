use anyhow::Result;
use async_trait::async_trait;
use serde_json::json;
use tokio::process::Command;

use super::Tool;
use crate::types::tool::ToolResult;

/// 用户管理工具 — 创建/删除/查询/修改系统用户
pub struct UserTool;

impl UserTool {
    pub fn new() -> Self {
        Self
    }
}

#[async_trait]
impl Tool for UserTool {
    fn name(&self) -> &str {
        "user.manage"
    }

    fn description(&self) -> &str {
        "管理系统用户：列出用户、查询用户信息、创建普通用户、删除用户。\
         创建用户为中等风险，删除用户为高风险，会经过风险审查和用户确认。"
    }

    fn schema(&self) -> serde_json::Value {
        json!({
            "type": "object",
            "properties": {
                "action": {
                    "type": "string",
                    "enum": ["list", "info", "create", "delete", "passwd"],
                    "description": "操作：list=列出用户, info=查询详情, create=创建用户, delete=删除用户, passwd=修改密码"
                },
                "username": {
                    "type": "string",
                    "description": "用户名（create/delete/info/passwd 时必填）"
                },
                "groups": {
                    "type": "array",
                    "items": { "type": "string" },
                    "description": "附加用户组（create 时可选，如 ['sudo', 'docker']）"
                },
                "shell": {
                    "type": "string",
                    "description": "用户 Shell（create 时可选，默认 /bin/bash）"
                },
                "comment": {
                    "type": "string",
                    "description": "用户注释/全名（create 时可选）"
                }
            },
            "required": ["action"]
        })
    }

    async fn execute(&self, args: &serde_json::Value, dry_run: bool) -> Result<ToolResult> {
        let action = args["action"]
            .as_str()
            .ok_or_else(|| anyhow::anyhow!("缺少 action 参数"))?;

        match action {
            "list" => {
                if dry_run {
                    return Ok(ToolResult::dry_run_preview(self.name(), "将列出系统中的所有普通用户"));
                }
                let output = list_users().await;
                Ok(ToolResult::success(self.name(), &output, 0))
            }
            "info" => {
                let username = args["username"]
                    .as_str()
                    .ok_or_else(|| anyhow::anyhow!("info 操作需要 username"))?;
                validate_username(username)?;

                if dry_run {
                    return Ok(ToolResult::dry_run_preview(self.name(), &format!("将查询用户 '{}' 的信息", username)));
                }
                let output = get_user_info(username).await;
                Ok(ToolResult::success(self.name(), &output, 0))
            }
            "create" => {
                let username = args["username"]
                    .as_str()
                    .ok_or_else(|| anyhow::anyhow!("create 操作需要 username"))?;
                validate_username(username)?;

                if dry_run {
                    let groups = args["groups"]
                        .as_array()
                        .map(|g| g.iter().filter_map(|v| v.as_str()).collect::<Vec<_>>().join(","))
                        .unwrap_or_default();
                    return Ok(ToolResult::dry_run_preview(
                        self.name(),
                        &format!("将创建用户 '{}' (组: {})", username, if groups.is_empty() { "默认" } else { &groups }),
                    ));
                }
                create_user(self.name(), username, args).await
            }
            "delete" => {
                let username = args["username"]
                    .as_str()
                    .ok_or_else(|| anyhow::anyhow!("delete 操作需要 username"))?;
                validate_username(username)?;

                if dry_run {
                    return Ok(ToolResult::dry_run_preview(
                        self.name(),
                        &format!("将删除用户 '{}' 及其家目录（不可逆）", username),
                    ));
                }
                delete_user(self.name(), username).await
            }
            _ => Err(anyhow::anyhow!("不支持的操作: {}", action)),
        }
    }
}

fn validate_username(name: &str) -> Result<()> {
    if name.len() > 32 {
        return Err(anyhow::anyhow!("用户名过长（最多 32 字符）"));
    }
    if name.chars().all(|c| c.is_ascii_alphanumeric() || matches!(c, '_' | '-' | '.')) {
        Ok(())
    } else {
        Err(anyhow::anyhow!("用户名包含非法字符，只允许字母、数字、下划线、连字符"))
    }
}

async fn list_users() -> String {
    // 列出 UID >= 1000 的普通用户（Linux 惯例）
    let output = Command::new("sh")
        .arg("-c")
        .arg(if cfg!(target_os = "macos") {
            "dscl . list /Users | grep -v '^_'"
        } else {
            "awk -F: '$3>=1000 && $1!=\"nobody\" {print $1\"\t\"$3\"\t\"$6\"\t\"$7}' /etc/passwd"
        })
        .output()
        .await
        .map(|o| String::from_utf8_lossy(&o.stdout).to_string())
        .unwrap_or_else(|e| format!("ERROR: {}", e));

    if output.trim().is_empty() {
        "当前系统没有 UID >= 1000 的普通用户".to_string()
    } else {
        format!("用户名\tUID\t家目录\tShell\n{}", output)
    }
}

async fn get_user_info(username: &str) -> String {
    let id_out = Command::new("id")
        .arg(username)
        .output()
        .await
        .map(|o| String::from_utf8_lossy(&o.stdout).trim().to_string())
        .unwrap_or_else(|e| format!("id 查询失败: {}", e));

    let passwd_out = Command::new("sh")
        .arg("-c")
        .arg(format!("grep ^{}': /etc/passwd || true", username))
        .output()
        .await
        .map(|o| String::from_utf8_lossy(&o.stdout).trim().to_string())
        .unwrap_or_default();

    let last_out = Command::new("last")
        .args([username, "-n", "3"])
        .output()
        .await
        .map(|o| {
            String::from_utf8_lossy(&o.stdout)
                .lines()
                .take(3)
                .collect::<Vec<_>>()
                .join("\n")
        })
        .unwrap_or_default();

    format!(
        "用户信息：{}\n/etc/passwd：{}\n最近登录：\n{}",
        id_out, passwd_out, last_out
    )
}

async fn create_user(tool_name: &str, username: &str, args: &serde_json::Value) -> Result<ToolResult> {
    let shell = args["shell"].as_str().unwrap_or("/bin/bash");
    let comment = args["comment"].as_str().unwrap_or(username);
    let groups: Vec<&str> = args["groups"]
        .as_array()
        .map(|g| g.iter().filter_map(|v| v.as_str()).collect())
        .unwrap_or_default();

    let mut cmd_args = vec![
        "-m".to_string(),
        "-s".to_string(),
        shell.to_string(),
        "-c".to_string(),
        comment.to_string(),
    ];

    if !groups.is_empty() {
        cmd_args.push("-G".to_string());
        cmd_args.push(groups.join(","));
    }

    cmd_args.push(username.to_string());

    let output = Command::new("useradd")
        .args(&cmd_args)
        .output()
        .await;

    match output {
        Ok(o) if o.status.success() => {
            let info = get_user_info(username).await;
            Ok(ToolResult::success(
                tool_name,
                &format!("用户 '{}' 创建成功\n\n{}", username, info),
                0,
            ))
        }
        Ok(o) => {
            let stderr = String::from_utf8_lossy(&o.stderr).to_string();
            Ok(ToolResult::failure(tool_name, &stderr, o.status.code().unwrap_or(-1)))
        }
        Err(e) => Ok(ToolResult::failure(tool_name, &e.to_string(), -1)),
    }
}

async fn delete_user(tool_name: &str, username: &str) -> Result<ToolResult> {
    // userdel -r 删除用户及家目录
    let output = Command::new("userdel")
        .args(["-r", username])
        .output()
        .await;

    match output {
        Ok(o) if o.status.success() => Ok(ToolResult::success(
            tool_name,
            &format!("用户 '{}' 已删除（包含家目录）", username),
            0,
        )),
        Ok(o) => {
            let stderr = String::from_utf8_lossy(&o.stderr).to_string();
            Ok(ToolResult::failure(tool_name, &stderr, o.status.code().unwrap_or(-1)))
        }
        Err(e) => Ok(ToolResult::failure(tool_name, &e.to_string(), -1)),
    }
}
