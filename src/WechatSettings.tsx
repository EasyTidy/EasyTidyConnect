import { useEffect, useState } from "react";
import { api, type WechatConfig } from "./api";
import { copyText } from "./clipboard";

const EMPTY: WechatConfig = { enabled: false, mode: "server", app_id: "", app_secret: "", signer_url: "", allowed_domains: "" };

/** 设置页：微信分享（公众号 JS-SDK）。 */
export default function WechatSettings({ notify }: { notify: (message: string, error?: boolean) => void }) {
  const [config, setConfig] = useState<WechatConfig>(EMPTY);
  const [saved, setSaved] = useState<WechatConfig>(EMPTY);
  const [busy, setBusy] = useState<"save" | "test" | null>(null);
  const [result, setResult] = useState<{ ok: boolean; text: string } | null>(null);
  const [showSecret, setShowSecret] = useState(false);

  useEffect(() => {
    api
      .wechatConfig()
      .then((value) => {
        const next = { ...value, mode: value.mode === "local" ? "local" : "server" } as WechatConfig;
        setConfig(next);
        setSaved(next);
      })
      .catch(() => undefined);
  }, []);

  const set = (patch: Partial<WechatConfig>) => {
    setConfig((current) => ({ ...current, ...patch }));
    setResult(null);
  };
  const local = config.mode === "local";
  const dirty = JSON.stringify(config) !== JSON.stringify(saved);
  const domain = config.allowed_domains.split(/[,，\s]+/).find(Boolean) ?? "love.example.com";
  // 独立的 wechat-signer 服务（crates/wechat-signer），Release 里提供 Docker 镜像和各平台二进制。
  const deployCommand = [
    "docker run -d --name wechat-signer --restart unless-stopped \\",
    "  -p 127.0.0.1:8080:8080 \\",
    `  -e WECHAT_APP_ID=${config.app_id || "wx你的AppID"} \\`,
    "  -e WECHAT_APP_SECRET=你的AppSecret \\",
    `  -e WECHAT_ALLOWED_DOMAINS=${config.allowed_domains.replace(/\s+/g, "") || domain} \\`,
    "  ghcr.io/easytidy/wechat-signer:latest",
  ].join("\n");

  const validate = () => {
    if (!config.enabled) return null;
    if (!config.signer_url.trim()) return local ? "请填写签名服务地址（内网穿透地址/wx/jssdk）" : "请填写签名服务器地址";
    if (local && (!config.app_id.trim() || !config.app_secret.trim())) return "本机签名需要填写 AppID 和 AppSecret";
    return null;
  };

  const persist = async () => {
    // 使用签名服务器时不在本机保存 AppSecret（后端同样会清空）。
    const next = local ? config : { ...config, app_secret: "" };
    await api.saveWechatConfig(next);
    setConfig(next);
    setSaved(next);
  };

  const save = async () => {
    const problem = validate();
    if (problem) {
      notify(problem, true);
      return;
    }
    setBusy("save");
    try {
      await persist();
      notify("微信分享设置已保存；重新发布页面后生效");
    } catch (error) {
      notify(`保存失败：${String(error)}`, true);
    } finally {
      setBusy(null);
    }
  };

  const test = async () => {
    setBusy("test");
    setResult(null);
    try {
      // 本机签名读取的是已保存的配置，先保存再测试。
      if (dirty) await persist();
      setResult({ ok: true, text: await api.testWechat(local ? config : { ...config, app_secret: "" }) });
    } catch (error) {
      setResult({ ok: false, text: String(error) });
    } finally {
      setBusy(null);
    }
  };

  const credentials = (
    <div className="form-grid">
      <div className="field">
        <label>AppID</label>
        <input value={config.app_id} placeholder="wx 开头，公众号后台 · 基本配置" onChange={(e) => set({ app_id: e.target.value.trim() })} />
      </div>
      {local && (
        <div className="field">
          <label>AppSecret</label>
          <div className="input-with-action">
            <input
              type={showSecret ? "text" : "password"}
              value={config.app_secret}
              placeholder="开发者密码"
              onChange={(e) => set({ app_secret: e.target.value.trim() })}
            />
            <button className="btn small subtle" onClick={() => setShowSecret((v) => !v)}>
              {showSecret ? "隐藏" : "显示"}
            </button>
          </div>
        </div>
      )}
    </div>
  );

  return (
    <section className="card" style={{ marginBottom: 16 }}>
      <div className="inline" style={{ justifyContent: "space-between", marginBottom: 6 }}>
        <h2 style={{ margin: 0 }}>微信分享</h2>
        <label className="inline">
          <input type="checkbox" checked={config.enabled} onChange={(event) => set({ enabled: event.target.checked })} />
          <span>启用</span>
        </label>
      </div>
      <p className="muted" style={{ marginTop: 0 }}>
        在微信里转发纪念日页面时，显示带标题、描述和缩略图的分享卡片。需要一个已认证的公众号，公众号密钥不会出现在发布的页面里。
      </p>

      <div className="wechat-layout">
        <div>
          <div className="field">
            <label>签名方式</label>
            <div className="segmented" role="radiogroup" aria-label="签名方式">
              <button role="radio" aria-checked={!local} className={!local ? "active" : ""} onClick={() => set({ mode: "server" })}>
                签名服务器（推荐）
              </button>
              <button role="radio" aria-checked={local} className={local ? "active" : ""} onClick={() => set({ mode: "local" })}>
                本机签名
              </button>
            </div>
            <span className="muted">
              {local
                ? "由这台电脑完成验证：需要在这里填写 AppID / AppSecret，并配置内网穿透，电脑需保持在线。"
                : "AppID / AppSecret 只放在你自己的服务器上，这里不需要填写，也不会保存。"}
            </span>
          </div>

          {local && credentials}

          <div className="field">
            <label>{local ? "签名服务地址" : "签名服务器地址"}</label>
            <input
              value={config.signer_url}
              placeholder={local ? "https://你的隧道地址/wx/jssdk" : "https://sign.example.com/wx/jssdk"}
              onChange={(e) => set({ signer_url: e.target.value.trim() })}
            />
          </div>
          <div className="field">
            <label>允许签名的域名</label>
            <input
              value={config.allowed_domains}
              placeholder="love.example.com, example.com（与公众号「JS 接口安全域名」一致）"
              onChange={(e) => set({ allowed_domains: e.target.value })}
            />
          </div>
          <div className="inline wrap">
            <button className="btn primary" disabled={busy !== null || !dirty} onClick={() => void save()}>
              {busy === "save" ? "保存中…" : "保存"}
            </button>
            <button
              className="btn"
              disabled={busy !== null || !config.signer_url.trim() || (local && (!config.app_id || !config.app_secret))}
              onClick={() => void test()}
            >
              {busy === "test" ? "测试中…" : "测试签名"}
            </button>
            {result && <span className={result.ok ? "test-ok" : "test-bad"}>{result.text}</span>}
          </div>
        </div>

        <div className="wechat-guide">
          <div className="section-label">上线前检查</div>
          <ol>
            <li>公众号已<b>认证</b>（未认证没有分享接口权限）</li>
            <li>页面域名已备案，并加入「设置与开发 → 公众号设置 → 功能设置 → <b>JS 接口安全域名</b>」</li>
            <li>{local ? "本机" : "签名服务器"}的公网 IP 加入「基本配置 → <b>IP 白名单</b>」</li>
            <li>签名服务地址使用 HTTPS（可用 Caddy / Nginx 反代）</li>
          </ol>
          {!local && (
            <>
              <div className="section-label" style={{ marginTop: 14 }}>
                部署签名服务器
              </div>
              <div className="field" style={{ marginBottom: 8 }}>
                <input
                  value={config.app_id}
                  placeholder="AppID（可选，仅用于生成下面的命令）"
                  onChange={(e) => set({ app_id: e.target.value.trim() })}
                />
              </div>
              <pre className="mono deploy-cmd">{deployCommand}</pre>
              <div className="inline wrap">
                <button className="btn small" onClick={() => void copyText(deployCommand).then((ok) => notify(ok ? "部署命令已复制" : "复制失败", !ok))}>
                  复制命令
                </button>
                <span className="muted">
                  也可以下载 Release 里的 wechat-signer 二进制（Linux / macOS / Windows）用 systemd 运行。部署后在服务器上执行
                  <code> wechat-signer check </code>确认 IP 白名单；AppSecret 只放在服务器上。
                </span>
              </div>
            </>
          )}
        </div>
      </div>
    </section>
  );
}
