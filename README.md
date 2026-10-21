# 星枢 · Astralink

将已有 SSH 客户端配置集中管理，让 Codex 通过内置 MCP 服务使用你的服务器。

技术栈：Tauri 2、Vue 3、TypeScript、Rust、Node SSH MCP 引擎。

## 开发

Windows 需要 Rust stable（MSVC）、Visual Studio C++ Build Tools 和 Windows SDK。运行时依赖 WebView2。已提供 Tauri 启动脚本，能在安装 Rust 后自动将当前用户的 `.cargo/bin` 加到子进程 PATH。

```powershell
npm install
npm run desktop
```

仅查看界面：`npm run dev`。浏览器模式不能访问本机 Tauri 功能，界面会显示需要桌面环境，不返回模拟连接成功。

## 独立验证入口

```powershell
npm test
npm test --workspace @astralink/engine
cargo test --manifest-path src-tauri/Cargo.toml
npm run build
```

普通 build 只执行类型检查和打包，不自动跑测试。

## 构建桌面安装包

```powershell
npm run desktop:build
```

Windows 产物位于 `src-tauri/target/release/Astralink.exe` 和 `src-tauri/target/release/bundle/nsis/Astralink_0.1.0_x64-setup.exe`。macOS 使用 `Astralink.app` / `Astralink_*.dmg`，Linux 使用 `Astralink_*.deb` / `Astralink_*.AppImage`（系统包 ID 按平台规则为小写 astralink）。

资源准备脚本将 Node 运行时、锁定依赖和引擎代码放入被 Git 忽略的资源目录。环境变量 `ASTRALINK_NODE_BINARY` 可指定经过验证的 Node 可执行文件。用户运行完整包不需要手动 npm 安装。

同一命令可在原生 macOS 和 Linux 环境构建：macOS 生成 DMG，Linux 生成 DEB/AppImage。GitHub 的 **Build desktop packages** 工作流提供 Windows x64、macOS Intel/Apple Silicon、Linux x64 四个构建任务，支持手动运行和 `v*` 标签触发；产物在运行记录的 Artifacts 下载。详情及验证边界见 [跨平台构建说明](docs/cross-platform-build.md)。

Node 路径按平台识别：显式覆盖路径、内置运行时、系统 PATH。未找到时界面提示安装 Node.js；Windows 使用 `node.exe`，macOS/Linux 使用 `node`。

桌面和引擎的状态、实际导入兼容范围和验证证据以 docs/validation.md 为准。架构和接口见 docs/api-contract.md 与 docs/superpowers/specs/2026-10-14-astralink-design.md。

## 数据和凭据

个人连接数据保存在操作系统应用数据目录；不提交到仓库。密码、私钥口令和产品访问令牌由 Rust 管理，产品日志不打印认证材料。MCP 和控制入口使用不同令牌，并只监听本机地址。

项目目录为 `F:/develop/astralink`，项目、程序与安装包名称统一为 Astralink（npm 包名 astralink，引擎 @astralink/engine）。应用标识和系统凭据库使用 `dev.astralink.desktop`，数据库使用 `astralink.db`，MCP 配置键使用 `astralink`。首次启动会从旧版目录复制数据库和恢复日志，旧文件保留；凭据按需迁入新的系统凭据库。重新点击客户端的一键接入后，会备份并迁移旧版本机 MCP 条目，然后重新加载客户端。

设置页的“MCP 客户端接入”提供 Claude Code、Codex、Cursor、VS Code、旧版 Windsurf Cascade、Antigravity、OpenCode、OpenClaw、Hermes 的配置片段。按当前端口生成本机地址，片段只显示令牌占位符，令牌须在桌面界面显式点击复制；只有 Codex 提供已验证的一键配置。ChatGPT 网页版无法直接访问本机回环地址，Claude Desktop 的本机接入尚未完成验证，因此它们不会生成可复制的错误配置。修改端口后先保存并重启 MCP 服务，再复制新片段。

## 上游

SSH 工具能力基于 classfang/ssh-mcp-server（ISC），保留其许可证。星枢 Astralink 对其传输、凭据、OTP 和连接配置生命周期做集成适配。前端为 Vue；本项目不修改 MediPaaS。
