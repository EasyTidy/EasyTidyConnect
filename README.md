# EasyTidy Connect

独立、跨平台的 NFC 局域网触发器。碰一下 NTAG216，就能触发一个本地动作。

> 当前是 M3：触发器、NFC、EasyTidy Bridge、三种启动模式、纪念日工作室、AI 文案、S3/R2 发布。
> 自动更新、Cloudflare Pages、凭证加密在后续迭代加入。

## 技术栈

- **Tauri 2** + Rust
- React 19 + TypeScript + Vite
- Axum（局域网 HTTP 服务）
- SQLite + rusqlite

## 本地开发

```bash
npm install
npm run tauri dev
```

前端单独构建：

```bash
npm run build
```

Rust 检查：

```bash
cargo check -p connect-core
cargo check -p easytidy-connect
```

## 发布构建

```bash
npm run tauri build
```

产物在 `target/release/bundle/`。Windows 生成 NSIS/MSI；macOS/Linux 需在对应平台构建。

发布资产放在 `EasyTidyConnect` 的 `connect-latest` release：

- `connect-manifest.json`
- `EasyTidy.Connect_0.3.1_x64-setup.exe`

manifest 指向同 release 的安装包，EasyTidy 下载后校验大小与 SHA256。

## 目录

```text
crates/connect-core/   # 数据库、模型、Axum 服务
crates/connectd/       # 无 UI 的服务进程
src-tauri/             # Tauri 壳与 invoke 命令
src/                   # React UI
```

## 当前功能

- 创建/编辑/启用/删除触发器；
- 动作：测试消息、Webhook、打开 URL、EasyTidy（占位）；
- 每个触发器生成 NFC URI、二维码、写入辅助页；
- 写入辅助页：手机浏览器复制 URI，供 NFC Tools 写入；
- 局域网结果页 `/t/{publicId}?k={token}`；
- 运行历史、成功/失败状态；
- EasyTidy 任务/工作流运行后每 2 秒轮询 Bridge，自动更新为成功/失败；
- 标记“已写入”，首次成功触发后自动标记“已验证”；
- EasyTidy 本地 Bridge：
  - 从 EasyTidy 写入的 `connect-bridge.json` 自动导入地址/token；
  - 测试连接、读取任务/工作流列表；
  - NFC 动作直接触发 EasyTidy 任务或工作流；- 三种启动模式（配置 Bridge 后）：
  - 跟随 EasyTidy：EasyTidy「文件投递」页可一键启动，`--embedded --parent-pid`，父进程退出后自动退出；
  - 独立开机启动；
  - 仅手动；
- 纪念日工作室：
  - 项目编辑、5 套模板、照片、本地预览；
  - AI 文案（OpenAI 兼容 / Anthropic / Gemini）；
  - S3 / MinIO / Cloudflare R2 发布；
  - Cloudflare Pages Direct Upload；
  - 局域网穿透（Cloudflare Quick Tunnel）与 Cloudflare Tunnel 发布；
- 系统托盘：关闭窗口隐藏到托盘，服务继续；
- 单实例：重复启动只唤醒已有窗口；
- 开机启动：设置页开关会写入/移除系统自启项。

## 尚未实现

- Cloudflare Pages 真实账号 E2E（当前 mock 测试）；
- Cloudflare Named Tunnel 的 ingress 需用户在 Cloudflare 配置；
- AI 集成模式复用 EasyTidy provider/key；
- AI/发布凭证的系统密钥库加密（当前 SQLite 明文）；
- 发布回滚 UI、版本预览；
- 桌面 NFC 读卡器；
- 自动更新、代码签名；
- macOS/Linux 实机打包与验证。

## 触发流程

1. 创建触发器；
2. 打开「NFC 标签」；
3. 复制 URI / 扫码 / 打开写入辅助页；
4. 手机 NFC 工具写入 NTAG216；
5. 碰一下标签，浏览器打开结果页并执行动作。

## License

MIT
