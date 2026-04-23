#!/bin/bash
# 编译 Agent Unix 多平台版本

set -euo pipefail

BLUE='\033[0;34m'; GREEN='\033[0;32m'; YELLOW='\033[1;33m'; NC='\033[0m'
PROJECT_DIR="$(cd "$(dirname "$0")" && pwd)"
RELEASES_DIR="$PROJECT_DIR/releases"

mkdir -p "$RELEASES_DIR"

echo -e "${BLUE}🔨 Agent Unix 多平台编译${NC}"
echo "项目: $PROJECT_DIR"
echo "输出: $RELEASES_DIR"
echo

# ─── macOS 编译 ─────────────────────────────────────────────────────────────
echo -e "${GREEN}[1/3]${NC} 📦 macOS arm64 (Apple Silicon)"
if cargo build --release --target aarch64-apple-darwin 2>&1 | tail -1 | grep -q "Finished"; then
    cp "$PROJECT_DIR/target/aarch64-apple-darwin/release/agent-unix" "$RELEASES_DIR/agent-unix-macos-arm64"
    chmod +x "$RELEASES_DIR/agent-unix-macos-arm64"
    SIZE=$(ls -lh "$RELEASES_DIR/agent-unix-macos-arm64" | awk '{print $5}')
    echo -e "   ${GREEN}✓${NC} $SIZE - agent-unix-macos-arm64"
else
    echo -e "   ${YELLOW}⚠${NC} 编译失败"
fi
echo

# ─── Linux 编译 ─────────────────────────────────────────────────────────────
echo -e "${GREEN}[2/3]${NC} 🐧 Linux x86_64"
echo "   使用 Docker 编译..."

BUILD_LOG=$(mktemp)
if docker run --rm -v "$PROJECT_DIR":/workspace -w /workspace rust:latest \
  sh -c 'rustup target add x86_64-unknown-linux-gnu && \
         cargo build --release --target x86_64-unknown-linux-gnu 2>&1' > "$BUILD_LOG" 2>&1; then
    if [ -f "$PROJECT_DIR/target/x86_64-unknown-linux-gnu/release/agent-unix" ]; then
        cp "$PROJECT_DIR/target/x86_64-unknown-linux-gnu/release/agent-unix" "$RELEASES_DIR/agent-unix-linux-x86_64"
        chmod +x "$RELEASES_DIR/agent-unix-linux-x86_64"
        SIZE=$(ls -lh "$RELEASES_DIR/agent-unix-linux-x86_64" | awk '{print $5}')
        echo -e "   ${GREEN}✓${NC} $SIZE - agent-unix-linux-x86_64"
    fi
else
    echo -e "   ${YELLOW}⚠${NC} 编译失败，查看日志:"
    tail -5 "$BUILD_LOG"
fi
rm -f "$BUILD_LOG"
echo

# ─── Windows 编译 ───────────────────────────────────────────────────────────
echo -e "${GREEN}[3/3]${NC} 🪟 Windows x86_64"
echo "   使用 Docker + mingw64 编译..."

BUILD_LOG=$(mktemp)
if docker run --rm -v "$PROJECT_DIR":/workspace -w /workspace rust:latest \
  sh -c 'rustup target add x86_64-pc-windows-gnu && \
         apt-get update >/dev/null 2>&1 && apt-get install -y mingw-w64 >/dev/null 2>&1 && \
         cargo build --release --target x86_64-pc-windows-gnu 2>&1' > "$BUILD_LOG" 2>&1; then
    if [ -f "$PROJECT_DIR/target/x86_64-pc-windows-gnu/release/agent-unix.exe" ]; then
        cp "$PROJECT_DIR/target/x86_64-pc-windows-gnu/release/agent-unix.exe" "$RELEASES_DIR/agent-unix-windows-x86_64.exe"
        chmod +x "$RELEASES_DIR/agent-unix-windows-x86_64.exe"
    elif [ -f "$PROJECT_DIR/target/x86_64-pc-windows-gnu/release/agent-unix" ]; then
        cp "$PROJECT_DIR/target/x86_64-pc-windows-gnu/release/agent-unix" "$RELEASES_DIR/agent-unix-windows-x86_64.exe"
        chmod +x "$RELEASES_DIR/agent-unix-windows-x86_64.exe"
    fi
    if [ -f "$RELEASES_DIR/agent-unix-windows-x86_64.exe" ]; then
        SIZE=$(ls -lh "$RELEASES_DIR/agent-unix-windows-x86_64.exe" | awk '{print $5}')
        echo -e "   ${GREEN}✓${NC} $SIZE - agent-unix-windows-x86_64.exe"
    fi
else
    echo -e "   ${YELLOW}⚠${NC} 编译失败，查看日志:"
    tail -10 "$BUILD_LOG"
fi
rm -f "$BUILD_LOG"
echo

# ─── 总结 ───────────────────────────────────────────────────────────────────
echo -e "${GREEN}📊 编译完成${NC}"
echo
if [ -d "$RELEASES_DIR" ] && [ "$(ls -A "$RELEASES_DIR")" ]; then
    echo "📦 可用的二进制文件:"
    ls -lh "$RELEASES_DIR" | tail -n +2 | awk '{printf "   %8s  %s\n", $5, $9}'
    echo
    echo "💾 总大小:"
    du -sh "$RELEASES_DIR"
else
    echo -e "${YELLOW}⚠${NC} 没有生成任何二进制文件"
fi
