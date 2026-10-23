# AstraLink 服务器版

服务器版是单实例、单管理员、共享连接空间；桌面版仍独立运行。服务器和桌面端不共享 SQLite 文件，使用设置中的加密备份进行迁移。网页只提供管理能力；MCP 引擎仍绑定服务器内部的 `127.0.0.1`，网页登录会话不能用于访问 MCP。

如果在同一台机器同时运行桌面版和服务器版，请在服务器版设置中选择不同的 MCP 端口，避免默认端口冲突。

## Linux 原生运行

构建机器需要 Node.js 24、Rust 工具链及 Tauri 2 的 Linux 构建依赖。当前服务端复用桌面 Rust crate，构建仍需要 WebKitGTK 开发包；运行时也需要对应动态库，但不启动桌面窗口。

```bash
npm ci
npm run build
npm run resources
cargo build --release --manifest-path src-tauri/Cargo.toml --bin astralink-server
install -d -m 700 /srv/astralink/data
read -rsp '设置管理员密码（至少 12 字符）: ' ADMIN_PASSWORD; printf '\n'
printf '%s\n' "$ADMIN_PASSWORD" | ASTRALINK_DATA_DIR=/srv/astralink/data src-tauri/target/release/astralink-server init-admin
unset ADMIN_PASSWORD
ASTRALINK_DATA_DIR=/srv/astralink/data ASTRALINK_RESOURCE_DIR="$PWD" ASTRALINK_WEB_DIR="$PWD/dist" \
  ASTRALINK_LISTEN=127.0.0.1:1421 ASTRALINK_PUBLIC_ORIGIN=http://127.0.0.1:1421 \
  src-tauri/target/release/astralink-server serve
```

生产环境请用专用非 root 用户运行，并设置 `ASTRALINK_PUBLIC_ORIGIN=https://your-domain.example`。反向代理应提供 HTTPS，再转发至本机的 `127.0.0.1:1421`；不要将管理端口或 MCP 端口直接公开。公网入口必须与浏览器实际 Origin 完全一致。服务重启后网页登录会话失效。

## Docker Compose

推送 `v*` tag 后，GitHub Actions 在桌面构建通过时额外构建并推送 Docker Hub 镜像 `<DOCKERHUB_USERNAME>/astralink:v<版本>`（linux/amd64）。先在 Docker Hub 账号下创建 `astralink` 仓库，并在 GitHub 仓库 Secrets 中设置 `DOCKERHUB_USERNAME` 和具有写入权限的 Docker Hub access token `DOCKERHUB_TOKEN`；未配置时镜像步骤会跳过，桌面 Release 不受影响。手动补发旧 Release 不会自动补推镜像。

镜像构建会显式执行前端构建、资源准备和 Rust 服务端构建。Compose 默认只把 HTTP 端口绑定在宿主机回环地址，用于本机验证；远程访问需通过宿主机 HTTPS 反向代理。

```bash
docker compose -f compose.server.yml build
read -rsp '设置管理员密码（至少 12 字符）: ' ADMIN_PASSWORD; printf '\n'
printf '%s\n' "$ADMIN_PASSWORD" | docker compose -f compose.server.yml run --rm -T astralink init-admin
unset ADMIN_PASSWORD
docker compose -f compose.server.yml up -d
```

本机访问 `http://127.0.0.1:1421/`。远程访问前，在未提交到 Git 的 `.env` 中设定 `ASTRALINK_PUBLIC_ORIGIN=https://your-domain.example`，并让反向代理转发至宿主机 `127.0.0.1:1421`。生产环境也可以删除 `ASTRALINK_LOCAL_HTTP`；它仅为 Compose 的本机 HTTP 验证开放，不改变公网 HTTPS 要求。

`astralink-data` 卷包含 SQLite、管理员密码哈希、主密钥、加密凭据和服务器端私钥。务必将整个卷作为敏感数据备份；丢失主密钥后不能解密原凭据。不要挂载桌面版的数据库，也不要把数据卷、私钥或导出的 `.backup` 文件放入镜像和源码仓库。

## 配置迁移

桌面版和网页版在“设置 > 配置迁移”导出、预览并导入同一加密 `.backup` 文件；可选择合并或替换连接。新版备份包含连接凭据、TOTP 和应用设置，旧版 WebDAV `astralink-backup-v1` 仍可读取。管理员密码、MCP 令牌、运行日志、客户端注册信息和私钥文件不打包。导入后若目标机器缺少原私钥或本地传输目录，预览会提示数量，相关连接会被停用，需重新选择文件或目录后启用。服务器本地私钥可在连接编辑器中上传；第三方客户端文件在浏览器导入时上传临时副本，目录型来源需先导出单文件。

首次部署后检查：未登录的配置 API 返回 401；错误密码被限速；登录后可新增连接、导出备份、删除并恢复连接；重启服务后配置仍在，旧会话失效。实际 SSH/OTP 还需用目标服务器验证。
