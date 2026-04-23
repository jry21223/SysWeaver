# Agent Unix 多平台编译与发布指南

## 📦 当前状态

已成功编译 **macOS arm64** 版本，文件位于：
```
releases/agent-unix-macos-arm64  (5.2M)
```

## 🚀 使用 GitHub Actions 编译所有平台

### 方法 1：自动编译（推荐）

1. **上传到 GitHub：**
   ```bash
   # 如果还未初始化 git
   cd /Users/jerry/Desktop/Hackathon/Hackathon/agent-unix
   git init
   git add .
   git commit -m "feat: Agent Unix 智能操作系统代理"
   git remote add origin https://github.com/<你的用户名>/agent-unix.git
   git push -u origin main
   ```

2. **创建标签并推送（触发编译）：**
   ```bash
   git tag v0.1.0
   git push origin v0.1.0
   ```

3. **查看编译进度：**
   - 访问 GitHub 仓库的 **Actions** 标签页
   - 观察 "多平台编译与发布" 工作流的执行
   - 等待编译完成（通常 5-10 分钟）

4. **下载二进制文件：**
   - 进入 **Releases** 页面
   - 找到 v0.1.0 版本
   - 下载对应平台的二进制文件

### 方法 2：本地手动编译

由于 Docker 交叉编译依赖问题，建议在每个平台上原生编译：

#### macOS（已完成）
```bash
# 已编译完毕，位于 releases/agent-unix-macos-arm64
./releases/agent-unix-macos-arm64 info
./releases/agent-unix-macos-arm64 chat
```

#### Linux（在 Linux 机器上执行）
```bash
rustup target add x86_64-unknown-linux-gnu
cargo build --release --target x86_64-unknown-linux-gnu
cp target/x86_64-unknown-linux-gnu/release/agent-unix ./agent-unix-linux-x86_64
```

#### Windows（在 Windows 机器上执行）
```powershell
rustup target add x86_64-pc-windows-msvc
cargo build --release --target x86_64-pc-windows-msvc
copy target\x86_64-pc-windows-msvc\release\agent-unix.exe agent-unix-windows-x86_64.exe
```

## 📋 GitHub Actions 工作流说明

### 工作流文件
`.github/workflows/release.yml` 定义了以下任务：

1. **编译阶段**（并行）
   - macOS arm64（GitHub 上的 macOS runner）
   - Linux x86_64（GitHub 上的 Ubuntu runner）
   - Windows x86_64（GitHub 上的 Windows runner）

2. **发布阶段**
   - 自动创建 GitHub Release
   - 上传所有平台的二进制文件
   - 生成版本说明（自动）

### 触发方式

```yaml
on:
  push:
    tags:
      - 'v*'         # 推送 v0.1.0、v1.0.0 等标签时自动触发
  workflow_dispatch: # 也可在 GitHub UI 手动触发
```

## 🔧 快速开始

### 1. 推送到 GitHub
```bash
cd agent-unix
git remote add origin https://github.com/<用户>/agent-unix.git
git push -u origin main
```

### 2. 创建发布版本
```bash
git tag v0.1.0
git push origin v0.1.0
```

### 3. 等待编译完成
- GitHub Actions 会自动编译三个平台版本
- 在 **Actions** 页面查看进度
- 编译完成后自动创建 **Release**

### 4. 分享二进制文件
所有用户可从 Release 页面下载：
- `agent-unix-macos-arm64` - macOS (Apple Silicon)
- `agent-unix-linux-x86_64` - Linux (x86_64)
- `agent-unix-windows-x86_64.exe` - Windows (x86_64)

## 📝 使用说明

### macOS
```bash
chmod +x agent-unix-macos-arm64
./agent-unix-macos-arm64 chat                    # TUI 对话
./agent-unix-macos-arm64 chat --no-tui           # CLI 对话
./agent-unix-macos-arm64 run "查看磁盘"          # 单条指令
./agent-unix-macos-arm64 config --setup          # 配置 LLM
```

### Linux
```bash
chmod +x agent-unix-linux-x86_64
./agent-unix-linux-x86_64 chat                   # TUI 对话
./agent-unix-linux-x86_64 run "查看磁盘"         # 单条指令
```

### Windows
```cmd
agent-unix-windows-x86_64.exe chat               # CLI 对话
agent-unix-windows-x86_64.exe run "查看磁盘"     # 单条指令
agent-unix-windows-x86_64.exe config --setup     # 配置 LLM
```

## 🔐 环境要求

### 系统要求
- **macOS**: Sonoma 或更新版本（arm64）
- **Linux**: Ubuntu 20.04+，Debian 11+，CentOS 8+ 等
- **Windows**: Windows 10 或更新版本（需要 C++ 运行库）

### LLM 配置
三个平台版本都需要配置 LLM Provider：

```bash
# 交互式配置
./agent-unix-* config --setup

# 或使用环境变量
export ANTHROPIC_API_KEY=sk-ant-xxx
./agent-unix-* chat
```

## 📊 编译信息

### 文件大小参考
- macOS arm64: ~5.2M（strip 后）
- Linux x86_64: ~8-10M（预计）
- Windows x86_64: ~9-12M（预计）

### 构建时间（GitHub Actions）
- 每个平台约 3-5 分钟
- 并行编译，总耗时约 5-10 分钟

### 缓存策略
GitHub Actions 中已启用：
- Cargo 注册表缓存
- Cargo git 缓存
- 构建目录缓存
- 加速后续编译

## 🐛 故障排除

### 问题 1：Workflow 不执行
**解决：** 确保 `.github/workflows/release.yml` 已推送到 main 分支

### 问题 2：编译失败
**解决：** 
- 检查 GitHub Actions logs
- 确保 Rust 版本兼容（1.75+）
- 清理本地缓存后重试

### 问题 3：Release 页面不显示
**解决：** 检查 GitHub Token 权限，确保有写入 Release 的权限

## 📚 相关文件

- `.github/workflows/release.yml` - GitHub Actions 工作流
- `Cargo.toml` - Rust 项目配置
- `RELEASE.md` - 本文档

---

**提示：** 首次创建 tag 并推送后，GitHub Actions 会自动构建并发布，无需手动干预。
