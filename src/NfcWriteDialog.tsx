import { useEffect, useRef, useState } from "react";
import QRCode from "qrcode";
import { api, type NfcLinkInfo } from "./api";
import { copyText } from "./clipboard";

const TUNNEL_KINDS = ["ngrok", "frp", "ssh_tunnel", "bore", "lan_tunnel", "cloudflare_tunnel"];

/**
 * 把发布后的公开网页链接写入 NFC：标签里直接是网页地址，碰一下就打开，不依赖电脑和局域网。
 * 手机扫码打开局域网写入辅助页 → 复制链接 → 用 NFC App 写入。
 */
export default function NfcWriteDialog(props: {
  url: string;
  name: string;
  /** 发布目标类型：隧道类地址依赖电脑在线，需提示 */
  targetKind?: string;
  onClose: () => void;
  notify: (message: string, error?: boolean) => void;
}) {
  const [info, setInfo] = useState<NfcLinkInfo | null>(null);
  const [error, setError] = useState("");
  const qrRef = useRef<HTMLCanvasElement | null>(null);
  const isTunnel = TUNNEL_KINDS.includes(props.targetKind ?? "");
  const temporary = props.targetKind === "lan_tunnel" || props.targetKind === "bore";
  const local = props.targetKind === "local";

  useEffect(() => {
    api
      .nfcLinkInfo(props.url, props.name)
      .then(setInfo)
      .catch((err) => setError(String(err)));
  }, [props.url, props.name]);

  useEffect(() => {
    if (!qrRef.current || !info) return;
    void QRCode.toCanvas(qrRef.current, info.write_page, { width: 200, margin: 1, color: { dark: "#111111", light: "#ffffff" } });
  }, [info]);

  const copy = async () => {
    const ok = await copyText(props.url);
    props.notify(ok ? "网页链接已复制" : "复制失败，请手动复制", !ok);
  };

  return (
    <div className="modal-backdrop" onMouseDown={(event) => event.target === event.currentTarget && props.onClose()}>
      <div className="modal nfc-modal">
        <h2>写入 NFC 标签</h2>
        <p className="muted" style={{ marginTop: -8 }}>
          标签里直接写入发布后的网页地址，碰一下就打开「{props.name}」。
        </p>

        {temporary && (
          <div className="cf-error" style={{ marginBottom: 12 }}>
            这是一个临时地址，电脑重启后可能会变化，写进标签后可能失效。建议发布到云端后再写入。
          </div>
        )}
        {local && (
          <div className="tip" style={{ marginBottom: 12 }}>
            这是本机发布的地址：只有连在同一个 Wi-Fi 下的手机能打开，电脑需保持开机；电脑换了网络后需要重新写入。
            想在任何地方都能打开，请发布到云端。
          </div>
        )}
        {isTunnel && !temporary && (
          <div className="tip" style={{ marginBottom: 12 }}>
            这个地址需要电脑保持开机并运行 EasyTidy Connect，标签才能打开。长期使用建议发布到云端。
          </div>
        )}

        <div className="nfc-layout">
          <div className="nfc-qr">
            {info ? <canvas ref={qrRef} /> : <div className="muted">{error || "生成中…"}</div>}
            <span className="muted">手机（与电脑同一 Wi-Fi）扫码</span>
          </div>
          <div className="grow">
            <ol className="nfc-steps">
              <li>手机扫描左侧二维码，打开写入辅助页</li>
              <li>点「复制网页链接」</li>
              <li>打开 NFC Tools 等 App，选择「写入 → URL」并粘贴</li>
              <li>把手机贴近 NFC 标签完成写入，碰一下试试</li>
            </ol>
            <div className="field" style={{ marginBottom: 8 }}>
              <label>网页链接</label>
              <div className="mono">{props.url}</div>
            </div>
            <div className="inline wrap">
              <button className="btn small" onClick={() => void copy()}>
                复制链接
              </button>
              {info && <span className="muted">占用 {info.bytes} 字节，请确认你的 NFC 标签容量足够。</span>}
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

type PublishedPage = { projectId: number; name: string; url: string; kind: string; publishedAt: string };

/** NFC 标签页：列出已发布的纪念日网页，一键写入 NFC。 */
export function PublishedPagesCard({ notify, onOpenStudio }: { notify: (message: string, error?: boolean) => void; onOpenStudio: () => void }) {
  const [pages, setPages] = useState<PublishedPage[] | null>(null);
  const [writing, setWriting] = useState<PublishedPage | null>(null);

  useEffect(() => {
    let cancelled = false;
    void (async () => {
      try {
        const projects = await api.projects();
        const latest = await Promise.all(
          projects.map(async (project) => {
            const versions = await api.projectVersions(project.id).catch(() => []);
            const version = versions[0];
            return version
              ? { projectId: project.id, name: project.name, url: version.published_url, kind: version.target_kind, publishedAt: version.published_at }
              : null;
          }),
        );
        if (!cancelled) setPages(latest.filter((item): item is PublishedPage => item !== null));
      } catch {
        if (!cancelled) setPages([]);
      }
    })();
    return () => {
      cancelled = true;
    };
  }, []);

  return (
    <section className="card" style={{ marginBottom: 16 }}>
      <h2>纪念日网页</h2>
      {pages === null ? (
        <div className="muted">读取中…</div>
      ) : pages.length === 0 ? (
        <div className="empty">
          还没有发布过纪念日网页。
          <br />
          <button className="btn small" style={{ marginTop: 10 }} onClick={onOpenStudio}>
            去纪念日工作室发布
          </button>
        </div>
      ) : (
        <div className="list">
          {pages.map((page) => (
            <div key={page.projectId} className="row-card">
              <div className="grow">
                <div className="row-title">
                  {page.name}
                  {page.kind === "local" && <span className="pill neutral">局域网</span>}
                </div>
                <div className="row-sub" title={page.url}>
                  {page.url}
                </div>
              </div>
              <button className="btn small primary" onClick={() => setWriting(page)}>
                写入 NFC
              </button>
            </div>
          ))}
        </div>
      )}
      <div className="muted" style={{ marginTop: 10 }}>
        纪念日直接写入发布后的网页地址，碰一下就打开网页；下方的触发器链接只在家里的 Wi-Fi 下可用。
      </div>
      {writing && (
        <NfcWriteDialog url={writing.url} name={writing.name} targetKind={writing.kind} onClose={() => setWriting(null)} notify={notify} />
      )}
    </section>
  );
}
