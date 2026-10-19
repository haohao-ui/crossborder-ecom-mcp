# 多 Agent 通用一键安装提示词 (Universal Agent Installer Prompt)

> **使用方法**：直接复制下方方框中的完整提示词，发送给你正在使用的任何 AI 编程智能体（如 **Claude Code**, **Cursor**, **Windsurf**, **Antigravity / AGY**, **Cline**, **Roo Code** 等），智能体将自动识别环境、安装二进制、注册 MCP 配置并加载电商分析 Skill。

---

```markdown
请帮我自动安装和配置跨境出海电商情报 MCP 工具【crossborder-ecom-mcp】，并加载配套的 Skill。请按照以下步骤自动化执行：

### 步骤 1：安装二进制可执行文件
1. 检查本地是否已安装 `crossborder-ecom-mcp`。若未安装：
   - **macOS / Linux** 终端执行：
     ```bash
     curl -fsSL https://raw.githubusercontent.com/haohao-ui/crossborder-ecom-mcp/main/install.sh | bash
     ```
   - **Windows** PowerShell 执行：
     ```powershell
     irm https://raw.githubusercontent.com/haohao-ui/crossborder-ecom-mcp/main/install.ps1 | iex
     ```
2. 若一键脚本下载失败但本地安装了 Rust 环境，可通过 Cargo 源码编译安装：
   ```bash
   cargo install --git https://github.com/haohao-ui/crossborder-ecom-mcp.git
   ```
3. 确认二进制文件路径（通常为 `$HOME/.local/bin/crossborder-ecom-mcp` 或 `cargo` bin 目录），并执行以下命令验证是否能正常运行：
   ```bash
   crossborder-ecom-mcp --help
   ```

### 步骤 2：自动配置 MCP 服务端 (根据当前 Agent 宿主自动选择)
请检测当前运行的宿主环境，并在对应的 MCP 配置文件中追加 `crossborder-ecom` 服务（请将 `<BINARY_PATH>` 替换为实际的二进制绝对路径）：

MCP 配置片段如下：
```json
{
  "mcpServers": {
    "crossborder-ecom": {
      "command": "<BINARY_PATH>",
      "args": []
    }
  }
}
```

请优先适配以下路径：
- **Claude Desktop / Claude Code**：
  - macOS: `~/Library/Application Support/Claude/claude_desktop_config.json`
  - Linux: `~/.config/Claude/claude_desktop_config.json`
  - Windows: `%APPDATA%\Claude\claude_desktop_config.json`
  - 若支持 Claude Code CLI：直接执行 `claude mcp add crossborder-ecom -- <BINARY_PATH>`
- **Cursor**：
  - `~/.cursor/mcp.json` 或当前项目根目录下的 `.cursor/mcp.json`
- **Windsurf**：
  - `~/.codeium/windsurf/mcp_config.json`
- **Cline / Roo Code (VS Code)**：
  - `~/Library/Application Support/Code/User/globalStorage/saoudrizwan.claude-dev/settings/cline_mcp_settings.json`（或 Roo Code 对应目录）
- **Antigravity (AGY)**：
  - 注册到当前用户的 MCP 配置文件中。

### 步骤 3：安装并加载跨境电商 Skill 技能定义
1. 请下载官方技能定义文件并保存到本地：
   - 下载地址：`https://raw.githubusercontent.com/haohao-ui/crossborder-ecom-mcp/main/skills/crossborder-ecom/SKILL.md`
   - 根据宿主环境保存到对应的技能/规则目录（例如 Antigravity 的 `~/.gemini/config/skills/crossborder-ecom/SKILL.md`，Claude 的 `~/.claude/skills/crossborder-ecom/SKILL.md`，或 Cursor 的 `.cursorrules` / rules 目录）。

### 步骤 4：功能验证与汇报
1. 在终端运行一次测试命令确认功能完好：
   ```bash
   crossborder-ecom-mcp category "美妆"
   ```
2. 全部配置完成后，请向我输出简明汇报：
   - 二进制安装路径与版本
   - 已配置的 MCP 文件路径
   - 已安装的 Skill 路径与可用 MCP 工具列表（`get_product_detail`, `get_product_reviews`, `get_price_history`, `list_categories`, `list_supported_countries`）
```
