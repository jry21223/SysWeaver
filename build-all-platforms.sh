#!/bin/bash
# 编译 Agent Unix for macOS, Linux, Windows

set -euo pipefail

BLUE='\033[0;34m'; GREEN='\033[0;32m'; NC='\033[0m'
PROJECT_DIR="$(cd "$(dirname "$0")" && pwd)"
RELEASES_DIR="$PROJECT_DIR/releases"

mkdir -p "$RELEASES_DIR"

echo -e "${BLUE}🔨 Agent Unix 多平台编译${NC}"
echo "目标目录: $RELEASES_DIR"
echo

# ─── macOS 编译 ─────────────────────────────────────────────────────────────
echo -e "${GREEN}[1/3]${NC} 编译 macOS 版本 (arm64)..."
cargo build --release --target aarch64-apple-darwin 2>&1 | grep -E "Finished|Compiling agent"
cp "$PROJECT_DIR/target/aarch64-apple-darwin/release/agent-unix" "$RELEASES_DIR/agent-unix-macos-arm64"
chmod +x "$RELEASES_DIR/agent-unix-macos-arm64"
echo -e "${GREEN}✓${NC} macOS 版本完成: $RELEASES_DIR/agent-unix-macos-arm64"
echo

# ─── Linux 编译（使用 Docker）───────────────────────────────────────────────
echo -e "${GREEN}[2/3]${NC} 编译 Linux 版本 (x86_64)..."
echo "  使用 Rust 官方 Docker 镜像交叉编译..."

# 使用 Rust 官方镜像
docker run --rm \
  -v "$PROJECT_DIR":/workspace \
  -w /workspace \
  rust:latest \
  bash -c "
    echo '  安装 Linux 目标...'
    rustup target add x86_64-unknown-linux-gnu
    echo '  编译中...'
    cargo build --release --target x86_64-unknown-linux-gnu 2>&1 | grep -E 'Finished|Compiling agent'
  " 2>&1 | tail -10

cp "$PROJECT_DIR/target/x86_64-unknown-linux-gnu/release/agent-unix" "$RELEASES_DIR/agent-unix-linux-x86_64"
chmod +x "$RELEASES_DIR/agent-unix-linux-x86_64"
echo -e "${GREEN}✓${NC} Linux 版本完成: $RELEASES_DIR/agent-unix-linux-x86_64"
echo

# ─── Windows 编译（使用 Docker + mingw）──────────────────────────────────────
echo -e "${GREEN}[3/3]${NC} 编译 Windows 版本 (x86_64)..."
echo "  使用 mingw64 交叉编译器..."

docker run --rm \
  -v "$PROJECT_DIR":/workspace \
  -w /workspace \
  rust:latest \
  bash -c "
    echo '  安装 Windows 目标...'
    rustup target add x86_64-pc-windows-gnu
    apt-get update -qq && apt-get install -y -qq mingw-w64 > /dev/null
    echo '  编译中...'
    cargo build --release --target x86_64-pc-windows-gnu 2>&1 | grep -E 'Finished|Compiling agent'
  " 2>&1 | tail -10

cp "$PROJECT_DIR/target/x86_64-pc-windows-gnu/release/agent-unix.exe" "$RELEASES_DIR/agent-unix-windows-x86_64.exe" 2>/dev/null || \
cp "$PROJECT_DIR/target/x86_64-pc-windows-gnu/release/agent-unix" "$RELEASES_DIR/agent-unix-windows-x86_64.exe"
chmod +x "$RELEASES_DIR/agent-unix-windows-x86_64.exe"
echo -e "${GREEN}✓${NC} Windows 版本完成: $RELEASES_DIR/agent-unix-windows-x86_64.exe"
echo

# ─── 显示编译结果 ───────────────────────────────────────────────────────────
echo -e "${GREEN}✅ 编译完成！${NC}"
echo
echo "📦 生成的二进制文件:"
ls -lh "$RELEASES_DIR/" | tail -n +2 | awk '{printf "   %s  %s  %s\n", $9, $5, ($1 ~ /x/) ? "可执行" : ""}'
echo
echo "💾 文件大小:"
du -sh "$RELEASES_DIR"/*
echo
echo "🚀 使用方法:"
echo "   macOS:   $RELEASES_DIR/agent-unix-macos-arm64 chat"
echo "   Linux:   $RELEASES_DIR/agent-unix-linux-x86_64 chat"
echo "   Windows: agent-unix-windows-x86_64.exe chat"
