import { useEffect, useState } from "react";
import { openUrl } from "@tauri-apps/plugin-opener";
import { api, type AboutInfo } from "./api";
import { copyText } from "./clipboard";

const OS_NAMES: Record<string, string> = { windows: "Windows", macos: "macOS", linux: "Linux" };

/** 关于（托盘菜单「关于 EasyTidy Connect」打开）。 */
export default function AboutDialog({ onClose, notify }: { onClose: () => void; notify: (message: string, error?: boolean) => void }) {
  const [info, setInfo] = useState<AboutInfo | null>(null);

  useEffect(() => {
    api.aboutInfo().then(setInfo).catch(() => setInfo(null));
  }, []);

  const rows: [string, string][] = info
    ? [
        ["版本", `v${info.version}`],
        ["运行环境", `${info.os_version || OS_NAMES[info.os] || info.os} · ${info.arch}`],
        ["Tauri", info.tauri_version],
        ["局域网服务", info.lan_address],
        ["数据目录", info.data_dir],
      ]
    : [];

  const copyInfo = async () => {
    if (!info) return;
    const text = [`${info.name} v${info.version}`, ...rows.slice(1).map(([k, v]) => `${k}：${v}`)].join("\n");
    const ok = await copyText(text);
    notify(ok ? "版本信息已复制" : "复制失败", !ok);
  };

  return (
    <div className="modal-backdrop" onMouseDown={(event) => event.target === event.currentTarget && onClose()}>
      <div className="modal about-modal">
        <div className="about-head">
          <img src="/app-icon.svg" alt="" />
          <div>
            <h2>EasyTidy Connect</h2>
            <div className="muted">碰一下 NFC，让局域网里发生一件有结果的事</div>
          </div>
        </div>
        <table className="table about-table">
          <tbody>
            {rows.map(([label, value]) => (
              <tr key={label}>
                <td>{label}</td>
                <td>{value}</td>
              </tr>
            ))}
          </tbody>
        </table>
        <div className="muted" style={{ marginTop: 12 }}>
          © {new Date().getFullYear()} EasyTidy · MIT License
        </div>
        <div className="modal-actions">
          <button className="btn" onClick={() => void copyInfo()} disabled={!info}>
            复制版本信息
          </button>
          <button className="btn" onClick={() => void api.openDataDir().catch((err) => notify(String(err), true))}>
            打开数据目录
          </button>
          <button className="btn" onClick={() => info && void openUrl(info.homepage)} disabled={!info}>
            项目主页
          </button>
          <button className="btn primary" onClick={onClose}>
            关闭
          </button>
        </div>
      </div>
    </div>
  );
}
