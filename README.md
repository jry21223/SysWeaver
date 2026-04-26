# SysWeaver

> 用自然语言管理你的 Linux / macOS / Windows 系统

**SysWeaver** 是一个由 AI 驱动的操作系统智能代理，将自然语言指令转换为可控的系统操作，内置五级风险安全层，支持本地与 SSH 远程双模式。

---

## 功能概览

| 模块 | 功能 |
|------|------|
| **自然语言执行** | 将中文/英文指令翻译为系统操作，ReAct 多步规划 |
| **13 种工具** | Shell、文件、进程、服务、用户、包管理、网络、日志、Cron、健康检查等 |
| **五级风险安全层** | CRITICAL 直接拒绝，HIGH 弹窗确认，全程审计日志 |
| **TUI 界面** | 富文本交互终端，实时系统状态面板，鼠标支持 |
| **10+ LLM Provider** | Anthropic、OpenAI、DeepSeek、Groq、Kimi、GLM、百炼、MiniMax、OpenRouter |
| **SSH 远程模式** | 透明路由到远程服务器，Key/密码认证 |
| **Playbook 系统** | 内置 6 个常用 Playbook，支持录制与回放 |
| **反向解释模式** | 用 AI 解读 nginx/sshd/crontab 等配置文件 |
| **后台监控** | Watchdog 实时告警（磁盘/内存/CPU 阈值） |
| **API Key 自动导入** | 从 Claude Code / Codex CLI 配置文件一键授权导入 |
| **语音朗读** | TTS 朗读 Agent 回复（macOS/Linux） |
| **图片分析** | 粘贴/拖入图片，发给 LLM 进行视觉分析 |
| **Windows 全兼容** | 自动设置 UTF-8 控制台，中文不乱码 |

---

## 快速开始（1 分钟上手）

### 第一步：设置 API Key

**方式 A：自动导入（推荐）**

若已安装 Claude Code 或 Codex CLI，启动 `sysweaver chat` 时会自动检测其配置文件并询问是否导入 API Key，一键完成配置，无需手动填写。

**方式 B：环境变量**

```bash
# 使用 Anthropic Claude（推荐）
export ANTHROPIC_API_KEY=sk-ant-xxxxxxxx

# 或使用 OpenAI GPT
export OPENAI_API_KEY=sk-xxxxxxxx

# 或使用其他 Provider
export DEEPSEEK_API_KEY=xxxxxxxx
export GROQ_API_KEY=xxxxxxxx
```

SysWeaver 会自动检测已设置的 API Key 并选择对应 Provider，启动时显示检测结果。

**方式 C：交互式配置向导**

```bash
sysweaver config --setup
```

### 第二步：启动

```bash
sysweaver chat          # 交互式 TUI 对话（推荐）
sysweaver chat --no-tui # 纯文本 CLI 模式
sysweaver run "查看磁盘使用情况"  # 单条指令执行
```

### 启动时模式选择

运行 `sysweaver chat` 时会弹出运行模式选择：

```
  ▶ 选择运行模式

    [1] 💻 本地模式      — 管理当前机器（默认）
    [2] 🔗 SSH 远程模式  — 通过 SSH 连接到远程服务器
    [q] 🚪 退出

  选择 ›
```

跳过菜单直接启动：

```bash
sysweaver chat --local                                           # 强制本地，跳过菜单
sysweaver chat --no-prompt                                       # 脚本场景：完全不交互
sysweaver chat --ssh root@192.168.1.100                          # 直接进入 SSH 远程
sysweaver chat --ssh admin@host:2222 --ssh-key ~/.ssh/id_ed25519
```

---

## 安装

### 从源码编译

```bash
git clone <repo>
cd sysweaver
cargo build --release
# 二进制文件：target/release/sysweaver（Windows：sysweaver.exe）
```

### 一键安装到 PATH

```bash
cargo install --path .
```

> 自动安装到 `~/.cargo/bin/`，Rust 工具链已将该目录加入 PATH，安装后可直接运行 `sysweaver`。

或手动复制：

```bash
# macOS / Linux
sudo cp target/release/sysweaver /usr/local/bin/

# 或追加到 ~/.bashrc / ~/.zshrc
export PATH="$PATH:/path/to/sysweaver/target/release"
```

### Windows

```powershell
# 编译后将 sysweaver.exe 添加到 PATH，设置 API Key 后即可使用
$env:ANTHROPIC_API_KEY = "sk-ant-xxxxxxxx"
sysweaver chat
# SysWeaver 会自动设置控制台为 UTF-8，中文显示正常
```

---

## 配置详解

### 方式一：环境变量（推荐，无需配置文件）

| 环境变量 | 对应 Provider | 说明 |
|---------|-------------|------|
| `ANTHROPIC_API_KEY` | Anthropic Claude | 自动检测，优先级高 |
| `OPENAI_API_KEY` | OpenAI GPT | 自动检测 |
| `DEEPSEEK_API_KEY` | DeepSeek | 自动检测 |
| `GROQ_API_KEY` | Groq | 自动检测 |
| `KIMI_API_KEY` / `MOONSHOT_API_KEY` | Moonshot Kimi | 自动检测 |
| `GLM_API_KEY` / `BIGMODEL_API_KEY` | 智谱 GLM | 自动检测 |
| `BAILIAN_API_KEY` / `DASHSCOPE_API_KEY` | 阿里云百炼 | 自动检测 |
| `MINIMAX_API_KEY` | MiniMax | 自动检测 |
| `OPENROUTER_API_KEY` | OpenRouter | 自动检测 |
| `SYSWEAVER_LLM_API_KEY` | 通用（最高优先级） | 覆盖所有其他 Key |

自定义模型或端点：

```bash
export SYSWEAVER_LLM_PROVIDER=anthropic
export SYSWEAVER_LLM_BASE_URL=https://your-proxy.com
export SYSWEAVER_LLM_MODEL=claude-opus-4-7
```

### 方式二：配置文件

```bash
sysweaver config --setup              # 交互式向导（自动检测本地 AI 工具配置）
sysweaver config --show               # 查看当前配置
sysweaver config --list               # 列出所有支持的 Provider
sysweaver config --delete             # 删除配置文件
```

配置文件路径：`~/.sysweaver/config.json`（Windows: `%USERPROFILE%\.sysweaver\config.json`）

### 方式三：CLI 参数（最高优先级）

```bash
sysweaver --provider openai --model gpt-5.5 chat
sysweaver --provider anthropic --base-url https://my-proxy.com chat
```

### 优先级顺序

```
CLI 参数 > 配置文件 > SYSWEAVER_LLM_* 环境变量 > Provider 专属环境变量 > 默认值
```

---

## 支持的 Provider

| 名称 | `--provider` | 默认模型（旗舰）|
|------|-------------|-----------------|
| Anthropic | `anthropic` | `claude-opus-4-7` |
| OpenAI | `openai` | `gpt-5.5` |
| DeepSeek | `deepseek` | `deepseek-v4-pro` |
| Groq | `groq` | `llama-4-scout-17b-16e-instruct` |
| Moonshot Kimi | `kimi` | `kimi-k2.6` |
| 智谱 GLM | `glm` | `glm-5.1` |
| 阿里云百炼 | `bailian` | `qwen3.6-plus` |
| MiniMax | `minimax` | `minimax-m2.7` |
| OpenRouter | `openrouter` | `openai/gpt-5.5` |
| **Ollama（本地）** | `ollama` | `llama4:scout` |
| **LM Studio（本地）** | `lmstudio` | `local-model` |
| 自定义端点 | `custom` | 手动填写 |

---

## 常用命令

```bash
sysweaver chat                            # 交互式 TUI 对话（推荐）
sysweaver chat --no-tui                   # CLI 纯文本对话
sysweaver chat --ssh root@192.168.1.100   # SSH 远程模式
sysweaver run "查看磁盘使用情况"             # 单条指令执行
sysweaver run --dry-run "清理30天前日志"     # 预览模式（不实际执行）
sysweaver explain /etc/nginx/nginx.conf    # AI 解释配置文件
sysweaver watch --duration 120            # 后台系统监控 120 秒
sysweaver info                            # 显示系统信息与环境
sysweaver config --setup                  # 配置 LLM Provider
sysweaver playbooks                       # 列出所有 Playbook
```

---

## 对话内命令

### 斜线命令

| 命令 | 功能 |
|------|------|
| `/help` | 显示完整帮助 |
| `/status` | 实时系统状态 + 异常检测 |
| `/report` | 生成系统健康综合报告 |
| `/history` | 操作历史（最近 10 步，含可撤销标记） |
| `/undo` | 撤销上一步操作 |
| `/clear` | 清除对话上下文，重新开始 |
| `/summary` | AI 生成本次会话操作总结 |
| `/export [文件路径]` | 导出完整对话记录为 Markdown 报告 |
| `/playbook list [关键词]` | 列出/搜索 Playbook |
| `/playbook save <名称> [步数]` | 将最近操作保存为 Playbook |
| `/playbook run <名称>` | 执行 Playbook |
| `/voice tts` | 开启/关闭语音朗读（macOS/Linux） |
| `/voice status` | 查看语音功能状态 |
| `/voice off` | 关闭所有语音功能 |
| `/exit` | 退出 |

### TUI 快捷键

| 快捷键 | 功能 |
|--------|------|
| `Ctrl+Y` | 复制最后一条 Agent 回复到剪贴板 |
| `Ctrl+P / N` | 浏览历史输入 |
| `PgUp / PgDn` | 滚动对话区 |
| `↑ / ↓` | 逐行滚动 |
| `End` | 跳到最底部 |
| `Ctrl+C / Esc` | 退出 |

---

## 安全机制

所有操作在执行前经过五级风险分类，全程写入审计日志：

| 级别 | 处理方式 | 示例 |
|------|---------|------|
| **Critical** | 直接拒绝，不可确认 | `rm -rf /`、删除 `/etc/passwd`、`dd` 覆盖磁盘 |
| **High** | TUI 弹窗或终端要求输入 `yes` | 停止 SSH 服务、删除用户、`kill -9` PID 1 |
| **Medium** | `--mode safe` 下要求确认 | 新建用户、重启服务、修改系统配置 |
| **Low** | 直接执行，记录日志 | 查看文件内容、grep 搜索 |
| **Safe** | 直接执行 | 读取系统信息、查询磁盘/内存 |

```bash
sysweaver --mode safe chat   # 对 Medium 风险也要求确认
sysweaver --mode auto chat   # 自动模式（Medium 不询问）
```

审计日志路径：`~/.sysweaver/audit-YYYYMMDD.jsonl`

---

## 工具列表（13 种）

| 工具 | 功能 |
|------|------|
| `shell.exec` | 执行任意 Shell 命令（有超时保护，支持工作目录） |
| `file.read` | 读取文件（防路径穿越，限制系统敏感路径） |
| `file.write` | 写入/追加文件（原子操作） |
| `file.search` | 正则搜索文件内容，输出行号 |
| `system.info` | 查询磁盘/内存/CPU/进程/用户/网络/服务/OS |
| `process.manage` | 列出/查找/终止/信息查询进程 |
| `service.manage` | 启停/重启/查询系统服务（systemctl/launchctl） |
| `user.manage` | 创建/删除/锁定/解锁/修改用户账户 |
| `package.manage` | 自动检测包管理器，安装/卸载/更新软件包 |
| `log.tail` | Tail 日志文件，支持行数限制与关键词过滤 |
| `net.check` | DNS 解析、端口连通性、网络接口、监听端口 |
| `cron.manage` | 列出/添加/删除/启停 Cron 任务 |
| `health.check` | 系统全面健康检查（磁盘/内存/服务/安全基线） |

---

## Playbook 系统

Playbook 是可重复执行的操作序列。SysWeaver 内置 6 个常用模板：

| Playbook | 功能 |
|---------|------|
| `system-health-check` | 磁盘/内存/CPU/服务全面健康检查 |
| `install-web-server` | 安装并启用 Nginx |
| `cleanup-old-logs` | 归档清理 30 天前的旧日志（默认预览模式） |
| `security-audit` | 端口扫描、SUID 审计、登录历史检查 |
| `process-diagnosis` | CPU/内存占用 Top、僵尸进程检测 |
| `disk-space-analysis` | 大文件/目录分析 |

在对话中使用：

```
/playbook list                  # 列出所有 Playbook
/playbook run system-health-check
/playbook save my-deploy 5      # 保存最近 5 步操作为新 Playbook
```

用户自定义 Playbook 保存到 `~/.sysweaver/playbooks/`，项目级 Playbook 保存到 `./.sysweaver/playbooks/`。

---

## 反向解释模式

用 AI 解读系统配置文件和日志，支持以下文件类型：

| 类型 | 支持文件 |
|------|---------|
| Web 服务器 | `/etc/nginx/nginx.conf` |
| SSH | `/etc/ssh/sshd_config` |
| 日志 | `/var/log/syslog`、`/var/log/auth.log`、`nginx/access.log` |
| 系统 | `/etc/fstab`、`/etc/passwd`、`/etc/hostname`、`/etc/rsyslog.conf` |
| 定时任务 | crontab 文件 |

```bash
sysweaver explain /etc/nginx/nginx.conf
sysweaver explain /var/log/auth.log
sysweaver explain             # 不加参数则列出所有支持的文件类型
```

---

## 后台监控（Watchdog）

```bash
sysweaver watch --duration 120   # 启动后台监控 120 秒
```

监控规则（可在 TUI 模式下持续运行）：

| 指标 | 警告阈值 | 严重阈值 |
|------|---------|---------|
| 磁盘使用率 | > 80% | > 95% |
| 内存使用率 | > 85% | > 95% |

告警分三级（Info / Warning / Critical），TUI 模式下实时推送到对话区。

---

## SSH 远程模式

```bash
# 启动时选择模式，或直接指定
sysweaver --ssh root@192.168.1.100 chat
sysweaver --ssh admin@server.example.com:2222 --ssh-key ~/.ssh/id_ed25519 chat
```

连接建立后，所有 `shell.exec` 和 `system.info` 工具调用均透明路由到远程服务器，本地安全拦截规则依然有效。

---

## API Key 自动检测

当没有配置 API Key 时，SysWeaver 会自动扫描以下 AI 工具的配置文件：

| 工具 | 扫描路径 | 提取字段 |
|------|---------|---------|
| Claude Code | `~/.claude/settings.json` | `env.ANTHROPIC_API_KEY` / `env.ANTHROPIC_AUTH_TOKEN` |
| Codex CLI | `~/.codex/auth.json` | `api_key` / `OPENAI_API_KEY` |
| Codex CLI | `~/.codex/config.toml` | `api_key = "..."` |

**流程**：扫描时仅检查文件是否存在（不读取内容） → 向用户展示路径 → **用户授权后**才读取并提取 Key → 脱敏显示（`sk-ant-***`）→ 保存到 `~/.sysweaver/config.json`。

---

## 图片分析

在 iTerm2 等支持图片预览的终端中，可以直接将图片拖入或粘贴到输入框，SysWeaver 会自动识别并以 Base64 格式发送给支持视觉能力的 LLM（如 claude-opus-4-7、gpt-5.5）。

支持格式：PNG、JPG、JPEG、GIF、WebP、BMP（最大 20 MB）

---

## 语音功能

```
/voice tts      # 开启/关闭 Agent 回复语音朗读
/voice status   # 查看语音功能状态
/voice off      # 关闭所有语音功能
```

- **macOS**：使用系统内置 `say` 命令
- **Linux**：使用 `espeak`（需单独安装）

---

## 数据文件路径

| 文件 | macOS / Linux | Windows |
|------|--------------|---------|
| 配置文件 | `~/.sysweaver/config.json` | `%USERPROFILE%\.sysweaver\config.json` |
| 对话历史 | `~/.sysweaver/history.txt` | `%USERPROFILE%\.sysweaver\history.txt` |
| 审计日志 | `~/.sysweaver/audit-YYYYMMDD.jsonl` | `%USERPROFILE%\.sysweaver\audit-YYYYMMDD.jsonl` |
| 用户 Playbook | `~/.sysweaver/playbooks/` | `%USERPROFILE%\.sysweaver\playbooks\` |
| 项目 Playbook | `./.sysweaver/playbooks/` | `.\.sysweaver\playbooks\` |

---

## 使用示例

```
# 基础查询
查看磁盘使用情况
列出内存占用最高的 5 个进程
当前有哪些 SSH 连接？

# 高危操作（会触发确认）
停止 nginx 服务           → HIGH 风险，需确认
删除用户 testuser          → HIGH 风险，需确认
rm -rf /                  → CRITICAL，直接拒绝

# 多步骤任务（自动规划）
把 nginx 配置改到 8080 端口并重启
安装 Docker 并启动一个 nginx 容器
帮我创建一个每天凌晨 2 点清理日志的定时任务

# 环境感知
帮我装一个 Web 服务器    → 自动识别 OS，选 apt/yum/brew

# 预览模式
sysweaver run --dry-run "清理 30 天前的日志"
```

---

## AI Hackathon 2026 · 超聚变 αFUSION 预赛
