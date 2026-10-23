# AstraLink 服务器版

服务器版是单实例、单管理员、共享连接空间；桌面版仍独立运行。服务器和桌面端不共享 SQLite 文件，使用设置中的加密备份进行迁移。网页只提供管理能力；MCP 引擎仍绑定服务器内部的 `127.0.0.1`，网页登录会话不能用于访问 MCP。

首次部署优先按 [README 的 Docker Compose 快速开始](../README.md#docker-compose-部署推荐)使用 `xinglinglove1029/astralink:v0.1.4`（linux/amd64）。本页补充源码构建、HTTPS 和运维细节。**没有默认管理员密码**，不要把密码写在仓库、`.env` 或 `docker run -e` 参数中。

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

生产环境请用专用非 root 用户运行。可信内网可按下文开启直连 HTTP；不可信网络应设置 `ASTRALINK_PUBLIC_ORIGIN=https://your-domain.example`，由反向代理提供 HTTPS 并转发至本机的 `127.0.0.1:1421`。不要公开 MCP 端口。公网入口必须与浏览器实际 Origin 完全一致。服务重启后网页登录会话失效。

## Docker Compose

推送 `v*` tag 后，GitHub Actions 在桌面构建通过时额外构建并推送 Docker Hub 镜像 `<DOCKERHUB_USERNAME>/astralink:v<版本>`（linux/amd64）。先在 Docker Hub 账号下创建 `astralink` 仓库，并在 GitHub 仓库 Secrets 中设置 `DOCKERHUB_USERNAME` 和具有写入权限的 Docker Hub access token `DOCKERHUB_TOKEN`；未配置时镜像步骤会跳过，桌面 Release 不受影响。手动补发旧 Release 不会自动补推镜像。

镜像构建会显式执行前端构建、资源准备和 Rust 服务端构建。Compose 默认将 HTTP 管理端口绑定到所有网卡；登录与同源校验仍生效，但 HTTP 会明文传输密码和会话。仓库里的 Compose 文件同时支持 `ASTRALINK_IMAGE` 选择预构建镜像和 `build` 本地构建。**部署旧版 `v0.1.3` 镜像**时，需显式设置 `ASTRALINK_WEB_BIND=127.0.0.1` 并使用回环 HTTP Origin；以下是从当前源码构建的方式：

```bash
git clone https://github.com/xinglinglove1029/astralink.git
cd astralink
docker compose -f compose.server.yml build
read -rsp '设置管理员密码（至少 12 字符）: ' ADMIN_PASSWORD; printf '\n'
printf '%s\n' "$ADMIN_PASSWORD" | docker compose -f compose.server.yml run --rm -T --no-deps astralink init-admin
unset ADMIN_PASSWORD
docker compose -f compose.server.yml up -d --no-build
```

源码构建的示例未设置公开地址时，本机访问 `http://127.0.0.1:1421/`；要从其他电脑访问，先设置 `ASTRALINK_PUBLIC_ORIGIN` 为实际使用的服务器地址。如 `1421` 被占用，可设置 `ASTRALINK_WEB_PORT=14421`，同时将 `ASTRALINK_PUBLIC_ORIGIN` 的端口改为 `14421`；这只是**管理页面的宿主机端口**，不是 MCP 服务端口。变更 Compose 项目名会创建不同的卷，新实例不会自动继承旧连接。

`v0.1.4` Docker 镜像允许 HTTP 管理页，Compose 默认绑定所有网卡；其他电脑直接打开 `http://192.168.97.240:1421/` 时，设 `ASTRALINK_PUBLIC_ORIGIN=http://192.168.97.240:1421`，保证登录的 Origin 校验与实际地址一致。管理员登录仍必需，MCP 端口仍只监听容器回环地址。HTTP 会明文传输管理员密码与会话 Cookie，建议后续改用 HTTPS；旧版 `v0.1.3` 镜像不支持非回环 HTTP。

### HTTPS 反向代理

在已有证书的 Linux 服务器上，可由同机 Nginx 终止 TLS，转发至宿主机回环端口。先在未提交的 `.env` 中设置 `ASTRALINK_PUBLIC_ORIGIN=https://astralink.example.com`、`ASTRALINK_WEB_BIND=127.0.0.1`；`ASTRALINK_LOCAL_HTTP` 可设为 `0`。然后配置 Nginx，例如：

```nginx
server {
    listen 443 ssl;
    server_name astralink.example.com;
    ssl_certificate /etc/letsencrypt/live/astralink.example.com/fullchain.pem;
    ssl_certificate_key /etc/letsencrypt/live/astralink.example.com/privkey.pem;

    location / {
        proxy_pass http://127.0.0.1:1421;
        proxy_set_header Host $host;
        proxy_set_header X-Forwarded-Proto https;
        client_max_body_size 45m;
    }
}
```

将示例域名与证书路径改成实际值，再 `docker compose -f compose.server.yml up -d --no-build`、`nginx -t` 并重载 Nginx。浏览器访问的源（协议、主机、端口）必须与 `ASTRALINK_PUBLIC_ORIGIN` **完全一致**，否则登录和修改配置会被同源校验拒绝。反向代理需在宿主机上运行；另一个容器里的 `127.0.0.1` 并不是宿主机地址。不要将 `18766` 等 MCP 端口直接转发到公网。

`astralink-data` 卷包含 SQLite、管理员密码哈希、主密钥、加密凭据和服务器端私钥。务必将整个卷作为敏感数据备份；丢失主密钥后不能解密原凭据。不要挂载桌面版的数据库，也不要把数据卷、私钥或导出的 `.backup` 文件放入镜像和源码仓库。

排查部署时先看 `docker compose -f compose.server.yml ps`、`docker compose -f compose.server.yml logs --tail 100 astralink` 和 `curl -i http://127.0.0.1:1421/api/auth/status`。接口返回 HTTP 200 和 `authenticated:false` 表示服务可用、当前会话尚未登录；登录/修改配置出现 403 时检查公开源是否与反向代理实际地址一致。同机运行桌面版和服务器版时，两者默认 MCP 端口可能冲突，请在服务器设置页为其中一方换用空闲端口。

## 配置迁移

桌面版和网页版在“设置 > 数据与备份 > 配置迁移”导出、预览并导入同一加密 `.backup` 文件；可选择合并或替换连接。新版备份包含连接凭据、TOTP 和应用设置，旧版 WebDAV `astralink-backup-v1` 仍可读取。管理员密码、MCP 令牌、运行日志、客户端注册信息和私钥文件不打包。导入后若目标机器缺少原私钥或本地传输目录，预览会提示数量，相关连接会被停用，需重新选择文件或目录后启用。服务器本地私钥可在连接编辑器中上传；第三方客户端文件在浏览器导入时上传临时副本，目录型来源需先导出单文件。

首次部署后检查：未登录的配置 API 返回 401；错误密码被限速；登录后可新增连接、导出备份、删除并恢复连接；重启服务后配置仍在，旧会话失效。实际 SSH/OTP 还需用目标服务器验证。
