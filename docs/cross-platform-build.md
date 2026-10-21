# 跨平台构建

Astralink 使用各平台的原生构建环境，安装包包含对应操作系统和 CPU 架构的 Node 与 SSH MCP 引擎。Windows 使用系统凭据管理器，macOS 使用 Keychain，Linux 使用 Secret Service；Linux 桌面需有已解锁的 GNOME Keyring 或兼容服务。

## GitHub Actions

提交项目到 GitHub 后，打开 **Actions → Build desktop packages → Run workflow**，选择分支运行。推送 `v*` 标签也会自动触发。工作流只上传构建产物，不自动发布 Release。

| 构建任务 | 原生 runner | 产物 |
| --- | --- | --- |
| Windows x64 | windows-2022 | NSIS `.exe` |
| macOS Apple Silicon | macos-15 | arm64 `.dmg` |
| macOS Intel | macos-15-intel | x64 `.dmg` |
| Linux x64 | ubuntu-22.04 | `.deb`、`.AppImage` |

成功后在运行记录的 **Artifacts** 下载 `Astralink-windows-x64`、`Astralink-macos-arm64`、`Astralink-macos-x64`、`Astralink-linux-x64`。每个任务单独运行前端、引擎及 Rust 测试，再构建并启动资源目录中的内置 Node/引擎做控制接口验证。资源包含的是对应原生 runner 的 Node，禁止从 Windows 复制 `node.exe` 到 Linux/macOS 包。

macOS 当前使用 ad-hoc 签名，不需要配置 Apple 证书，但没有完成 Apple 公证；公开分发时仍可能遇到 Gatekeeper 提示。未来配置 Developer ID 证书与公证后，再覆盖 `signingIdentity` 和增加 Apple secrets。构建能力不等于各平台已完成桌面功能验收。

## 本地原生构建

所有平台使用同一命令，Tauri 自动合并 `tauri.macos.conf.json` 或 `tauri.linux.conf.json`，Windows 使用主配置：

```sh
npm ci --ignore-scripts
npm run desktop:build
```

当前 macOS 包将最低系统版本设为 13.5。macOS 需要 Xcode Command Line Tools 与 Rust；Linux 需要 WebKitGTK 4.1、GTK、托盘、D-Bus、编译工具和 Rust。Ubuntu 22.04 可参考工作流中的 apt 依赖，Ubuntu 24.04/26.04 的 FUSE 包名使用 `libfuse2t64`。

WSL 验证应在 Linux 文件系统的隔离副本中安装 Linux 原生 Node/Rust、执行 `npm ci` 和构建，不能复用 Windows `node_modules`。Linux x64 包不承诺支持所有发行版；工作流以 Ubuntu 22.04 为构建基线，WSL 上更高版本的成功不代替该基线 runner 验证。

工作流安装依赖时使用 `--ignore-scripts`，与最终引擎资源的安装方式一致。SSH2 使用纯 JavaScript 路径，避免其可选原生加密插件与 Node/OpenSSL 版本不匹配；Vite 和 Tauri 使用锁文件中的平台预编译依赖。

## Node 识别和缺失提示

1. 显式设置 `ASTRALINK_NODE_BINARY` 时使用该路径。
2. 安装版优先使用打包资源中的 `runtime/node.exe`（Windows）或 `runtime/node`（macOS/Linux）。
3. 开发版或内置 Node 缺失时，从当前系统 PATH 查找相应可执行文件，再检查常见安装目录：Windows 的 Program Files/nodejs，macOS 的 Homebrew 和 /usr/local/bin，Linux 的 /usr/local/bin、/usr/bin。
4. 都未找到时，MCP 启动失败并明确提示安装 Node.js、加入 PATH，或设置 `ASTRALINK_NODE_BINARY`；不会伪造服务启动成功。

完整安装包正常情况下无需用户另装 Node。更改系统 PATH 后重新启动 Astralink，让进程取得新的环境。构建机使用 Node 24 LTS，与工作流和二维码依赖的版本要求保持一致。

## 本次验证记录

- 独立导出本次待提交源码验证，未依赖工作区其他尚未提交的功能：Linux 前端 50 项、引擎 50 项、Rust 72 项通过，2 项环境测试跳过；Windows Rust 75 项通过，2 项环境测试跳过。
- Windows 安装包构建流程通过，准备资源中的内置 Node/MCP 引擎启动测试通过。
- WSL Ubuntu 26.04 x64、Node 24.21.0、Rust stable 环境实际生成 DEB（57,055,884 字节）和 AppImage（134,367,736 字节）。从两种包分别解出资源，运行包内 Node 并启动 MCP 控制接口，均通过。
- 独立 D-Bus 会话与隔离的 Secret Service 数据目录中，系统凭据后端读写生成的 MCP 令牌，并完成引擎启动、停止；未访问真实服务器或已有用户凭据。
- WSL 中 AppImage 工具直链下载超时后，改由官方 GitHub API 获取资产，核对官方 SHA-256 后放入 Tauri 工具缓存，封装成功。工作流保留 Tauri 默认下载流程。
- macOS Intel/Apple Silicon 配置和工作流已提供，但尚未在 macOS runner 实际执行；Ubuntu 22.04 的 GitHub 构建基线也需上传仓库后运行验证。WSL 结果不能替代这两项验证，且本轮未进行 Linux 桌面 GUI 的完整交互验收。
