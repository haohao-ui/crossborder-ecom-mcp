#!/usr/bin/env bash
set -euo pipefail

# ==============================================================================
# crossborder-ecom-mcp 一键安装脚本
# 项目地址: https://github.com/haohao-ui/crossborder-ecom-mcp
# ==============================================================================

REPO="haohao-ui/crossborder-ecom-mcp"
BINARY_NAME="crossborder-ecom-mcp"
INSTALL_DIR="${INSTALL_DIR:-$HOME/.local/bin}"

RED='\033[0;31m'
GREEN='\033[0;32m'
BLUE='\033[0;34m'
YELLOW='\033[1;33m'
NC='\033[0m' # No Color

info() {
    echo -e "${BLUE}[INFO]${NC} $1"
}

success() {
    echo -e "${GREEN}[SUCCESS]${NC} $1"
}

warn() {
    echo -e "${YELLOW}[WARN]${NC} $1"
}

error() {
    echo -e "${RED}[ERROR]${NC} $1" >&2
}

# 1. 架构与操作系统识别
detect_platform() {
    OS="$(uname -s)"
    ARCH="$(uname -m)"

    case "$OS" in
        Linux)
            PLATFORM_OS="linux"
            ;;
        Darwin)
            PLATFORM_OS="darwin"
            ;;
        MINGW*|MSYS*|CYGWIN*)
            PLATFORM_OS="windows"
            ;;
        *)
            error "不支持的操作系统: $OS"
            exit 1
            ;;
    esac

    case "$ARCH" in
        x86_64|amd64)
            PLATFORM_ARCH="amd64"
            ;;
        arm64|aarch64)
            PLATFORM_ARCH="arm64"
            ;;
        *)
            error "不支持的硬件架构: $ARCH"
            exit 1
            ;;
    esac

    info "检测到当前系统架构: ${PLATFORM_OS}-${PLATFORM_ARCH}"
}

# 2. 检查依赖
check_dependencies() {
    if ! command -v curl >/dev/null 2>&1; then
        error "未找到 curl，请先安装 curl。"
        exit 1
    fi
    if ! command -v tar >/dev/null 2>&1; then
        error "未找到 tar，请先安装 tar。"
        exit 1
    fi
}

# 3. 安装预编译二进制或从源码构建
install_binary() {
    mkdir -p "$INSTALL_DIR"

    ARCHIVE_EXT="tar.gz"
    if [ "$PLATFORM_OS" = "windows" ]; then
        ARCHIVE_EXT="zip"
    fi

    ARTIFACT_NAME="${BINARY_NAME}-${PLATFORM_OS}-${PLATFORM_ARCH}.${ARCHIVE_EXT}"
    LATEST_RELEASE_URL="https://github.com/${REPO}/releases/latest/download/${ARTIFACT_NAME}"

    TMP_DIR="$(mktemp -d)"
    trap 'rm -rf "$TMP_DIR"' EXIT

    info "尝试从 GitHub Releases 下载预编译文件..."
    info "下载地址: ${LATEST_RELEASE_URL}"

    HTTP_STATUS=$(curl -fsSL -w "%{http_code}" -o "${TMP_DIR}/${ARTIFACT_NAME}" "$LATEST_RELEASE_URL" || true)

    if [ "$HTTP_STATUS" = "200" ] && [ -f "${TMP_DIR}/${ARTIFACT_NAME}" ]; then
        info "正在解压并安装到 ${INSTALL_DIR}..."
        if [ "$ARCHIVE_EXT" = "zip" ]; then
            unzip -q -o "${TMP_DIR}/${ARTIFACT_NAME}" -d "$INSTALL_DIR"
        else
            tar -xzf "${TMP_DIR}/${ARTIFACT_NAME}" -C "$INSTALL_DIR"
        fi
        chmod +x "${INSTALL_DIR}/${BINARY_NAME}"
        success "已成功安装预编译二进制文件至: ${INSTALL_DIR}/${BINARY_NAME}"
    else
        warn "未找到对应版本的 GitHub Release 预编译包 (HTTP $HTTP_STATUS)。"
        if command -v cargo >/dev/null 2>&1; then
            info "检测到已安装 Rust 环境，正在通过 Cargo 自动拉取源码编译安装..."
            cargo install --git "https://github.com/${REPO}.git" --force
            success "已通过 Cargo 成功编译并安装 ${BINARY_NAME}！"
            return 0
        else
            error "未找到预编译发布包，且本地未安装 Cargo/Rust 环境。"
            echo "请前往 https://github.com/${REPO}/releases 手动下载，或执行以下命令安装 Rust 后重试："
            echo "  curl --proto '=https' --tlsv1.2 -sSf https://sh.rustup.rs | sh"
            exit 1
        fi
    fi
}

# 4. PATH 路径检查与 Claude 配置提示
post_install() {
    echo ""
    echo "=================================================================="
    success "🎉 crossborder-ecom-mcp 安装成功！"
    echo "=================================================================="
    echo ""
    
    # PATH 提示
    case ":$PATH:" in
        *":$INSTALL_DIR:"*) ;;
        *)
            warn "提示: ${INSTALL_DIR} 不在当前的 PATH 环境变量中。"
            echo "请将以下内容添加到您的 Shell 配置文件 (~/.zshrc 或 ~/.bashrc) 中："
            echo -e "  ${BLUE}export PATH=\"\$HOME/.local/bin:\$PATH\"${NC}"
            echo ""
            ;;
    esac

    echo "快速测试命令："
    echo "  ${BINARY_NAME} --help"
    echo "  ${BINARY_NAME} category \"美妆\""
    echo ""
    echo "接入 Claude Desktop (将以下配置加入 ~/Library/Application Support/Claude/claude_desktop_config.json)："
    cat <<EOF
{
  "mcpServers": {
    "crossborder-ecom": {
      "command": "${INSTALL_DIR}/${BINARY_NAME}",
      "args": []
    }
  }
}
EOF
    echo ""
    echo "=================================================================="
}

main() {
    detect_platform
    check_dependencies
    install_binary
    post_install
}

main "$@"
