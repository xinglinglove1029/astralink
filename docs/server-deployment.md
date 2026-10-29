# AstraLink 服务器版

服务器版是单实例、单管理员、共享连接空间；桌面版仍独立运行。服务器和桌面端不共享 SQLite 文件，使用设置中的加密备份进行迁移。网页只提供管理能力；MCP 引擎仍绑定服务器内部的 `127.0.0.1`，网页登录会话不能用于访问 MCP。

首次部署优先按 [README 的 Docker Compose 步骤](../README.md#docker-compose-部署)使用 `xinglinglove1029/astralink:v0.1.7`（linux/amd64）。本页补充 Linux 原生运行、HTTPS 和运维细节。**没有默认管理员密码**，不要把真实密码提交到仓库。

如果在同一台机器同时运行桌面版和服务器版，请在服务器版设置中选择不同的 MCP 端口，避免默认端口冲突。

## Linux 原生运行

构建机器需要 Node.js 24、Rust 工具链及 Tauri 2 的 Linux 构建依赖。当前服务端复用桌面 Rust crate，构建仍需要 WebKitGTK 开发包；运行时也需要对应动态库，但不启动桌面窗口。

```bash
npm ci
npm run build
npm run resources
cargo build --release --manifest-path src-tauri/Cargo.toml --bin astralink-server
install -d -m 700 /srv/astralink/data
read -rsp '设置管理员密码（至少 12 字符）: ' ASTRALINK_ADMIN_PASSWORD; printf '\n'
export ASTRALINK_ADMIN_PASSWORD
ASTRALINK_DATA_DIR=/srv/astralink/data ASTRALINK_RESOURCE_DIR="$PWD" ASTRALINK_WEB_DIR="$PWD/dist" \
  ASTRALINK_LISTEN=127.0.0.1:1421 \
  src-tauri/target/release/astralink-server serve
```

生产环境请用专用非 root 用户运行。可信内网可直连 HTTP；不可信网络应由反向代理提供 HTTPS 并转发至本机的 `127.0.0.1:1421`。不要公开 MCP 端口。代理应保留浏览器实际使用的 Host，否则同源校验会拒绝登录。服务重启后网页登录会话失效。

## Docker Compose

仓库根目录的 `docker-compose.yml` 直接使用 Docker Hub 镜像，部署命令见 README。可选环境变量 `ASTRALINK_MCP_CLIENT_HOST=192.168.97.240` 只改变网页版客户端配置片段的展示地址。MCP 默认监听容器回环地址；下方“WorkBuddy / Cherry Studio”一节给出可信内网远程监听和端口映射方式。

发布工作流为正式版本同时推送版本标签和 `latest`，预发布版本只推送版本标签。已有部署需要执行 `docker compose pull` 再执行 `docker compose up -d` 才会更新。

拉取时若提示 `this image is not in the allowlist`，表示配置的第三方镜像加速源拒绝提供该镜像，需要使用能访问此仓库的镜像源或 Docker Hub 直连；这不是容器启动失败。若 Docker Hub 返回 `manifest unknown`，则检查镜像标签是否已发布。修改标签不会解决加速源的白名单限制。

首次启动时密码至少 12 字符。已有完整数据卷时忽略启动密码，不会在容器重启或升级时重设；若仅存在管理员哈希或主密钥中的一个，服务会拒绝启动以保护已有数据。没有密码且未初始化时会明确报错。

密码作为环境变量会出现在 Docker 容器元数据及 Compose 配置输出中，只有具有主机或 Docker 管理权限的人应能读取它；不要提交带真实密码的 Compose 文件。首次初始化后可将密码项清空并重新创建容器，已有数据卷不会重置。手动删除数据卷会丢失连接及凭据，不要通过删卷来重设密码。

Compose 默认映射 `1421:1421`，本机及同网段设备可以用服务器 IP 访问。若 `1421` 被占用，可把 `docker-compose.yml` 中的端口映射改成 `14421:1421`，浏览器访问新端口。这只是**管理页面的宿主机端口**，不是 MCP 服务端口。变更 Compose 项目名会创建不同的卷，新实例不会自动继承旧连接。

网页登录仍必需，初始用户名 `admin`，密码为初始化时的 `ASTRALINK_ADMIN_PASSWORD`。已初始化的管理员密码沿用，不因升级重置。MCP 端口仍只监听容器回环地址；Web 用户须各自生成个人 MCP 令牌，旧共享令牌在服务器模式下失效。HTTP 会明文传输密码、会话 Cookie 和可能复制的 MCP 令牌，建议在不可信网络使用 HTTPS。

管理员在左侧“用户管理”菜单管理账号，在“角色管理”菜单查看三个内置角色及成员、调整成员角色，在“调用审计”菜单查看记录；目前不支持自定义角色权限。管理员拥有用户、设置、备份及报表权限；操作员可维护共享连接并使用 MCP；查看者只读，不能领取 MCP 令牌。用户在“设置 → 安全与更新”修改自己的密码。角色、密码或启停状态改变时旧会话和个人令牌失效。审计只记录调用元数据；网页记录保留 90 天，MCP 记录按文件大小轮换（当前文件及一个备份文件各约 8 MiB）。统计大屏按最近 7 天的已保留完成记录汇总，因此不等于历史总调用量。

### HTTPS 反向代理

在已有证书的 Linux 服务器上，可由同机 Nginx 终止 TLS，转发至宿主机回环端口。先将 `docker-compose.yml` 中的端口映射改成 `127.0.0.1:1421:1421`，再配置 Nginx，例如：

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

将示例域名与证书路径改成实际值，再执行 `docker compose up -d`、`nginx -t` 并重载 Nginx。反向代理需透传浏览器请求的 Host，登录和修改配置要求 Origin 与 Host 同源。反向代理需在宿主机上运行；另一个容器里的 `127.0.0.1` 并不是宿主机地址。不要将 `18766` 等 MCP 端口直接转发到公网。

`astralink-data` 卷包含 SQLite、管理员密码哈希、Web 用户库与审计文件、主密钥、加密凭据和服务器端私钥。务必将整个卷作为敏感数据备份；丢失主密钥后不能解密原凭据。不要挂载桌面版的数据库，也不要把数据卷、私钥或导出的 `.backup` 文件放入镜像和源码仓库。

排查部署时先看 `docker compose ps`、`docker compose logs --tail 100 astralink` 和 `curl -i http://127.0.0.1:1421/api/auth/status`。接口返回 HTTP 200 和 `authenticated:false` 表示服务可用、当前会话尚未登录；登录/修改配置出现 403 时检查代理是否透传了浏览器实际 Host。同机运行桌面版和服务器版时，两者默认 MCP 端口可能冲突，请在服务器设置页为其中一方换用空闲端口。

## 配置迁移

桌面版和网页版在“设置 > 数据与备份 > 配置迁移”导出、预览并导入同一加密 `.backup` 文件；可选择合并或替换连接。新版备份包含连接凭据、TOTP 和应用设置，旧版 WebDAV `astralink-backup-v1` 仍可读取。管理员密码、MCP 令牌、运行日志、客户端注册信息和私钥文件不打包。导入后若目标机器缺少原私钥或本地传输目录，预览会提示数量，相关连接会被停用，需重新选择文件或目录后启用。服务器本地私钥可在连接编辑器中上传；第三方客户端文件在浏览器导入时上传临时副本，目录型来源需先导出单文件。

首次部署后检查：未登录的配置 API 返回 401；错误密码被限速；登录后可新增连接、导出备份、删除并恢复连接；重启服务后配置仍在，旧会话失效。实际 SSH/OTP 还需用目标服务器验证。


## WorkBuddy / Cherry Studio 从客户端电脑连接服务器

支持 HTTP 的客户端不需要安装 AstraLink 桌面版。WorkBuddy 使用 `type: "http"`、`url` 和 `headers`；Cherry Studio 在 MCP 设置中选 Streamable HTTP、填写 URL 和 Authorization 请求头。Claude Desktop 的本地配置可使用 `npx mcp-remote` 桥接（客户端电脑需 Node.js），不必安装 AstraLink 桌面版。

远程监听开关从 v0.1.9 开始提供；v0.1.8 镜像没有该开关。可信内网直连时，在原 `docker-compose.yml` 中补充以下项目（保留原镜像、密码与数据卷）：

```yaml
services:
  astralink:
    ports:
      - "1421:1421"
      - "18766:18766"
    environment:
      ASTRALINK_MCP_REMOTE_HOST: "172.17.112.203"
```

把 IP 改成实际服务器 IP，MCP 设置的端口与映射端口均保持为 18766；执行 `docker compose up -d`。开关未配置时仍只监听回环；配置后引擎监听容器网络，并只对远程开放 `/mcp`。远程请求必须使用个人令牌，控制接口仍只接受本机请求。`ASTRALINK_MCP_CLIENT_HOST` 只影响展示，不能替代监听开关或 Docker 端口映射；未指定展示 IP 时会使用远程监听 IP。

WorkBuddy 示例（填入新签发的个人令牌）：

```json
{
  "mcpServers": {
    "astralink-xkkj": {
      "type": "http",
      "url": "http://172.17.112.203:18766/mcp",
      "headers": { "Authorization": "Bearer <ASTRALINK_MCP_TOKEN>" }
    }
  }
}
```

`ECONNREFUSED` 表示网络端口未监听或未映射，发生在令牌校验之前；401 表示网络已通但令牌无效。先用 `docker compose ps` 检查 18766 端口映射，再使用不带令牌的 `curl -i http://服务器IP:18766/mcp` 检查，返回 401 是预期结果。不要在排障输出中打印真实令牌。

MCP 会话空闲 30 分钟后失效，有请求会自动刷新计时，服务重启也会使旧会话失效。从 v0.1.11 起，内置 stdio 桥接会每 5 分钟空闲保活，并在收到明确的会话失效响应后自动重新初始化，至多重试一次。直接 HTTP 接入的客户端若持续提示“会话不存在”，需重新初始化 MCP 会话或重连；不要只重复发送旧的 `mcp-session-id`。这不表示访问令牌过期，也不需要因此重新签发令牌。网络超时的命令不会自动重发，以免重复执行。

HTTP 适用于可信内网，跨不可信网络使用 HTTPS 网关或 SSH 隧道。Docker 下用 SSH 隧道时，也要启用远程监听，并将 MCP 映射改为 `127.0.0.1:18766:18766`，客户端电脑执行 `ssh -N -L 127.0.0.1:18767:127.0.0.1:18766 SSH用户@服务器IP`，客户端 URL 改为 `http://127.0.0.1:18767/mcp`。无需在服务器安装 WorkBuddy。
