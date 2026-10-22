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

Windows 产物位于 `src-tauri/target/release/Astralink.exe` 和 `src-tauri/target/release/bundle/nsis/Astralink_*_x64-setup.exe`。macOS 使用 `Astralink.app` / `Astralink_*.dmg`，Linux 使用 `Astralink_*.deb` / `Astralink_*.AppImage`（系统包 ID 按平台规则为小写 astralink）。

资源准备脚本将 Node 运行时、锁定依赖和引擎代码放入被 Git 忽略的资源目录。环境变量 `ASTRALINK_NODE_BINARY` 可指定经过验证的 Node 可执行文件。用户运行完整包不需要手动 npm 安装。

同一命令可在原生 macOS 和 Linux 环境构建：macOS 生成 DMG，Linux 生成 DEB/AppImage。GitHub 的 **Build desktop packages** 工作流提供 Windows x64、macOS Intel/Apple Silicon、Linux x64 四个构建任务，支持手动运行和 `v*` 标签触发；产物在运行记录的 Artifacts 下载。详情及验证边界见 [跨平台构建说明](docs/cross-platform-build.md)。

Node 路径按平台识别：显式覆盖路径、内置运行时、系统 PATH。未找到时界面提示安装 Node.js；Windows 使用 `node.exe`，macOS/Linux 使用 `node`。

桌面和引擎的状态、实际导入兼容范围和验证证据以 docs/validation.md 为准。架构和接口见 docs/api-contract.md 与 docs/superpowers/specs/2026-10-14-astralink-design.md。

## 数据和凭据

导入页支持 Xshell、MobaXterm `.mxtsessions`、WindTerm `user.sessions`、SecureCRT XML、FinalShell `conn`、Termius 本机 IndexedDB、Electerm 书签 JSON、NyaTerm 本机 `.redb` / v1 JSON、OpenSSH config，以及 NyaTerm `.nya` 加密备份。所有来源先预览再确认，仅导入 SSH 连接；不兼容的协议和不能安全转换的认证配置会在预览里标明。FinalShell 加密密码需补齐；NyaTerm `.redb` 只读连接和分组元数据，不读取其凭据表；Termius 使用本机系统凭据库中的 localKey 解密 IndexedDB 副本；WindTerm 主密码仅在本次预览使用。`.nya` 需备份密码和完整性校验，只提取 SSH 会话，不恢复 NyaTerm 的全量应用状态。内联私钥不会被写入本机文件，需重新选择私钥路径。

设置页可使用 HTTPS WebDAV 手动同步加密连接快照。上传包含连接元数据及密码、私钥口令、动态密钥；不上传本机私钥文件、MCP 访问令牌和应用设置。Argon2id 派生密钥、XChaCha20-Poly1305 加密；远端覆盖和恢复必须先预览版本并显式确认，恢复会替换本机连接。远端地址和账号仅在本机保存，WebDAV 密码与备份口令不保存。该功能不提供后台自动同步，尚未在真实 WebDAV 服务上验收。

个人连接数据保存在操作系统应用数据目录；不提交到仓库。密码、私钥口令和产品访问令牌由 Rust 管理，产品日志不打印认证材料。MCP 和控制入口使用不同令牌，并只监听本机地址。

项目目录为 `F:/develop/astralink`，项目、程序与安装包名称统一为 Astralink（npm 包名 astralink，引擎 @astralink/engine）。应用标识和系统凭据库使用 `dev.astralink.desktop`，数据库使用 `astralink.db`，MCP 配置键使用 `astralink`。首次启动会从旧版目录复制数据库和恢复日志，旧文件保留；凭据按需迁入新的系统凭据库。重新点击客户端的一键接入后，会备份并迁移旧版本机 MCP 条目，然后重新加载客户端。

设置页的“MCP 客户端接入”提供 Claude Code、Claude Desktop、Codex、Cursor、VS Code、旧版 Windsurf Cascade、Antigravity、OpenCode、OpenClaw、Hermes、WorkBuddy 和 Cherry Studio 的接入指引。Claude Desktop 与 WorkBuddy 使用随安装包提供的 stdio 桥接，可写入各自的本机用户配置；Cherry Studio 可在其 MCP 管理界面导入 stdio 桥接片段，桌面版会自动填入本机 Node 和脚本绝对路径。片段只显示令牌占位符，令牌须在桌面界面显式点击复制。云端 ChatGPT 不能直连本机回环地址，需在有权限的工作空间配置 Secure MCP Tunnel，或单独部署经认证的公网 HTTPS 网关，当前不提供本机一键接入。修改端口后先保存并重启 MCP 服务，再更新客户端配置。

## 上游

SSH 工具能力基于 classfang/ssh-mcp-server（ISC），保留其许可证。星枢 Astralink 对其传输、凭据、OTP 和连接配置生命周期做集成适配。前端为 Vue。
