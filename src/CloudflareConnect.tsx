import { useEffect, useState } from "react";
import { openUrl } from "@tauri-apps/plugin-opener";
import { api, type CloudflareAccount, type CloudflareStatus } from "./api";

const EMPTY: CloudflareStatus = { connected: false, method: "", account_id: "", account_name: "", oauth_available: false, client_id: "" };

/**
 * Cloudflare 授权面板：一键 OAuth 授权；OAuth 不可用或失败时展开「API Token」方式。
 * `compact` 用于发布目标弹窗内（只显示状态和授权按钮）。
 */
export default function CloudflareConnect(props: {
  compact?: boolean;
  notify: (message: string, error?: boolean) => void;
  onChange?: (status: CloudflareStatus) => void;
  /** 发布目标弹窗：切换到手动填写完整表单 */
  onManual?: () => void;
}) {
  const [status, setStatus] = useState<CloudflareStatus>(EMPTY);
  const [loaded, setLoaded] = useState(false);
  const [busy, setBusy] = useState<"oauth" | "token" | "disconnect" | null>(null);
  const [error, setError] = useState("");
  const [showToken, setShowToken] = useState(false);
  const [token, setToken] = useState("");
  const [accounts, setAccounts] = useState<CloudflareAccount[]>([]);
  const [clientId, setClientId] = useState("");
  const [showClient, setShowClient] = useState(false);

  const apply = (next: CloudflareStatus) => {
    setStatus(next);
    setClientId(next.client_id);
    props.onChange?.(next);
    if (next.connected) {
      setShowToken(false);
      setError("");
      api.cloudflareAccounts().then(setAccounts).catch(() => setAccounts([]));
    }
  };

  useEffect(() => {
    api
      .cloudflareStatus()
      .then((value) => {
        apply(value);
        // 没有 OAuth client ID 时直接显示 Token 方式
        if (!value.connected && !value.oauth_available) setShowToken(true);
      })
      .catch(() => setShowToken(true))
      .finally(() => setLoaded(true));
  }, []);

  const authorize = async () => {
    setBusy("oauth");
    setError("");
    try {
      apply(await api.cloudflareAuthorize());
      props.notify("Cloudflare 已授权");
    } catch (err) {
      setError(String(err));
      setShowToken(true); // 授权失败：展开 Token 方式
    } finally {
      setBusy(null);
    }
  };

  const connectToken = async () => {
    setBusy("token");
    setError("");
    try {
      apply(await api.cloudflareConnectToken(token));
      setToken("");
      props.notify("Cloudflare 已连接");
    } catch (err) {
      setError(String(err));
    } finally {
      setBusy(null);
    }
  };

  const disconnect = async () => {
    if (!window.confirm("断开 Cloudflare 授权？使用授权的发布目标将无法发布，直到重新授权。")) return;
    setBusy("disconnect");
    try {
      apply(await api.cloudflareDisconnect());
      setAccounts([]);
    } finally {
      setBusy(null);
    }
  };

  const openTemplate = async () => {
    try {
      await openUrl(await api.cloudflareTokenTemplateUrl());
    } catch (err) {
      props.notify(`无法打开浏览器：${String(err)}`, true);
    }
  };

  if (!loaded) return null;

  const connected = status.connected && (
    <div className="cf-connected">
      <span className="cf-badge" aria-hidden="true">
        {"\uE73E"}
      </span>
      <div className="grow">
        <strong>已连接 Cloudflare</strong>
        <span>
          {status.account_name || status.account_id} · {status.method === "oauth" ? "OAuth 授权" : "API Token"}
        </span>
      </div>
      {!props.compact && accounts.length > 1 && (
        <select
          className="compact-select"
          value={status.account_id}
          onChange={(event) => {
            const account = accounts.find((item) => item.id === event.target.value);
            if (account) void api.cloudflareSelectAccount(account).then(apply);
          }}
          aria-label="Cloudflare 账户"
        >
          {accounts.map((account) => (
            <option key={account.id} value={account.id}>
              {account.name || account.id}
            </option>
          ))}
        </select>
      )}
      {!props.compact && (
        <button className="btn small" disabled={busy !== null} onClick={() => void disconnect()}>
          断开
        </button>
      )}
    </div>
  );

  const tokenForm = (
    <div className="cf-token">
      <div className="muted" style={{ marginBottom: 8 }}>
        {status.oauth_available ? "授权不可用时，" : ""}在 Cloudflare 创建一个已勾选好权限的 API Token，粘贴回来即可：
      </div>
      <div className="inline wrap" style={{ marginBottom: 8 }}>
        <button className="btn small" onClick={() => void openTemplate()}>
          1. 打开 Cloudflare 创建 Token（权限已预填）
        </button>
        <span className="muted">点「继续以显示摘要」→「创建令牌」→ 复制</span>
      </div>
      <div className="inline">
        <input
          className="compact-input grow"
          type="password"
          value={token}
          placeholder="2. 粘贴 API Token"
          onChange={(event) => setToken(event.target.value)}
          onKeyDown={(event) => {
            if (event.key === "Enter" && token.trim()) void connectToken();
          }}
        />
        <button className="btn primary small" disabled={busy !== null || !token.trim()} onClick={() => void connectToken()}>
          {busy === "token" ? "验证中…" : "连接"}
        </button>
      </div>
    </div>
  );

  return (
    <div className={`cf-connect ${props.compact ? "compact" : ""}`}>
      {connected || (
        <>
          <div className="cf-cta">
            <span className="cf-logo" aria-hidden="true" />
            <div className="grow">
              <strong>连接 Cloudflare</strong>
              <span>授权一次，之后添加发布目标只需填写名称</span>
            </div>
            {status.oauth_available && (
              <button className="btn primary" disabled={busy !== null} onClick={() => void authorize()}>
                {busy === "oauth" ? "等待浏览器授权…" : "授权 Cloudflare"}
              </button>
            )}
          </div>
          {error && <div className="cf-error">{error}</div>}
          {(showToken || !status.oauth_available) ? (
            tokenForm
          ) : (
            <div className="inline wrap">
              <button className="btn small subtle" onClick={() => setShowToken(true)}>
                改用 API Token
              </button>
              {props.onManual && (
                <button className="btn small subtle" onClick={props.onManual}>
                  手动配置
                </button>
              )}
            </div>
          )}
          {(showToken || !status.oauth_available) && props.onManual && (
            <button className="btn small subtle" onClick={props.onManual}>
              {error ? "授权失败？改为手动填写完整配置" : "手动配置（不使用授权）"}
            </button>
          )}
        </>
      )}

      {!props.compact && (
        <details className="advanced" open={showClient} onToggle={(e) => setShowClient((e.target as HTMLDetailsElement).open)}>
          <summary>OAuth 客户端</summary>
          <div className="muted" style={{ margin: "6px 0" }}>
            在你的 Cloudflare 账户「Manage Account → OAuth clients」创建：Grant type 为 authorization_code，Token
            认证方式选 <code>none</code>（PKCE），Redirect URL 填 <code>http://localhost:8976/oauth/callback</code>。
          </div>
          <div className="inline">
            <input
              className="compact-input grow"
              value={clientId}
              placeholder="OAuth Client ID"
              onChange={(event) => setClientId(event.target.value)}
            />
            <button
              className="btn small"
              disabled={clientId.trim() === status.client_id.trim()}
              onClick={() => void api.cloudflareSetClientId(clientId).then(apply).then(() => props.notify("已保存 Client ID"))}
            >
              保存
            </button>
          </div>
        </details>
      )}
    </div>
  );
}
