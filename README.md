# EasyTidy Connect

独立、跨平台的 NFC 局域网触发器。碰一下 NTAG216，就能触发一个本地动作。

> 当前是 M1：触发器、NFC 写入数据、局域网触发、运行历史、系统托盘、单实例、开机启动。
> AI、纪念日发布、EasyTidy 桥接、安装/更新在后续迭代加入。

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
- 标记“已写入”，首次成功触发后自动标记“已验证”；
- 系统托盘：关闭窗口隐藏到托盘，服务继续；
- 单实例：重复启动只唤醒已有窗口；
- 开机启动：设置页开关会写入/移除系统自启项。

## 尚未实现

- EasyTidy 本地桥接与真实任务/工作流触发；
- 跟随 EasyTidy / 独立 / 仅手动三种集成启动模式；
- AI provider 与纪念日模板/发布；
- S3 / Cloudflare 发布；
- 桌面 NFC 读卡器；
- 密钥库（当前 token 明文存在本地 SQLite，后续迁移到系统密钥库）；
- 自动更新、代码签名。

## 触发流程

1. 创建触发器；
2. 打开「NFC 标签」；
3. 复制 URI / 扫码 / 打开写入辅助页；
4. 手机 NFC 工具写入 NTAG216；
5. 碰一下标签，浏览器打开结果页并执行动作。

## License

MIT
