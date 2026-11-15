import { useEffect, useMemo, useRef, useState } from "react";
import QRCode from "qrcode";
import { api, type BridgeItem, type Run, type Settings, type Status, type TagPayload, type Trigger } from "./api";

type Page = "dashboard" | "triggers" | "tags" | "runs" | "settings";

const NAV: { id: Page; label: string; icon: string }[] = [
  { id: "dashboard", label: "仪表盘", icon: "◉" },
  { id: "triggers", label: "触发器", icon: "⚡" },
  { id: "tags", label: "NFC 标签", icon: "◎" },
  { id: "runs", label: "运行历史", icon: "☰" },
  { id: "settings", label: "设置", icon: "⚙" },
];

const ACTION_LABELS: Record<string, string> = {
  message: "测试消息",
  webhook: "调用 Webhook",
  open_url: "打开网页",
  easytidy_task: "运行 EasyTidy 任务",
  easytidy_workflow: "运行 EasyTidy 工作流",
  easytidy: "运行 EasyTidy",
};

const ACTION_OPTIONS = [
  { value: "message", label: "测试消息（验证标签是否可用）" },
  { value: "webhook", label: "调用 Webhook" },
  { value: "open_url", label: "打开网页" },
  { value: "easytidy_task", label: "运行 EasyTidy 任务（需 Bridge）" },
  { value: "easytidy_workflow", label: "运行 EasyTidy 工作流（需 Bridge）" },
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
  return ACTION_LABELS[trigger.action_type] ?? trigger.action_type;
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

  const activeTriggers = triggers.filter((trigger) => trigger.enabled).length;
  const successRuns = runs.filter((run) => run.status === "success").length;

  return (
    <div className="app">
      <aside className="sidebar">
        <div className="brand">
          <div className="brand-mark">C</div>
          <div>
            <div className="brand-title">EasyTidy Connect</div>
            <div className="brand-sub">NFC · LAN · Automation</div>
          </div>
        </div>
        <nav className="nav">
          {NAV.map((item) => (
            <button
              key={item.id}
              className={page === item.id ? "active" : ""}
              onClick={() => setPage(item.id)}
            >
              <span className="nav-icon">{item.icon}</span>
              {item.label}
            </button>
          ))}
        </nav>
        <div className="sidebar-footer">
          <div>
            <span className={`dot ${status?.running ? "" : "off"}`} />
            {status?.running ? "局域网服务运行中" : "服务未运行"}
          </div>
          <div>
            {status ? `${status.ip}:${status.port}` : "正在检测..."}
          </div>
          <div>v{status?.version ?? "0.1.0"} · 本地优先</div>
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
            triggers={triggers}
            selected={selectedTag}
            payload={payload}
            onSelect={setSelectedTag}
            onCopied={() => notify("已复制到剪贴板")}
            onMarkWritten={async (id) => {
              await api.markTagWritten(id);
              await refresh();
              notify("已标记为已写入");
            }}
            onOpenWriter={() => {
              if (payload) window.open(payload.write_page, "_blank");
            }}
          />
        )}
        {page === "runs" && <RunsPage runs={runs} onRefresh={refresh} />}
        {page === "settings" && (
          <SettingsPage
            settings={settings}
            status={status}
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
            1. 创建触发器 → 2. 生成 NFC 写入数据 → 3. 用手机 NFC 工具写入 NTAG216 → 4. 碰一下标签。
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
                <div className="row-sub">当前由手机 NFC 工具写入，Connect 负责生成可复制的数据。</div>
              </div>
              <button className="btn" onClick={() => onNavigate("tags")}>
                去写入
              </button>
            </div>
            <div className="row-card">
              <div className="grow">
                <div className="row-title">EasyTidy 集成</div>
                <div className="row-sub">桌面读卡器与整理流程触发在后续版本接入。</div>
              </div>
              <span className="pill neutral">规划中</span>
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
  triggers: Trigger[];
  selected: number | null;
  payload: TagPayload | null;
  onSelect: (id: number) => void;
  onCopied: () => void;
  onMarkWritten: (id: number) => void;
  onOpenWriter: () => void;
}) {
  const canvasRef = useRef<HTMLCanvasElement | null>(null);
  const trigger = props.triggers.find((item) => item.id === props.selected);
  const percent = props.payload ? Math.min(100, (props.payload.bytes / 888) * 100) : 0;

  useEffect(() => {
    if (!canvasRef.current || !props.payload) return;
    void QRCode.toCanvas(canvasRef.current, props.payload.uri, {
      width: 216,
      margin: 1,
      color: { dark: "#152238", light: "#ffffff" },
    });
  }, [props.payload]);

  const copy = async (value: string) => {
    await navigator.clipboard.writeText(value);
    props.onCopied();
  };

  return (
    <>
      <header className="page-head">
        <div>
          <h1>NFC 标签</h1>
          <p>生成写入数据；当前用手机 NFC 工具写入 NTAG216。</p>
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
            <div className="empty">先创建一个触发器，再生成 NFC 写入数据。</div>
          ) : (
            <>
              <h3>写入数据（URI）</h3>
              <div className="payload-row">
                <div className="mono">{props.payload.uri}</div>
                <button className="btn" onClick={() => void copy(props.payload!.uri)}>
                  复制
                </button>
              </div>
              <div className="muted" style={{ marginTop: 8 }}>
                {props.payload.bytes} / 888 字节
              </div>
              <div className="progress">
                <span style={{ width: `${percent}%` }} />
              </div>

              <div className="qr-wrap">
                <div className="qr-box">
                  <canvas ref={canvasRef} />
                </div>
                <div>
                  <h3>手机写入步骤</h3>
                  <div className="muted">
                    1. 手机打开 NFC Tools 等 App
                    <br />
                    2. 选择“写入 URL / 文本”
                    <br />
                    3. 粘贴上面的 URI
                    <br />
                    4. 贴近 NTAG216 写入
                    <br />
                    5. 回到这里点“已写入”
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

        <section className="card">
          <h2>为什么只写 URL？</h2>
          <div className="tip">
            NTAG216 只有 888 字节。一个触发 URL 约 70–110 字节，足够；纪念日页面、图片、正文都放在
            Connect 本地或发布目标里。
          </div>
          <div className="list" style={{ marginTop: 14 }}>
            <div className="row-card">
              <div className="grow">
                <div className="row-title">当前写入方式：手机</div>
                <div className="row-sub">桌面端暂不集成 NFC 读卡器。</div>
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
                <div className="row-title">IP 变化</div>
                <div className="row-sub">如果电脑 IP 变了，需要重新生成并重写标签。</div>
              </div>
            </div>
          </div>
        </section>
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
                  <td style={{ color: "#b42318" }}>{run.error ?? ""}</td>
                </tr>
              ))}
            </tbody>
          </table>
        )}
      </section>
    </>
  );
}

function SettingsPage(props: {
  settings: Settings;
  status: Status | null;
  onSave: (settings: Settings) => Promise<void>;
}) {
  const [draft, setDraft] = useState<Settings>(props.settings);
  const [mode, setMode] = useState<"follow" | "independent" | "manual">(
    props.settings.integration_mode ?? "manual",
  );
  const [bridgeTest, setBridgeTest] = useState<string>("");
  const [testing, setTesting] = useState(false);
  const [saving, setSaving] = useState(false);
  useEffect(() => {
    setDraft(props.settings);
    setMode(props.settings.integration_mode ?? "manual");
  }, [props.settings]);

  const hasBridge = Boolean(draft.bridge_url.trim() && draft.bridge_token.trim());

  const testBridge = async () => {
    setTesting(true);
    try {
      const health = await api.testBridge(draft.bridge_url.trim(), draft.bridge_token.trim());
      setBridgeTest(`已连接 ${health.app} ${health.version}（Bridge API v${health.bridgeApiVersion}）`);
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
          <p>服务、EasyTidy Bridge 与启动方式。</p>
        </div>
      </header>
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
            <span className="muted">修改端口后需要重启应用；当前版本不会热切换监听端口。</span>
          </div>

          <h3 style={{ marginTop: 22 }}>EasyTidy Bridge</h3>
          <div className="field">
            <label>Bridge 地址</label>
            <input
              value={draft.bridge_url}
              placeholder="http://127.0.0.1:53401"
              onChange={(event) => setDraft({ ...draft, bridge_url: event.target.value })}
            />
          </div>
          <div className="field">
            <label>Bridge Token</label>
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
                  <div className="muted">EasyTidy 启动时拉起 Connect；EasyTidy 退出后自动退出。</div>
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
                  <div className="muted">登录后自动启动，EasyTidy 未运行也保持托盘服务。</div>
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
                  <div className="muted">不写系统自启；由用户或 EasyTidy 显式启动。</div>
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
                还没配置 EasyTidy Bridge，所以这里只有“开机启动”开关。配置 Bridge 后会出现三种启动模式。
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
          <h2>服务信息</h2>
          <table className="table">
            <tbody>
              <tr>
                <td>状态</td>
                <td>{props.status?.running ? "运行中" : "未运行"}</td>
              </tr>
              <tr>
                <td>监听地址</td>
                <td>
                  {props.status?.ip}:{props.status?.port}
                </td>
              </tr>
              <tr>
                <td>版本</td>
                <td>{props.status?.version}</td>
              </tr>
              <tr>
                <td>数据库</td>
                <td>本地 SQLite</td>
              </tr>
              <tr>
                <td>EasyTidy Bridge</td>
                <td>{hasBridge ? "已配置" : "未配置"}</td>
              </tr>
            </tbody>
          </table>
          <div className="tip" style={{ marginTop: 14 }}>
            当前版本支持 EasyTidy 任务/工作流触发与三种启动模式；AI、发布、桌面读卡器在后续迭代加入。
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
      };
    } catch {
      return { url: "", id: "", name: "" };
    }
  }, [props.trigger]);

  const [name, setName] = useState(props.trigger?.name ?? "");
  const [actionType, setActionType] = useState(props.trigger?.action_type ?? "message");
  const [url, setUrl] = useState(initialConfig.url);
  const [targetId, setTargetId] = useState(initialConfig.id);
  const [targets, setTargets] = useState<BridgeItem[]>([]);
  const [targetError, setTargetError] = useState("");
  const [busy, setBusy] = useState(false);

  const needsUrl = actionType === "webhook" || actionType === "open_url";
  const needsEasyTidy =
    actionType === "easytidy_task" || actionType === "easytidy_workflow";

  useEffect(() => {
    if (!needsEasyTidy) return;
    let cancelled = false;
    setTargetError("");
    setTargets([]);

    api
      .settings()
      .then(async (settings) => {
        if (!settings.bridge_url.trim() || !settings.bridge_token.trim()) {
          setTargetError("请先在设置中配置 EasyTidy Bridge 地址与 token。");
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
      props.onError("请填写 URL");
      return;
    }
    if (needsEasyTidy && !targetId) {
      props.onError("请选择 EasyTidy 任务或工作流");
      return;
    }
    setBusy(true);
    try {
      const selected = targets.find((item) => String(item.id) === targetId);
      const config = needsUrl
        ? JSON.stringify({ url: url.trim() })
        : needsEasyTidy
          ? JSON.stringify({
              id: Number(targetId),
              name: selected?.name ?? initialConfig.name,
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
            <label>{actionType === "webhook" ? "Webhook URL" : "要打开的 URL"}</label>
            <input
              value={url}
              placeholder="https://example.com/hook"
              onChange={(event) => setUrl(event.target.value)}
            />
          </div>
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
