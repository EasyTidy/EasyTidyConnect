import { useEffect, useRef, useState } from "react";
import QRCode from "qrcode";
import { openUrl } from "@tauri-apps/plugin-opener";
import { api, type AnniversaryProject, type ShareSettings } from "./api";
import { copyText } from "./clipboard";

/** 与后端 share::build_meta 保持一致的默认文案。 */
function defaults(project: AnniversaryProject) {
  const title = project.title.trim() || project.name.trim() || "送给你的一封信";
  const description =
    [project.opening, project.body, project.closing]
      .flatMap((text) => text.split("\n"))
      .map((line) => line.trim())
      .find(Boolean) ?? "";
  return { title, description };
}

const enc = encodeURIComponent;

export default function ShareDialog(props: {
  project: AnniversaryProject;
  url: string;
  /** 本地预览缩略图（发布后的页面用的是公开地址里的同一张图）。 */
  localThumb: string;
  onClose: () => void;
  onRepublish: () => Promise<void>;
  notify: (message: string, error?: boolean) => void;
}) {
  const { project, url } = props;
  const fallback = defaults(project);
  const [settings, setSettings] = useState<ShareSettings>({ title: "", description: "" });
  const [saved, setSaved] = useState<ShareSettings>({ title: "", description: "" });
  const [busy, setBusy] = useState(false);
  const qrRef = useRef<HTMLCanvasElement | null>(null);
  const [wechatReady, setWechatReady] = useState(false);

  useEffect(() => {
    api
      .wechatConfig()
      .then((value) => setWechatReady(value.enabled && Boolean(value.signer_url.trim())))
      .catch(() => setWechatReady(false));
  }, []);

  useEffect(() => {
    api
      .shareSettings(project.id)
      .then((value) => {
        setSettings(value);
        setSaved(value);
      })
      .catch(() => undefined);
  }, [project.id]);

  useEffect(() => {
    if (!qrRef.current) return;
    void QRCode.toCanvas(qrRef.current, url, { width: 168, margin: 1, color: { dark: "#111111", light: "#ffffff" } });
  }, [url]);

  const title = settings.title.trim() || fallback.title;
  const description = settings.description.trim() || fallback.description;
  const assetBase = url.replace(/index\.html$/, "");
  // 发布时生成的 300×300 分享缩略图（微信要求小图）
  const image = `${assetBase}share.jpg`;
  const host = (() => {
    try {
      return new URL(url).host;
    } catch {
      return url;
    }
  })();
  const dirty = settings.title !== saved.title || settings.description !== saved.description;

  const copy = async (text: string, label: string) => {
    const ok = await copyText(text);
    props.notify(ok ? `${label}已复制` : "复制失败，请手动复制", !ok);
  };

  const open = (target: string) =>
    openUrl(target).catch((error) => props.notify(`打开失败：${String(error)}`, true));

  const systemShare = async () => {
    try {
      await navigator.share({ title, text: description, url });
    } catch (error) {
      if ((error as Error)?.name !== "AbortError") props.notify(`系统分享不可用：${String(error)}`, true);
    }
  };

  const saveAndRepublish = async () => {
    setBusy(true);
    try {
      await api.saveShareSettings(project.id, settings);
      setSaved(settings);
      await props.onRepublish();
    } catch (error) {
      props.notify(`更新卡片失败：${String(error)}`, true);
    } finally {
      setBusy(false);
    }
  };

  const platforms = [
    { name: "微博", glyph: "微", href: `https://service.weibo.com/share/share.php?url=${enc(url)}&title=${enc(`${title} ${description}`)}&pic=${enc(image)}` },
    { name: "QQ", glyph: "Q", href: `https://connect.qq.com/widget/shareqq/index.html?url=${enc(url)}&title=${enc(title)}&desc=${enc(description)}&summary=${enc(description)}&pics=${enc(image)}` },
    { name: "QQ 空间", glyph: "空", href: `https://sns.qzone.qq.com/cgi-bin/qzshare/cgi_qzshare_onekey?url=${enc(url)}&title=${enc(title)}&desc=${enc(description)}&summary=${enc(description)}&pics=${enc(image)}` },
    { name: "X", glyph: "X", href: `https://twitter.com/intent/tweet?url=${enc(url)}&text=${enc(title)}` },
    { name: "Facebook", glyph: "f", href: `https://www.facebook.com/sharer/sharer.php?u=${enc(url)}` },
  ];

  return (
    <div className="modal-backdrop" onMouseDown={(event) => event.target === event.currentTarget && props.onClose()}>
      <div className="modal share-modal">
        <h2>分享纪念日</h2>
        <div className="share-layout">
          <div>
            <div className="section-label">卡片预览</div>
            {/* 微信聊天里的链接卡片 */}
            <div className="chat-preview">
              <div className="chat-row">
                <div className="chat-bubble wx-card">
                  <div className="wx-title">{title}</div>
                  <div className="wx-body">
                    <div className="wx-desc">{description || host}</div>
                    <div className="wx-thumb" style={{ backgroundImage: `url("${props.localThumb}")` }} />
                  </div>
                </div>
                <img className="chat-avatar" src="/app-icon.svg" alt="" />
              </div>
            </div>
            {/* 微博 / X / 浏览器等的大图卡片 */}
            <div className="og-card">
              <div
                className={`og-image ${project.photos.length ? "" : "fallback"}`}
                style={{ backgroundImage: `url("${props.localThumb}")` }}
              />
              <div className="og-text">
                <span>{host}</span>
                <strong>{title}</strong>
                <p>{description}</p>
              </div>
            </div>

            <div className="field" style={{ marginTop: 16 }}>
              <label>分享标题</label>
              <input
                value={settings.title}
                maxLength={40}
                placeholder={fallback.title}
                onChange={(event) => setSettings({ ...settings, title: event.target.value })}
              />
            </div>
            <div className="field">
              <label>分享描述</label>
              <input
                value={settings.description}
                maxLength={80}
                placeholder={fallback.description || "一句话介绍这封信"}
                onChange={(event) => setSettings({ ...settings, description: event.target.value })}
              />
            </div>
            <div className="inline wrap">
              <button className="btn primary" disabled={busy || !dirty} onClick={() => void saveAndRepublish()}>
                {busy ? "重新发布中…" : "保存并更新卡片"}
              </button>
              <span className="muted">修改后会自动重新发布页面。</span>
            </div>
            <div className="muted share-note">
              {wechatReady
                ? "已开启微信分享：在微信里打开链接后转发给朋友或朋友圈，会显示上面的完整卡片。"
                : "未开启微信分享时，部分微信版本只显示标题和链接。可在「设置 → 微信分享」配置公众号后重新发布。"}
            </div>
          </div>

          <div className="share-side">
            <div className="section-label">微信</div>
            <div className="share-qr">
              <canvas ref={qrRef} />
            </div>
            <div className="muted" style={{ textAlign: "center" }}>
              手机微信扫码打开，点右上角「···」
              <br />
              选择「发送给朋友」或「分享到朋友圈」
            </div>

            <div className="section-label" style={{ marginTop: 16 }}>
              链接
            </div>
            <div className="mono share-url">{url}</div>
            <div className="inline wrap" style={{ marginTop: 8 }}>
              <button className="btn small" onClick={() => void copy(url, "链接")}>
                复制链接
              </button>
              <button className="btn small" onClick={() => void copy(`${title}\n${description}\n${url}`.trim(), "分享文案")}>
                复制文案
              </button>
              {"share" in navigator && (
                <button className="btn small" onClick={() => void systemShare()}>
                  系统分享
                </button>
              )}
            </div>

            <div className="section-label" style={{ marginTop: 16 }}>
              其他平台
            </div>
            <div className="share-platforms">
              {platforms.map((item) => (
                <button key={item.name} className="share-platform" onClick={() => void open(item.href)} title={`分享到${item.name}`}>
                  <span>{item.glyph}</span>
                  {item.name}
                </button>
              ))}
            </div>
          </div>
        </div>

        <div className="modal-actions">
          <button className="btn" onClick={props.onClose}>
            完成
          </button>
        </div>
      </div>
    </div>
  );
}
