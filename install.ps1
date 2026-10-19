# ==============================================================================
# crossborder-ecom-mcp Windows PowerShell 一键安装脚本
# 项目地址: https://github.com/haohao-ui/crossborder-ecom-mcp
# ==============================================================================

[CmdletBinding()]
param(
    [string]$InstallDir = $env:INSTALL_DIR
)

$ErrorActionPreference = 'Stop'

$Repo = "haohao-ui/crossborder-ecom-mcp"
$BinaryName = "crossborder-ecom-mcp.exe"

if (-not $InstallDir) {
    $InstallDir = Join-Path $HOME ".local\bin"
}

function Write-Info($msg) {
    Write-Host "[INFO] $msg" -ForegroundColor Cyan
}

function Write-Success($msg) {
    Write-Host "[SUCCESS] $msg" -ForegroundColor Green
}

function Write-Warn($msg) {
    Write-Host "[WARN] $msg" -ForegroundColor Yellow
}

function Write-Err($msg) {
    Write-Host "[ERROR] $msg" -ForegroundColor Red
}

# 1. 架构识别
$arch = [System.Runtime.InteropServices.RuntimeInformation]::OSArchitecture.ToString().ToLower()
switch -Regex ($arch) {
    "^(x64|amd64)$" { $platformArch = "amd64" }
    "^arm64$"       { $platformArch = "arm64" }
    default {
        Write-Err "不支持的硬件架构: $arch"
        exit 1
    }
}

Write-Info "检测到当前系统架构: windows-$platformArch"

# 2. 准备目录
if (-not (Test-Path $InstallDir)) {
    New-Item -ItemType Directory -Path $InstallDir -Force | Out-Null
}

# 3. 下载预编译文件
$artifactName = "crossborder-ecom-mcp-windows-$platformArch.zip"
$latestReleaseUrl = "https://github.com/$Repo/releases/latest/download/$artifactName"
$tempZip = Join-Path ([System.IO.Path]::GetTempPath()) ([System.Guid]::NewGuid().ToString() + ".zip")

Write-Info "尝试从 GitHub Releases 下载预编译文件..."
Write-Info "下载地址: $latestReleaseUrl"

$downloadSuccess = $false
try {
    Invoke-WebRequest -Uri $latestReleaseUrl -OutFile $tempZip -UseBasicParsing
    $downloadSuccess = $true
} catch {
    Write-Warn "未找到对应版本的 GitHub Release 预编译包或网络下载受限。"
}

if ($downloadSuccess -and (Test-Path $tempZip)) {
    try {
        Write-Info "正在解压并安装到 $InstallDir..."
        Expand-Archive -Path $tempZip -DestinationPath $InstallDir -Force
        $installedFile = Join-Path $InstallDir $BinaryName
        if (Test-Path $installedFile) {
            Write-Success "已成功安装二进制文件至: $installedFile"
        } else {
            throw "解压后未找到可执行文件 $BinaryName"
        }
    } finally {
        if (Test-Path $tempZip) {
            Remove-Item -Path $tempZip -Force -ErrorAction SilentlyContinue
        }
    }
} else {
    # 源码回退
    if (Get-Command cargo -ErrorAction SilentlyContinue) {
        Write-Info "检测到已安装 Rust 环境，正在通过 Cargo 自动编译安装..."
        cargo install --git "https://github.com/$Repo.git" --force
        Write-Success "已通过 Cargo 成功编译并安装 $BinaryName！"
    } else {
        Write-Err "未找到预编译发布包，且本地未检测到 Cargo/Rust 开发环境。"
        Write-Host "请前往 https://github.com/$Repo/releases 手动下载，或安装 Rust 后重试。"
        exit 1
    }
}

# 4. 安全配置系统变量 (PATH)，不破坏用户已有环境
Write-Info "检查 PATH 环境变量..."
$userPath = [Environment]::GetEnvironmentVariable("Path", "User")
$pathList = if ($userPath) { $userPath -split ';' | Where-Object { $_ -ne '' } } else { @() }

if ($pathList -notcontains $InstallDir) {
    try {
        $newPath = if ([string]::IsNullOrEmpty($userPath)) { $InstallDir } else { "$userPath;$InstallDir" }
        [Environment]::SetEnvironmentVariable("Path", $newPath, "User")
        $env:Path = "$env:Path;$InstallDir"
        Write-Success "已安全将 $InstallDir 追加到用户级 PATH 环境变量中。"
    } catch {
        Write-Warn "未能自动写入系统变量，建议手动将 $InstallDir 添加到系统的 PATH 环境变量中。"
    }
} else {
    Write-Info "检测到 $InstallDir 已存在于 PATH 环境变量中，无需重复配置。"
}

# 5. 完成提示
Write-Host ""
Write-Host "=================================================================="
Write-Success "🎉 crossborder-ecom-mcp 安装成功！"
Write-Host "=================================================================="
Write-Host ""
Write-Host "快速测试命令："
Write-Host "  $BinaryName --help"
Write-Host "  $BinaryName category `"美妆`""
Write-Host ""

$claudeConfigPath = Join-Path $env:APPDATA "Claude\claude_desktop_config.json"
$escapedInstallDir = (Join-Path $InstallDir $BinaryName) -replace '\\', '\\'

Write-Host "接入 Claude Desktop (将以下配置加入 $claudeConfigPath)："
Write-Host @"
{
  "mcpServers": {
    "crossborder-ecom": {
      "command": "$escapedInstallDir",
      "args": []
    }
  }
}
"@
Write-Host ""
Write-Host "=================================================================="
