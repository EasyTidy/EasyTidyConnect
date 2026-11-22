# wechat-signer

微信公众号 JS-SDK 签名服务。EasyTidy Connect 发布到 S3 / R2 / Cloudflare Pages 的纪念日页面是纯静态的，
在微信里打开时会请求本服务获取 `wx.config` 签名，再设置「发送给朋友 / 分享到朋友圈」的卡片（标题、描述、缩略图）。

AppID / AppSecret 只保存在部署本服务的服务器上，不会出现在发布的页面里，桌面端也不需要填写 AppSecret。

## 部署前准备

| 要求 | 位置 |
|---|---|
| 已认证的公众号（未认证没有分享接口权限） | 微信公众平台 |
| 页面域名已备案，并加入 **JS 接口安全域名** | 设置与开发 → 公众号设置 → 功能设置 |
| 本服务所在服务器的公网 IP 加入 **IP 白名单** | 设置与开发 → 基本配置 |
| 一个 HTTPS 域名指向本服务，例如 `sign.example.com` | DNS + Caddy / Nginx |

## 部署

三种方式任选其一。配置项见 [deploy/wechat-signer.env.example](deploy/wechat-signer.env.example)。

### Docker

```sh
docker run -d --name wechat-signer --restart unless-stopped \
  -p 127.0.0.1:8080:8080 \
  -e WECHAT_APP_ID=wx0000000000000000 \
  -e WECHAT_APP_SECRET=你的AppSecret \
  -e WECHAT_ALLOWED_DOMAINS=love.example.com \
  ghcr.io/easytidy/wechat-signer:latest
```

或使用 [deploy/docker-compose.yml](deploy/docker-compose.yml)：把配置写进同目录的 `wechat-signer.env`，然后 `docker compose up -d`。

从源码构建镜像（在仓库根目录执行）：

```sh
docker build -f crates/wechat-signer/Dockerfile -t wechat-signer .
```

### 二进制 + systemd（Linux）

从 Release 下载对应平台的压缩包（`x86_64` / `aarch64`，Linux 版为静态链接，任何发行版都能运行）：

```sh
sudo install -m 755 wechat-signer /usr/local/bin/
sudo install -m 600 deploy/wechat-signer.env.example /etc/wechat-signer.env   # 然后编辑填写
sudo cp deploy/wechat-signer.service /etc/systemd/system/
sudo systemctl daemon-reload && sudo systemctl enable --now wechat-signer
```

### 直接运行（Windows / macOS / Linux）

```sh
WECHAT_APP_ID=wx... WECHAT_APP_SECRET=... WECHAT_ALLOWED_DOMAINS=love.example.com \
  wechat-signer --listen 127.0.0.1:8080
```

也可以把配置写进文件：`wechat-signer --config wechat-signer.env`（环境变量优先于文件）。

## HTTPS

页面在 HTTPS 下运行，签名接口也必须是 HTTPS。示例：

- Caddy（自动申请证书）：[deploy/Caddyfile](deploy/Caddyfile)
- Nginx：[deploy/nginx.conf](deploy/nginx.conf)

## 检查配置

部署后先在服务器上运行一次检查，会实际换取 ticket 并签名：

```sh
wechat-signer check --config /etc/wechat-signer.env
# Docker：docker exec wechat-signer /wechat-signer check
```

| 输出 | 处理 |
|---|---|
| `调用 IP x.x.x.x 不在公众号 IP 白名单中` | 把提示里的 IP 加入公众号 IP 白名单 |
| `AppSecret 无效` | 在公众号后台重置 AppSecret 后更新配置 |
| `公众号没有该接口权限` | 公众号需要完成认证 |
| `✓ 获取 jsapi_ticket 成功` | 服务端正常 |

最后在 EasyTidy Connect 的「设置 → 微信分享」选择「签名服务器」，签名服务器地址填
`https://sign.example.com/wx/jssdk`，点「测试签名」，然后重新发布页面。

## 接口

| 接口 | 说明 |
|---|---|
| `GET /wx/jssdk?url=<页面地址>` | 返回 `{ appId, timestamp, nonceStr, signature }`。页面地址的域名必须在 `WECHAT_ALLOWED_DOMAINS` 内，只对这些域名返回 CORS 头 |
| `GET /wx/health` | 健康检查：`{ status, version, configured }` |

## 行为说明

- **token 缓存**：jsapi_ticket 有效期约 2 小时，到期前 5 分钟才刷新，访问量再大也不会触发微信的每日调用上限。缓存在内存里，重启后重新获取一次。
- **stable_token**：默认使用 `/cgi-bin/stable_token`，不会让同一公众号其他系统正在使用的 access_token 失效。确实需要旧接口时设置 `WECHAT_TOKEN_API=classic`。
- **域名限制**：不设置 `WECHAT_ALLOWED_DOMAINS` 时任何网页都能用你的公众号获取签名（启动时会有警告）。微信仍会校验 JS 接口安全域名，但建议始终设置。
- **优雅退出**：收到 SIGTERM / Ctrl+C 后处理完当前请求再退出，适合容器和 systemd。
