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
    TARGET_BIN="$BINARY_NAME"
    if [ "$PLATFORM_OS" = "windows" ]; then
        ARCHIVE_EXT="zip"
        TARGET_BIN="${BINARY_NAME}.exe"
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
            if command -v unzip >/dev/null 2>&1; then
                unzip -q -o "${TMP_DIR}/${ARTIFACT_NAME}" -d "$INSTALL_DIR"
            elif command -v tar >/dev/null 2>&1; then
                tar -xzf "${TMP_DIR}/${ARTIFACT_NAME}" -C "$INSTALL_DIR" || unzip -q -o "${TMP_DIR}/${ARTIFACT_NAME}" -d "$INSTALL_DIR"
            else
                error "解压需要 unzip 工具，请先安装 unzip。"
                exit 1
            fi
        else
            tar -xzf "${TMP_DIR}/${ARTIFACT_NAME}" -C "$INSTALL_DIR"
        fi

        # 检查解压出的可执行文件名
        if [ -f "${INSTALL_DIR}/${TARGET_BIN}" ]; then
            chmod +x "${INSTALL_DIR}/${TARGET_BIN}" 2>/dev/null || true
            ACTUAL_BIN="${TARGET_BIN}"
        elif [ -f "${INSTALL_DIR}/${BINARY_NAME}" ]; then
            chmod +x "${INSTALL_DIR}/${BINARY_NAME}" 2>/dev/null || true
            ACTUAL_BIN="${BINARY_NAME}"
        else
            error "解压完成但未在 ${INSTALL_DIR} 中找到目标可执行文件。"
            exit 1
        fi

        success "已成功安装预编译二进制文件至: ${INSTALL_DIR}/${ACTUAL_BIN}"
    else
        warn "未找到对应版本的 GitHub Release 预编译包 (HTTP $HTTP_STATUS)。"
        if command -v cargo >/dev/null 2>&1; then
            info "检测到已安装 Rust 环境，正在通过 Cargo 自动拉取源码编译安装..."
            cargo install --git "https://github.com/${REPO}.git" --force
            ACTUAL_BIN="$BINARY_NAME"
            success "已通过 Cargo 成功编译并安装 ${BINARY_NAME}！"
        else
            error "未找到预编译发布包，且本地未安装 Cargo/Rust 环境。"
            echo "请前往 https://github.com/${REPO}/releases 手动下载，或执行以下命令安装 Rust 后重试："
            echo "  curl --proto '=https' --tlsv1.2 -sSf https://sh.rustup.rs | sh"
            exit 1
        fi
    fi
}

# 4. 安全配置 PATH 环境变量（不破坏用户已有环境）
configure_path() {
    # 如果已经在当前 PATH 中，无需改动
    case ":$PATH:" in
        *":$INSTALL_DIR:"*)
            info "检测到 ${INSTALL_DIR} 已在当前 PATH 环境变量中。"
            return 0
            ;;
    esac

    warn "提示: 安装目录 ${INSTALL_DIR} 尚未包含在当前 PATH 环境变量中。"

    # 针对 Windows 环境（MINGW / MSYS / Git Bash）
    if [ "$PLATFORM_OS" = "windows" ]; then
        echo ""
        info "检测到您当前处于 Windows 环境。"
        echo "为了在 PowerShell、CMD 或 Windows 应用程序中永久生效，请在 PowerShell 中运行以下命令（安全添加至用户 PATH，不影响系统变量）："
        echo -e "  ${BLUE}[Environment]::SetEnvironmentVariable('Path', [Environment]::GetEnvironmentVariable('Path', 'User') + ';${INSTALL_DIR}', 'User')${NC}"
        echo ""
        echo "若您希望在当前的 Git Bash/MSYS 终端中生效，可将以下内容添加到 ~/.bashrc 中："
        echo -e "  ${BLUE}export PATH=\"${INSTALL_DIR}:\$PATH\"${NC}"
        echo ""
        return 0
    fi

    # 针对 Unix / macOS / Linux：检测匹配的 Shell 配置文件
    DETECTED_RC=""
    CURRENT_SHELL="$(basename "${SHELL:-bash}")"

    case "$CURRENT_SHELL" in
        zsh)
            DETECTED_RC="$HOME/.zshrc"
            ;;
        bash)
            if [ "$PLATFORM_OS" = "darwin" ]; then
                if [ -f "$HOME/.zprofile" ]; then
                    DETECTED_RC="$HOME/.zprofile"
                elif [ -f "$HOME/.bash_profile" ]; then
                    DETECTED_RC="$HOME/.bash_profile"
                else
                    DETECTED_RC="$HOME/.zshrc"
                fi
            else
                if [ -f "$HOME/.bashrc" ]; then
                    DETECTED_RC="$HOME/.bashrc"
                elif [ -f "$HOME/.bash_profile" ]; then
                    DETECTED_RC="$HOME/.bash_profile"
                else
                    DETECTED_RC="$HOME/.profile"
                fi
            fi
            ;;
        fish)
            DETECTED_RC="$HOME/.config/fish/config.fish"
            ;;
        *)
            if [ -f "$HOME/.profile" ]; then
                DETECTED_RC="$HOME/.profile"
            elif [ -f "$HOME/.bashrc" ]; then
                DETECTED_RC="$HOME/.bashrc"
            fi
            ;;
    esac

    # 如果找到了配置文件，先检查是否已经配置过，严格避免重复添加破坏环境
    if [ -n "$DETECTED_RC" ] && [ -f "$DETECTED_RC" ]; then
        if grep -Fq "$INSTALL_DIR" "$DETECTED_RC" 2>/dev/null; then
            info "检测到 ${DETECTED_RC} 中已包含 ${INSTALL_DIR} 相关配置，请执行以下命令使其在当前终端生效："
            echo -e "  ${BLUE}source \"${DETECTED_RC}\"${NC}"
            return 0
        fi
    fi

    # 安全地将 PATH 写入检测到的配置文件（使用追加模式，附带明确注释）
    if [ -n "$DETECTED_RC" ] && [ -w "$(dirname "$DETECTED_RC")" ]; then
        mkdir -p "$(dirname "$DETECTED_RC")"
        if [ "$CURRENT_SHELL" = "fish" ]; then
            {
                echo ""
                echo "# Added by crossborder-ecom-mcp installer"
                echo "fish_add_path \"$INSTALL_DIR\""
            } >> "$DETECTED_RC"
        else
            {
                echo ""
                echo "# Added by crossborder-ecom-mcp installer"
                echo "export PATH=\"${INSTALL_DIR}:\$PATH\""
            } >> "$DETECTED_RC"
        fi
        success "已安全将安装路径追加至您的 Shell 配置文件: ${DETECTED_RC}"
        echo "请执行以下命令使配置立即生效："
        echo -e "  ${BLUE}source \"${DETECTED_RC}\"${NC}"
        echo ""
    else
        # 无法自动写入时，提供安全的手动设置说明
        echo "请将以下内容手动添加到您的 Shell 配置文件 (~/.zshrc, ~/.bashrc 或 ~/.profile) 中："
        echo -e "  ${BLUE}export PATH=\"${INSTALL_DIR}:\$PATH\"${NC}"
        echo ""
    fi
}

# 5. 安装后说明与 MCP 配置指南
post_install() {
    echo ""
    echo "=================================================================="
    success "🎉 crossborder-ecom-mcp 安装成功！"
    echo "=================================================================="
    echo ""
    
    # 执行安全的环境变量配置
    configure_path

    BIN_EXEC="${ACTUAL_BIN:-$BINARY_NAME}"
    
    echo "快速测试命令："
    echo "  ${INSTALL_DIR}/${BIN_EXEC} --help"
    echo "  ${INSTALL_DIR}/${BIN_EXEC} category \"美妆\""
    echo ""

    # 根据不同操作系统显示对应的 MCP 配置指引
    CLAUDE_CONFIG_PATH=""
    case "$PLATFORM_OS" in
        darwin)
            CLAUDE_CONFIG_PATH="~/Library/Application Support/Claude/claude_desktop_config.json"
            ;;
        linux)
            CLAUDE_CONFIG_PATH="~/.config/Claude/claude_desktop_config.json"
            ;;
        windows)
            CLAUDE_CONFIG_PATH="%APPDATA%\\Claude\\claude_desktop_config.json"
            ;;
    esac

    echo "接入 Claude Desktop (将以下配置加入 ${CLAUDE_CONFIG_PATH})："
    cat <<EOF
{
  "mcpServers": {
    "crossborder-ecom": {
      "command": "${INSTALL_DIR}/${BIN_EXEC}",
      "args": []
    }
  }
}
EOF
    echo ""
    echo "若使用 Claude Code CLI，可直接运行："
    echo -e "  ${BLUE}claude mcp add crossborder-ecom -- \"${INSTALL_DIR}/${BIN_EXEC}\"${NC}"
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
