import { useEffect, useMemo, useRef, useState } from "react";
import QRCode from "qrcode";
import { openUrl } from "@tauri-apps/plugin-opener";
import StudioPage from "./StudioPage";
import { copyText } from "./clipboard";
import { useTheme, type Theme, type ThemeMode, type ThemeStyle } from "./theme";
import WechatSettings from "./WechatSettings";
import CloudflareConnect from "./CloudflareConnect";
import AboutDialog from "./AboutDialog";
import { PublishedPagesCard } from "./NfcWriteDialog";
import { listen } from "@tauri-apps/api/event";
import { api, type BridgeItem, type Run, type Settings, type Status, type TagPayload, type Trigger } from "./api";

type Page = "dashboard" | "triggers" | "tags" | "studio" | "runs" | "settings";

// 图标为 Segoe Fluent Icons 字形（Windows 11 自带，Windows 10 回退到 Segoe MDL2 Assets）。
const NAV: { id: Page; label: string; icon: string }[] = [
  { id: "dashboard", label: "仪表盘", icon: "\uE80F" },
  { id: "triggers", label: "触发器", icon: "\uE945" },
  { id: "tags", label: "NFC 标签", icon: "\uE8EC" },
  { id: "studio", label: "纪念日", icon: "\uEB51" },
  { id: "runs", label: "运行历史", icon: "\uE81C" },
];

const SETTINGS_NAV = { id: "settings" as Page, label: "设置", icon: "\uE713" };

const ACTION_LABELS: Record<string, string> = {
  message: "测试消息",
  webhook: "通知其他服务",
  open_url: "打开网页",
  easytidy_task: "运行 EasyTidy 任务",
  easytidy_workflow: "运行 EasyTidy 工作流",
  easytidy: "运行 EasyTidy",
  publish_anniversary: "打开纪念日",
};

const ACTION_OPTIONS = [
  { value: "message", label: "测试消息（验证标签是否可用）" },
  { value: "webhook", label: "通知其他服务（Webhook）" },
  { value: "open_url", label: "打开网页" },
  { value: "easytidy_task", label: "运行 EasyTidy 任务（需先连接 EasyTidy）" },
  { value: "easytidy_workflow", label: "运行 EasyTidy 工作流（需先连接 EasyTidy）" },
  { value: "publish_anniversary", label: "打开纪念日网页（有修改时自动重新发布）" },
];

function formatTime(value?: string | null) {
  if (!value) return "-";
  const date = new Date(value);
  if (Number.isNaN(date.getTime())) return value;
  return date.toLocaleString();
}

function actionSummary(trigger: Trigger) {
  if (trigger.action_type === "webhook" || trigger.action_type === "open_url") {
    try {
      const parsed = JSON.parse(trigger.action_config || "{}");
      if (parsed.url) return `${ACTION_LABELS[trigger.action_type]} · ${parsed.url}`;
    } catch {
      // ignore
    }
  }
  if (trigger.action_type === "easytidy_task" || trigger.action_type === "easytidy_workflow") {
    try {
      const parsed = JSON.parse(trigger.action_config || "{}");
      if (parsed.name) return `${ACTION_LABELS[trigger.action_type]} · ${parsed.name}`;
      if (parsed.id) return `${ACTION_LABELS[trigger.action_type]} · #${parsed.id}`;
    } catch {
      // ignore
    }
  }
  if (trigger.action_type === "publish_anniversary") {
    try {
      const parsed = JSON.parse(trigger.action_config || "{}");
      if (parsed.project_name) return `发布纪念日 · ${parsed.project_name}`;
      if (parsed.project_id) return `发布纪念日 · #${parsed.project_id}`;
    } catch {
      // ignore
    }
  }
  return ACTION_LABELS[trigger.action_type] ?? trigger.action_type;
}

const NAV_COLLAPSED_KEY = "connect.nav-collapsed";

function NavItem(props: {
  item: { id: Page; label: string; icon: string };
  active: boolean;
  collapsed: boolean;
  onClick: (page: Page) => void;
}) {
  const { item, active, collapsed } = props;
  return (
    <button
      className={active ? "active" : ""}
      aria-current={active ? "page" : undefined}
      aria-label={collapsed ? item.label : undefined}
      title={collapsed ? item.label : undefined}
      onClick={() => props.onClick(item.id)}
    >
      <span className="nav-icon" aria-hidden="true">{item.icon}</span>
      <span className="nav-label">{item.label}</span>
    </button>
  );
}

function StatusPill({ status }: { status: string }) {
  const cls = status === "success" ? "ok" : status === "failed" ? "bad" : "warn";
  const text = status === "success" ? "成功" : status === "failed" ? "失败" : "执行中";
  return <span className={`pill ${cls}`}>{text}</span>;
}

export default function App() {
  const [page, setPage] = useState<Page>("dashboard");
  const [status, setStatus] = useState<Status | null>(null);
  const [triggers, setTriggers] = useState<Trigger[]>([]);
  const [runs, setRuns] = useState<Run[]>([]);
  const [settings, setSettings] = useState<Settings>({
    device_name: "",
    port: 53400,
    auto_start: false,
    bridge_url: "",
    bridge_token: "",
    integration_mode: null,
  });
  const [editing, setEditing] = useState<Trigger | "new" | null>(null);
  const [selectedTag, setSelectedTag] = useState<number | null>(null);
  const [payload, setPayload] = useState<TagPayload | null>(null);
  const [toast, setToast] = useState<{ text: string; error?: boolean } | null>(null);
  const [theme, setTheme, micaActive] = useTheme();
  const [showAbout, setShowAbout] = useState(false);

  // 托盘菜单「关于」：后端打开主窗口后发送 show-about 事件
  useEffect(() => {
    if (!("__TAURI_INTERNALS__" in window)) return;
    const unlisten = listen("show-about", () => setShowAbout(true));
    return () => {
      void unlisten.then((stop) => stop());
    };
  }, []);
  // 导航栏折叠状态：记住用户的选择；首次使用时窄窗口默认折叠。
  const [navCollapsed, setNavCollapsed] = useState(() => {
    const saved = window.localStorage.getItem(NAV_COLLAPSED_KEY);
    return saved == null ? window.innerWidth < 1100 : saved === "1";
  });
  const toggleNav = () =>
    setNavCollapsed((current) => {
      window.localStorage.setItem(NAV_COLLAPSED_KEY, current ? "0" : "1");
      return !current;
    });

  const notify = (text: string, error = false) => {
    setToast({ text, error });
    window.setTimeout(() => setToast(null), 3600);
  };

  const refresh = async () => {
    try {
      const [nextStatus, nextTriggers, nextRuns, nextSettings] = await Promise.all([
        api.status(),
        api.triggers(),
        api.runs(50),
        api.settings(),
      ]);
      setStatus(nextStatus);
      setTriggers(nextTriggers);
      setRuns(nextRuns);
      setSettings(nextSettings);
      setSelectedTag((current) => current ?? nextTriggers[0]?.id ?? null);
    } catch (error) {
      notify(`无法连接本地服务：${String(error)}`, true);
    }
  };

  useEffect(() => {
    void refresh();
  }, []);

  useEffect(() => {
    if (selectedTag == null) {
      setPayload(null);
      return;
    }
    api
      .tagPayload(selectedTag)
      .then(setPayload)
      .catch((error) => notify(`生成标签失败：${String(error)}`, true));
  }, [selectedTag, triggers]);

  useEffect(() => {
    if (page !== "runs" && page !== "dashboard") return;
    const timer = window.setInterval(() => {
      void refresh();
    }, 3000);
    return () => window.clearInterval(timer);
  }, [page]);

  const activeTriggers = triggers.filter((trigger) => trigger.enabled).length;
  const successRuns = runs.filter((run) => run.status === "success").length;

  return (
    <div className="app">
      <aside className={`sidebar ${navCollapsed ? "collapsed" : ""}`}>
        <div className="pane-header">
          <button
            className="pane-toggle"
            onClick={toggleNav}
            title={navCollapsed ? "展开导航" : "折叠导航"}
            aria-label={navCollapsed ? "展开导航" : "折叠导航"}
            aria-expanded={!navCollapsed}
          >
            <span className="nav-icon" aria-hidden="true">{"\uE700"}</span>
          </button>
          <div className="brand">
            <img className="brand-mark" src="/app-icon.svg" alt="" />
            <div className="brand-text">
              <div className="brand-title">EasyTidy Connect</div>
              <div className="brand-sub">NFC · LAN · Automation</div>
            </div>
          </div>
        </div>
        <nav className="nav">
          {NAV.map((item) => (
            <NavItem key={item.id} item={item} active={page === item.id} collapsed={navCollapsed} onClick={setPage} />
          ))}
        </nav>
        <div className="sidebar-footer">
          <div
            className="service-status"
            title={navCollapsed ? `${status?.running ? "服务运行中" : "服务未运行"} · ${status ? `${status.ip}:${status.port}` : ""}` : undefined}
          >
            <span className={`dot ${status?.running ? "" : "off"}`} />
            <div className="service-text">
              <div>{status?.running ? "服务运行中" : "服务未运行"}</div>
              <div className="service-sub">
                {status ? `${status.ip}:${status.port}` : "正在检测..."} · v{status?.version ?? "-"}
              </div>
            </div>
          </div>
          <nav className="nav">
            <NavItem item={SETTINGS_NAV} active={page === SETTINGS_NAV.id} collapsed={navCollapsed} onClick={setPage} />
          </nav>
        </div>
      </aside>

      <main className="main">
        {page === "dashboard" && (
          <Dashboard
            status={status}
            triggers={triggers}
            runs={runs}
            activeTriggers={activeTriggers}
            successRuns={successRuns}
            onNavigate={setPage}
            onNew={() => setEditing("new")}
          />
        )}
        {page === "triggers" && (
          <TriggersPage
            triggers={triggers}
            onNew={() => setEditing("new")}
            onEdit={setEditing}
            onToggle={async (trigger) => {
              await api.setTriggerEnabled(trigger.id, !trigger.enabled);
              await refresh();
            }}
            onDelete={async (trigger) => {
              if (!window.confirm(`删除“${trigger.name}”？相关运行记录也会删除。`)) return;
              await api.deleteTrigger(trigger.id);
              if (selectedTag === trigger.id) setSelectedTag(null);
              await refresh();
              notify("触发器已删除");
            }}
            onNfc={(trigger) => {
              setSelectedTag(trigger.id);
              setPage("tags");
            }}
          />
        )}
        {page === "tags" && (
          <TagsPage
            notify={notify}
            onOpenStudio={() => setPage("studio")}
            triggers={triggers}
            selected={selectedTag}
            payload={payload}
            onSelect={setSelectedTag}
            onCopied={(ok) =>
              ok ? notify("已复制到剪贴板") : notify("复制失败，请手动选中文本复制", true)
            }
            onMarkWritten={async (id) => {
              await api.markTagWritten(id);
              await refresh();
              notify("已标记为已写入");
            }}
            onOpenWriter={() => {
              if (!payload) return;
              openUrl(payload.write_page).catch((error) =>
                notify(`打开写入辅助页失败：${String(error)}`, true),
              );
            }}
          />
        )}
        {page === "runs" && <RunsPage runs={runs} onRefresh={refresh} />}
        {page === "studio" && <StudioPage status={status} notify={notify} />}
        {page === "settings" && (
          <SettingsPage
            settings={settings}
            status={status}
            theme={theme}
            micaActive={micaActive}
            onThemeChange={setTheme}
            notify={notify}
            onSave={async (next) => {
              try {
                await api.saveSettings(next);
                setSettings(next);
                notify("设置已保存；修改端口后需要重启应用生效");
              } catch (error) {
                notify(`保存设置失败：${String(error)}`, true);
              }
            }}
          />
        )}
      </main>

      {editing && (
        <TriggerDialog
          trigger={editing === "new" ? null : editing}
          onClose={() => setEditing(null)}
          onSaved={async (message) => {
            setEditing(null);
            await refresh();
            notify(message);
          }}
          onError={(message) => notify(message, true)}
        />
      )}

      {showAbout && <AboutDialog onClose={() => setShowAbout(false)} notify={notify} />}
      {toast && <div className={`toast ${toast.error ? "error" : ""}`}>{toast.text}</div>}
    </div>
  );
}

function Dashboard(props: {
  status: Status | null;
  triggers: Trigger[];
  runs: Run[];
  activeTriggers: number;
  successRuns: number;
  onNavigate: (page: Page) => void;
  onNew: () => void;
}) {
  const { status, triggers, runs, activeTriggers, successRuns, onNavigate, onNew } = props;
  return (
    <>
      <header className="page-head">
        <div>
          <h1>仪表盘</h1>
          <p>碰一下 NFC，让局域网里发生一件有结果的事。</p>
        </div>
        <button className="btn primary" onClick={onNew}>
          + 新建触发器
        </button>
      </header>

      <div className="stat-row">
        <div className="stat">
          <div className="stat-label">服务状态</div>
          <div className="stat-value" style={{ fontSize: 20 }}>
            <span className={`dot ${status?.running ? "" : "off"}`} />
            {status?.running ? "运行中" : "未运行"}
          </div>
          <div className="muted">
            {status ? `${status.ip}:${status.port}` : "正在检测"}
          </div>
        </div>
        <div className="stat">
          <div className="stat-label">触发器</div>
          <div className="stat-value">{triggers.length}</div>
          <div className="muted">{activeTriggers} 个已启用</div>
        </div>
        <div className="stat">
          <div className="stat-label">成功触发</div>
          <div className="stat-value">{successRuns}</div>
          <div className="muted">最近 {runs.length} 次运行</div>
        </div>
      </div>

      <div className="grid two">
        <section className="card">
          <h2>开始使用</h2>
          <div className="tip">
            1. 创建触发器 → 2. 生成 NFC 写入数据 → 3. 用手机 NFC App 写入标签 → 4. 碰一下标签。
          </div>
          <div className="list" style={{ marginTop: 14 }}>
            <div className="row-card">
              <div className="grow">
                <div className="row-title">新建一个“测试消息”触发器</div>
                <div className="row-sub">不需要任何外部服务，先验证手机碰标签能否打开结果页。</div>
              </div>
              <button className="btn" onClick={onNew}>
                创建
              </button>
            </div>
            <div className="row-card">
              <div className="grow">
                <div className="row-title">NFC 标签写入</div>
                <div className="row-sub">用手机上的 NFC App 写入，这里会准备好要写入的链接。</div>
              </div>
              <button className="btn" onClick={() => onNavigate("tags")}>
                去写入
              </button>
            </div>
            <div className="row-card">
              <div className="grow">
                <div className="row-title">EasyTidy 集成</div>
                <div className="row-sub">连接 EasyTidy 后，碰一下标签就能运行 EasyTidy 任务或工作流。</div>
              </div>
              <button className="btn" onClick={() => onNavigate("settings")}>
                去配置
              </button>
            </div>
          </div>
        </section>

        <section className="card">
          <h2>最近运行</h2>
          {runs.length === 0 ? (
            <div className="empty">还没有运行记录。创建触发器后碰一下 NFC 标签。</div>
          ) : (
            <div className="list">
              {runs.slice(0, 6).map((run) => (
                <div key={run.id} className="row-card">
                  <div className="grow">
                    <div className="row-title">
                      {run.trigger_name} <StatusPill status={run.status} />
                    </div>
                    <div className="row-sub">{formatTime(run.started_at)}</div>
                  </div>
                </div>
              ))}
            </div>
          )}
        </section>
      </div>
    </>
  );
}

function TriggersPage(props: {
  triggers: Trigger[];
  onNew: () => void;
  onEdit: (trigger: Trigger) => void;
  onToggle: (trigger: Trigger) => void;
  onDelete: (trigger: Trigger) => void;
  onNfc: (trigger: Trigger) => void;
}) {
  return (
    <>
      <header className="page-head">
        <div>
          <h1>触发器</h1>
          <p>一个触发器 = 一个 NFC 标签 + 一个动作。</p>
        </div>
        <button className="btn primary" onClick={props.onNew}>
          + 新建触发器
        </button>
      </header>
      <section className="card">
        {props.triggers.length === 0 ? (
          <div className="empty">
            还没有触发器。
            <br />
            点击右上角“新建触发器”，先做一个测试消息。
          </div>
        ) : (
          <div className="list">
            {props.triggers.map((trigger) => (
              <div key={trigger.id} className="row-card">
                <div className="grow">
                  <div className="row-title">
                    {trigger.name}
                    <span className={`pill ${trigger.enabled ? "ok" : "neutral"}`}>
                      {trigger.enabled ? "启用" : "停用"}
                    </span>
                    {trigger.verified_at && <span className="pill ok">已验证</span>}
                  </div>
                  <div className="row-sub">{actionSummary(trigger)}</div>
                </div>
                <div className="row-actions">
                  <button className="btn small" onClick={() => props.onNfc(trigger)}>
                    NFC
                  </button>
                  <button className="btn small" onClick={() => props.onEdit(trigger)}>
                    编辑
                  </button>
                  <button className="btn small" onClick={() => props.onToggle(trigger)}>
                    {trigger.enabled ? "停用" : "启用"}
                  </button>
                  <button className="btn small danger" onClick={() => props.onDelete(trigger)}>
                    删除
                  </button>
                </div>
              </div>
            ))}
          </div>
        )}
      </section>
    </>
  );
}

function TagsPage(props: {
  notify: (message: string, error?: boolean) => void;
  onOpenStudio: () => void;
  triggers: Trigger[];
  selected: number | null;
  payload: TagPayload | null;
  onSelect: (id: number) => void;
  onCopied: (ok: boolean) => void;
  onMarkWritten: (id: number) => void;
  onOpenWriter: () => void;
}) {
  const canvasRef = useRef<HTMLCanvasElement | null>(null);
  const trigger = props.triggers.find((item) => item.id === props.selected);

  useEffect(() => {
    if (!canvasRef.current || !props.payload) return;
    // 二维码指向写入辅助页；若直接编码触发 URI，手机扫码会立刻执行动作。
    void QRCode.toCanvas(canvasRef.current, props.payload.write_page, {
      width: 216,
      margin: 1,
      color: { dark: "#152238", light: "#ffffff" },
    });
  }, [props.payload]);

  const copy = async (value: string) => {
    props.onCopied(await copyText(value));
  };

  return (
    <>
      <header className="page-head">
        <div>
          <h1>NFC 标签</h1>
          <p>准备好要写入的链接，用手机上的 NFC App 写入标签。</p>
        </div>
      </header>

      <div className="grid two">
        <section className="card">
          <h2>选择触发器</h2>
          <div className="field">
            <select
              value={props.selected ?? ""}
              onChange={(event) => props.onSelect(Number(event.target.value))}
            >
              <option value="" disabled>
                请选择
              </option>
              {props.triggers.map((item) => (
                <option key={item.id} value={item.id}>
                  {item.name}
                </option>
              ))}
            </select>
          </div>

          {!trigger || !props.payload ? (
            <div className="empty">先创建一个触发器，这里会生成要写入标签的链接。</div>
          ) : (
            <>
              <h3>要写入的链接</h3>
              <div className="payload-row">
                <div className="mono">{props.payload.uri}</div>
                <button className="btn" onClick={() => void copy(props.payload!.uri)}>
                  复制
                </button>
              </div>
              <div className="muted" style={{ marginTop: 8 }}>
                占用 {props.payload.bytes} 字节，请确认你的 NFC 标签容量足够。
              </div>

              <div className="qr-wrap">
                <div className="qr-box">
                  <canvas ref={canvasRef} />
                </div>
                <div>
                  <h3>手机写入步骤</h3>
                  <div className="muted">
                    1. 手机扫描左侧二维码，打开写入辅助页
                    <br />
                    2. 点“复制触发链接”
                    <br />
                    3. 在 NFC Tools 等 App 选择“写入 URL”并粘贴
                    <br />
                    4. 把手机贴近 NFC 标签完成写入
                    <br />
                    5. 回到这里点“标记为已写入”
                  </div>
                  <div className="inline" style={{ marginTop: 14, flexWrap: "wrap" }}>
                    <button className="btn small" onClick={() => void copy(props.payload!.write_page)}>
                      复制写入辅助页
                    </button>
                    <button className="btn small" onClick={props.onOpenWriter}>
                      打开写入辅助页
                    </button>
                  </div>
                </div>
              </div>

              <div className="inline" style={{ marginTop: 18 }}>
                <button
                  className="btn primary"
                  onClick={() => props.onMarkWritten(props.payload!.trigger_id)}
                  disabled={props.payload.tag_written}
                >
                  {props.payload.tag_written ? "已标记写入" : "标记为已写入"}
                </button>
                <button
                  className="btn"
                  onClick={async () => {
                    await copy(props.payload!.write_page);
                  }}
                >
                  发送到手机（复制辅助页）
                </button>
              </div>
            </>
          )}
        </section>

        <div>
        <PublishedPagesCard notify={props.notify} onOpenStudio={props.onOpenStudio} />
        <section className="card">
          <h2>为什么标签里只写一个链接？</h2>
          <div className="tip">
            NFC 标签容量很小，只够放一个链接。纪念日的文字、照片和音乐保存在电脑或网上，
            碰一下标签就会打开。
          </div>
          <div className="list" style={{ marginTop: 14 }}>
            <div className="row-card">
              <div className="grow">
                <div className="row-title">当前写入方式：手机</div>
                <div className="row-sub">暂不支持用电脑直接写入标签。</div>
              </div>
            </div>
            <div className="row-card">
              <div className="grow">
                <div className="row-title">写入后验证</div>
                <div className="row-sub">第一次碰标签成功触发后，状态自动变为“已验证”。</div>
              </div>
            </div>
            <div className="row-card">
              <div className="grow">
                <div className="row-title">电脑换了网络</div>
                <div className="row-sub">电脑换了网络后，需要重新写入标签。</div>
              </div>
            </div>
          </div>
        </section>
        </div>
      </div>
    </>
  );
}

function RunsPage({ runs, onRefresh }: { runs: Run[]; onRefresh: () => void }) {
  return (
    <>
      <header className="page-head">
        <div>
          <h1>运行历史</h1>
          <p>最近 50 次 NFC 触发记录。</p>
        </div>
        <button className="btn" onClick={onRefresh}>
          刷新
        </button>
      </header>
      <section className="card">
        {runs.length === 0 ? (
          <div className="empty">还没有运行记录。</div>
        ) : (
          <table className="table">
            <thead>
              <tr>
                <th>时间</th>
                <th>触发器</th>
                <th>状态</th>
                <th>来源</th>
                <th>EasyTidy 运行</th>
                <th>错误</th>
              </tr>
            </thead>
            <tbody>
              {runs.map((run) => (
                <tr key={run.id}>
                  <td>{formatTime(run.started_at)}</td>
                  <td>{run.trigger_name}</td>
                  <td>
                    <StatusPill status={run.status} />
                  </td>
                  <td>{run.source}</td>
                  <td style={{ fontFamily: "Cascadia Code, Consolas, monospace", fontSize: 11 }}>
                    {run.external_run_id ?? "-"}
                  </td>
                  <td className="error-text">{run.error ?? ""}</td>
                </tr>
              ))}
            </tbody>
          </table>
        )}
      </section>
    </>
  );
}

const THEME_STYLES: { id: ThemeStyle; name: string; desc: string }[] = [
  { id: "fluent", name: "Fluent", desc: "Windows 原生风格" },
  { id: "mica", name: "Fluent 云母", desc: "半透明云母背景（Windows 11）" },
  { id: "glass", name: "Liquid Glass", desc: "通透玻璃质感" },
];

const THEME_MODES: { id: ThemeMode; label: string; icon: string }[] = [
  { id: "system", label: "跟随系统", icon: "\uE7F4" },
  { id: "light", label: "浅色", icon: "\uE706" },
  { id: "dark", label: "深色", icon: "\uE708" },
];

function AppearanceCard({
  theme,
  micaActive,
  onChange,
}: {
  theme: Theme;
  micaActive: boolean | null;
  onChange: (theme: Theme) => void;
}) {
  return (
    <section className="card" style={{ marginBottom: 16 }}>
      <h2>外观</h2>
      <div className="theme-options" role="radiogroup" aria-label="界面风格">
        {THEME_STYLES.map((item) => (
          <button
            key={item.id}
            role="radio"
            aria-checked={theme.style === item.id}
            className={`theme-option ${theme.style === item.id ? "active" : ""}`}
            onClick={() => onChange({ ...theme, style: item.id })}
          >
            <span className={`theme-thumb ${item.id}`} />
            <strong>{item.name}</strong>
            <span>{item.desc}</span>
          </button>
        ))}
      </div>
      {theme.style === "mica" && micaActive === false && (
        <div className="tip" style={{ marginBottom: 16 }}>
          当前系统不支持云母效果（需要 Windows 11），已改用纯色背景。
        </div>
      )}
      <div className="field" style={{ marginBottom: 0 }}>
        <label>颜色模式</label>
        <div className="segmented" role="radiogroup" aria-label="颜色模式">
          {THEME_MODES.map((item) => (
            <button
              key={item.id}
              role="radio"
              aria-checked={theme.mode === item.id}
              className={theme.mode === item.id ? "active" : ""}
              onClick={() => onChange({ ...theme, mode: item.id })}
            >
              <span className="nav-icon" aria-hidden="true">{item.icon}</span>
              {item.label}
            </button>
          ))}
        </div>
      </div>
    </section>
  );
}

function SettingsPage(props: {
  settings: Settings;
  status: Status | null;
  theme: Theme;
  micaActive: boolean | null;
  onThemeChange: (theme: Theme) => void;
  notify: (message: string, error?: boolean) => void;
  onSave: (settings: Settings) => Promise<void>;
}) {
  const [draft, setDraft] = useState<Settings>(props.settings);
  const [mode, setMode] = useState<"follow" | "independent" | "manual">(
    props.settings.integration_mode ?? (props.settings.auto_start ? "independent" : "manual"),
  );
  const [bridgeTest, setBridgeTest] = useState<string>("");
  const [testing, setTesting] = useState(false);
  const [saving, setSaving] = useState(false);
  useEffect(() => {
    setDraft(props.settings);
    setMode(
      props.settings.integration_mode ??
        (props.settings.auto_start ? "independent" : "manual"),
    );
  }, [props.settings]);

  const hasBridge = Boolean(draft.bridge_url.trim() && draft.bridge_token.trim());

  const testBridge = async () => {
    setTesting(true);
    try {
      const health = await api.testBridge(draft.bridge_url.trim(), draft.bridge_token.trim());
      setBridgeTest(`已连接 ${health.app} ${health.version}`);
    } catch (error) {
      setBridgeTest(`连接失败：${String(error)}`);
    } finally {
      setTesting(false);
    }
  };

  const importBridge = async () => {
    try {
      const imported = await api.importBridge();
      setDraft((current) => ({
        ...current,
        bridge_url: imported.url,
        bridge_token: imported.token,
      }));
      setBridgeTest(`已从 ${imported.app} 导入；点击“测试连接”验证。`);
    } catch (error) {
      setBridgeTest(String(error));
    }
  };

  const save = async () => {
    setSaving(true);
    try {
      const next: Settings = {
        ...draft,
        auto_start: hasBridge ? mode === "independent" : draft.auto_start,
        integration_mode: hasBridge ? mode : null,
      };
      await props.onSave(next);
    } finally {
      setSaving(false);
    }
  };

  return (
    <>
      <header className="page-head">
        <div>
          <h1>设置</h1>
          <p>外观、连接 EasyTidy 与启动方式。</p>
        </div>
      </header>
      <AppearanceCard theme={props.theme} micaActive={props.micaActive} onChange={props.onThemeChange} />
      <section className="card" style={{ marginBottom: 16 }}>
        <h2>Cloudflare</h2>
        <CloudflareConnect notify={props.notify} />
      </section>
      <WechatSettings notify={props.notify} />
      <div className="grid two">
        <section className="card">
          <h2>常规</h2>
          <div className="field">
            <label>设备名</label>
            <input
              value={draft.device_name}
              onChange={(event) => setDraft({ ...draft, device_name: event.target.value })}
            />
          </div>
          <div className="field">
            <label>服务端口</label>
            <input
              type="number"
              value={draft.port}
              onChange={(event) => setDraft({ ...draft, port: Number(event.target.value) })}
            />
            <span className="muted">修改后需要重启应用才能生效。</span>
          </div>

          <h3 style={{ marginTop: 22 }}>连接 EasyTidy</h3>
          <div className="field">
            <label>连接地址</label>
            <input
              value={draft.bridge_url}
              placeholder="http://127.0.0.1:53401"
              onChange={(event) => setDraft({ ...draft, bridge_url: event.target.value })}
            />
          </div>
          <div className="field">
            <label>连接密钥</label>
            <input
              type="password"
              value={draft.bridge_token}
              placeholder="在 EasyTidy 的 Connect 设置中获取"
              onChange={(event) => setDraft({ ...draft, bridge_token: event.target.value })}
            />
          </div>
          <div className="inline" style={{ marginBottom: 12 }}>
            <button className="btn small" onClick={() => void testBridge()} disabled={testing || !hasBridge}>
              {testing ? "测试中..." : "测试连接"}
            </button>
            <button className="btn small" onClick={() => void importBridge()}>
              从 EasyTidy 导入
            </button>
            {bridgeTest && <span className="muted">{bridgeTest}</span>}
          </div>

          <h3 style={{ marginTop: 22 }}>启动方式</h3>
          {hasBridge ? (
            <div className="list">
              <label className="inline" style={{ alignItems: "flex-start" }}>
                <input
                  type="radio"
                  checked={mode === "follow"}
                  onChange={() => setMode("follow")}
                />
                <span>
                  <strong>跟随 EasyTidy</strong>
                  <div className="muted">打开 EasyTidy 时自动启动，关闭 EasyTidy 时一起退出。</div>
                </span>
              </label>
              <label className="inline" style={{ alignItems: "flex-start" }}>
                <input
                  type="radio"
                  checked={mode === "independent"}
                  onChange={() => setMode("independent")}
                />
                <span>
                  <strong>独立开机启动</strong>
                  <div className="muted">开机后自动启动，不打开 EasyTidy 也能使用。</div>
                </span>
              </label>
              <label className="inline" style={{ alignItems: "flex-start" }}>
                <input
                  type="radio"
                  checked={mode === "manual"}
                  onChange={() => setMode("manual")}
                />
                <span>
                  <strong>仅手动</strong>
                  <div className="muted">不自动启动，需要时手动打开。</div>
                </span>
              </label>
            </div>
          ) : (
            <>
              <label className="inline" style={{ marginBottom: 10 }}>
                <input
                  type="checkbox"
                  checked={draft.auto_start}
                  onChange={(event) => setDraft({ ...draft, auto_start: event.target.checked })}
                />
                <span style={{ fontSize: 13 }}>开机启动</span>
              </label>
              <div className="tip">
                连接 EasyTidy 后，还可以选择让 Connect 跟随 EasyTidy 一起启动和退出。
              </div>
            </>
          )}

          <div className="modal-actions">
            <button className="btn primary" onClick={() => void save()} disabled={saving}>
              {saving ? "保存中..." : "保存设置"}
            </button>
          </div>
        </section>

        <section className="card">
          <h2>运行状态</h2>
          <table className="table">
            <tbody>
              <tr>
                <td>状态</td>
                <td>{props.status?.running ? "运行中" : "未运行"}</td>
              </tr>
              <tr>
                <td>本机地址</td>
                <td>
                  {props.status?.ip}:{props.status?.port}
                </td>
              </tr>
              <tr>
                <td>版本</td>
                <td>{props.status?.version}</td>
              </tr>
              <tr>
                <td>数据</td>
                <td>保存在本机</td>
              </tr>
              <tr>
                <td>EasyTidy</td>
                <td>{hasBridge ? "已连接" : "未连接"}</td>
              </tr>
            </tbody>
          </table>
          <div className="tip" style={{ marginTop: 14 }}>
            数据只保存在这台电脑上。关闭窗口后应用会留在系统托盘，标签随时可用。
          </div>
        </section>
      </div>
    </>
  );
}

function TriggerDialog(props: {
  trigger: Trigger | null;
  onClose: () => void;
  onSaved: (message: string) => void;
  onError: (message: string) => void;
}) {
  const initialConfig = useMemo(() => {
    if (!props.trigger) return { url: "", id: "", name: "" };
    try {
      const parsed = JSON.parse(props.trigger.action_config || "{}");
      return {
        url: parsed.url ?? "",
        id: parsed.id != null ? String(parsed.id) : "",
        name: parsed.name ?? "",
        projectId: parsed.project_id != null ? String(parsed.project_id) : "",
        targetId: parsed.target_id != null ? String(parsed.target_id) : "",
        projectName: parsed.project_name ?? "",
      };
    } catch {
      return { url: "", id: "", name: "", projectId: "", targetId: "", projectName: "" };
    }
  }, [props.trigger]);

  const [name, setName] = useState(props.trigger?.name ?? "");
  const [actionType, setActionType] = useState(props.trigger?.action_type ?? "message");
  const [url, setUrl] = useState(initialConfig.url);
  const [targetId, setTargetId] = useState(initialConfig.id);
  const [targets, setTargets] = useState<BridgeItem[]>([]);
  const [targetError, setTargetError] = useState("");
  const [projects, setProjects] = useState<{ id: number; name: string }[]>([]);
  const [projectId, setProjectId] = useState(initialConfig.projectId);
  const [publishTargets, setPublishTargets] = useState<{ id: number; name: string }[]>([]);
  const [publishTargetId, setPublishTargetId] = useState(initialConfig.targetId);
  const [anniversaryError, setAnniversaryError] = useState("");
  const [busy, setBusy] = useState(false);

  const needsUrl = actionType === "webhook" || actionType === "open_url";
  const needsEasyTidy =
    actionType === "easytidy_task" || actionType === "easytidy_workflow";
  const needsAnniversary = actionType === "publish_anniversary";

  useEffect(() => {
    if (!needsAnniversary) return;
    let cancelled = false;
    setAnniversaryError("");
    Promise.all([api.projects(), api.publishTargets()])
      .then(([projectList, targetList]) => {
        if (cancelled) return;
        setProjects(projectList.map((item) => ({ id: item.id, name: item.name })));
        setPublishTargets(targetList.map((item) => ({ id: item.id, name: item.name })));
        setProjectId((current) => current || (projectList[0] ? String(projectList[0].id) : ""));
        setPublishTargetId((current) => current || (targetList[0] ? String(targetList[0].id) : ""));
        if (projectList.length === 0 || targetList.length === 0) {
          setAnniversaryError("请先在「纪念日」页创建项目，并添加发布目标。");
        }
      })
      .catch((error) => {
        if (!cancelled) setAnniversaryError(`读取纪念日数据失败：${String(error)}`);
      });
    return () => {
      cancelled = true;
    };
  }, [needsAnniversary]);

  useEffect(() => {
    if (!needsEasyTidy) return;
    let cancelled = false;
    setTargetError("");
    setTargets([]);

    api
      .settings()
      .then(async (settings) => {
        if (!settings.bridge_url.trim() || !settings.bridge_token.trim()) {
          setTargetError("请先在设置中连接 EasyTidy。");
          return;
        }
        const list =
          actionType === "easytidy_task"
            ? await api.bridgeTasks(settings.bridge_url, settings.bridge_token)
            : await api.bridgeWorkflows(settings.bridge_url, settings.bridge_token);
        if (!cancelled) {
          setTargets(list);
          setTargetId((current) => current || (list[0]?.id != null ? String(list[0].id) : ""));
        }
      })
      .catch((error) => {
        if (!cancelled) setTargetError(`无法读取 EasyTidy 列表：${String(error)}`);
      });

    return () => {
      cancelled = true;
    };
  }, [actionType, needsEasyTidy]);

  const save = async () => {
    if (!name.trim()) {
      props.onError("请填写触发器名称");
      return;
    }
    if (needsUrl && !url.trim()) {
      props.onError("请填写网址");
      return;
    }
    if (needsEasyTidy && !targetId) {
      props.onError("请选择 EasyTidy 任务或工作流");
      return;
    }
    if (actionType === "publish_anniversary" && (!projectId || !publishTargetId)) {
      props.onError("请选择纪念日项目和发布目标");
      return;
    }
    setBusy(true);
    try {
      const selected = targets.find((item) => String(item.id) === targetId);
      const project = projects.find((item) => String(item.id) === projectId);
      const config = needsUrl
        ? JSON.stringify({ url: url.trim() })
        : needsEasyTidy
          ? JSON.stringify({
              id: Number(targetId),
              name: selected?.name ?? initialConfig.name,
            })
          : actionType === "publish_anniversary"
            ? JSON.stringify({
                project_id: Number(projectId),
                project_name: project?.name ?? "",
                target_id: Number(publishTargetId),
              })
            : "{}";
      if (props.trigger) {
        await api.updateTrigger(
          props.trigger.id,
          name.trim(),
          props.trigger.enabled,
          actionType,
          config,
        );
        props.onSaved("触发器已更新");
      } else {
        await api.createTrigger(name.trim(), actionType, config);
        props.onSaved("触发器已创建");
      }
    } catch (error) {
      props.onError(`保存失败：${String(error)}`);
    } finally {
      setBusy(false);
    }
  };

  return (
    <div className="modal-backdrop" onMouseDown={(event) => event.target === event.currentTarget && props.onClose()}>
      <div className="modal">
        <h2>{props.trigger ? "编辑触发器" : "新建触发器"}</h2>
        <div className="field">
          <label>名称</label>
          <input
            autoFocus
            value={name}
            placeholder="例如：碰一下整理照片"
            onChange={(event) => setName(event.target.value)}
          />
        </div>
        <div className="field">
          <label>动作</label>
          <select value={actionType} onChange={(event) => setActionType(event.target.value)}>
            {ACTION_OPTIONS.map((option) => (
              <option key={option.value} value={option.value}>
                {option.label}
              </option>
            ))}
          </select>
        </div>
        {needsUrl && (
          <div className="field">
            <label>{actionType === "webhook" ? "通知地址（Webhook URL）" : "要打开的网址"}</label>
            <input
              value={url}
              placeholder="https://example.com/hook"
              onChange={(event) => setUrl(event.target.value)}
            />
          </div>
        )}
        {needsAnniversary && (
          <>
            <div className="tip" style={{ marginBottom: 14 }}>
              碰标签会直接打开纪念日网页（内容有修改时先自动重新发布）。注意：这种方式只在手机和电脑连同一个 Wi-Fi 时
              可用；想让 TA 在任何地方碰一下都能打开，请在「纪念日 → 发布」发布后点「写入 NFC」，把网页地址直接写进标签。
            </div>
            <div className="field">
              <label>纪念日项目</label>
              {anniversaryError ? (
                <div className="tip">{anniversaryError}</div>
              ) : (
                <select value={projectId} onChange={(event) => setProjectId(event.target.value)}>
                  <option value="" disabled>
                    请选择
                  </option>
                  {projects.map((project) => (
                    <option key={project.id} value={project.id}>
                      {project.name} (#{project.id})
                    </option>
                  ))}
                </select>
              )}
            </div>
            <div className="field">
              <label>发布目标</label>
              <select
                value={publishTargetId}
                onChange={(event) => setPublishTargetId(event.target.value)}
              >
                <option value="" disabled>
                  请选择
                </option>
                {publishTargets.map((target) => (
                  <option key={target.id} value={target.id}>
                    {target.name} (#{target.id})
                  </option>
                ))}
              </select>
            </div>
          </>
        )}
        {needsEasyTidy && (
          <div className="field">
            <label>
              {actionType === "easytidy_task" ? "EasyTidy 任务" : "EasyTidy 工作流"}
            </label>
            {targetError ? (
              <div className="tip">{targetError}</div>
            ) : (
              <select value={targetId} onChange={(event) => setTargetId(event.target.value)}>
                <option value="" disabled>
                  {targets.length === 0 ? "正在读取..." : "请选择"}
                </option>
                {targets.map((item) => (
                  <option key={item.id} value={item.id}>
                    {item.name} (#{item.id})
                  </option>
                ))}
              </select>
            )}
          </div>
        )}
        {actionType === "easytidy" && (
          <div className="tip">请选择“运行 EasyTidy 任务”或“运行 EasyTidy 工作流”。</div>
        )}
        <div className="modal-actions">
          <button className="btn" onClick={props.onClose} disabled={busy}>
            取消
          </button>
          <button className="btn primary" onClick={() => void save()} disabled={busy}>
            {busy ? "保存中..." : "保存"}
          </button>
        </div>
      </div>
    </div>
  );
}
