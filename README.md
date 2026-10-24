# AstraLink

AstraLink 提供 SSH 连接管理和内置 MCP 服务，可安装桌面客户端，也可通过 Docker 部署网页版。下方镜像支持 `linux/amd64`。

![AstraLink 浏览器连接总览](docs/images/web-overview.png)

## Docker Compose 部署

在服务器上创建 `docker-compose.yml`，内容如下。首次部署前，将 `ASTRALINK_ADMIN_PASSWORD` 改成自己的密码（至少 12 个字符），不要提交含真实密码的文件。

```yaml
name: astralink
services:
  astralink:
    image: xinglinglove1029/astralink:v0.1.7
    restart: unless-stopped
    ports:
      - "1421:1421"
    environment:
      ASTRALINK_ADMIN_PASSWORD: ""
    volumes:
      - astralink-data:/var/lib/astralink
volumes:
  astralink-data:
```

在 `docker-compose.yml` 所在目录执行：

```bash
docker compose pull
docker compose up -d
docker compose ps
```

## Docker 部署

不使用 Compose 时，先拉取 Docker Hub 镜像，并创建持久卷：

```bash
docker pull xinglinglove1029/astralink:v0.1.7
docker volume create astralink-data
read -rsp '设置管理员密码（至少 12 个字符）: ' ASTRALINK_ADMIN_PASSWORD; printf '\n'
export ASTRALINK_ADMIN_PASSWORD
docker run -d --name astralink --restart unless-stopped \
  -p 1421:1421 \
  -v astralink-data:/var/lib/astralink \
  -e ASTRALINK_ADMIN_PASSWORD \
  xinglinglove1029/astralink:v0.1.7
unset ASTRALINK_ADMIN_PASSWORD
docker ps --filter name=astralink
```

部署后用浏览器打开 `http://服务器IP:1421/`，只输入管理员密码，不需要用户名。密码仅在全新数据卷首次启动时生效；重启或升级不会重置已有密码。Docker Compose 和单容器示例默认创建**不同**的命名卷，不要在两种部署方式间直接切换数据卷。

管理页通过 HTTP 访问时，密码和会话以明文传输；不可信网络请使用 HTTPS 反向代理。MCP 服务只监听容器内的 `127.0.0.1`，映射管理端口不会将 MCP 对外开放。更多运维说明见 [服务器部署说明](docs/server-deployment.md)。
