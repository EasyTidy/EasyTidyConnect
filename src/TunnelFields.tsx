import type { TunnelConfig } from "./api";

export const TUNNEL_KINDS = ["ngrok", "frp", "ssh_tunnel", "bore", "lan_tunnel", "cloudflare_tunnel"];

/** SSH 反向隧道的常见服务：选中后自动填好主机、端口、用户名和远端端口。 */
const SSH_PRESETS: { id: string; label: string; patch: Partial<TunnelConfig> }[] = [
  { id: "own", label: "自有服务器（VPS）", patch: { ssh_host: "", ssh_port: 22, ssh_user: "root", ssh_remote_port: 8080 } },
  { id: "localhost.run", label: "localhost.run（免费，免注册）", patch: { ssh_host: "localhost.run", ssh_port: 22, ssh_user: "nokey", ssh_remote_port: 80 } },
  { id: "pinggy", label: "Pinggy（免费 60 分钟）", patch: { ssh_host: "a.pinggy.io", ssh_port: 443, ssh_user: "tunnel", ssh_remote_port: 0 } },
];

/** 校验必填项，返回错误信息；通过时返回 null。 */
export function validateTunnel(kind: string, cfg: TunnelConfig): string | null {
  switch (kind) {
    case "ngrok":
      return cfg.ngrok_authtoken.trim() ? null : "请填写 ngrok Authtoken";
    case "frp":
      if (!cfg.frp_server_addr.trim()) return "请填写 frps 服务器地址";
      if (cfg.frp_proxy_type === "tcp") return cfg.frp_remote_port > 0 ? null : "请填写 frp 远端端口";
      return cfg.frp_custom_domain.trim() ? null : "请填写 frp 自定义域名";
    case "ssh_tunnel":
      return cfg.ssh_host.trim() ? null : "请填写 SSH 服务器";
    default:
      return null;
  }
}

export default function TunnelFields(props: {
  kind: string;
  config: TunnelConfig;
  onChange: (config: TunnelConfig) => void;
}) {
  const { kind, config } = props;
  const set = (patch: Partial<TunnelConfig>) => props.onChange({ ...config, ...patch });
  const text = (key: keyof TunnelConfig, label: string, placeholder = "", type = "text") => (
    <div className="field">
      <label>{label}</label>
      <input
        type={type}
        value={String(config[key] ?? "")}
        placeholder={placeholder}
        onChange={(event) => set({ [key]: event.target.value } as Partial<TunnelConfig>)}
      />
    </div>
  );
  const number = (key: keyof TunnelConfig, label: string, placeholder = "") => (
    <div className="field">
      <label>{label}</label>
      <input
        type="number"
        min={0}
        max={65535}
        value={Number(config[key]) || ""}
        placeholder={placeholder}
        onChange={(event) => set({ [key]: Math.max(0, Math.min(65535, Number(event.target.value) || 0)) } as Partial<TunnelConfig>)}
      />
    </div>
  );
  const sshPreset = SSH_PRESETS.find((preset) => preset.id !== "own" && preset.patch.ssh_host === config.ssh_host)?.id ?? "own";

  const advanced = (
    <details className="advanced">
      <summary>高级</summary>
      {text("public_url", "公网地址（可选）", "一般留空；使用自己的域名时填写")}
    </details>
  );

  if (kind === "ngrok") {
    return (
      <>
        {text("ngrok_authtoken", "Authtoken", "在 dashboard.ngrok.com 获取", "password")}
        {text("ngrok_domain", "固定域名（可选）", "例如 your-name.ngrok-free.app，留空为随机地址")}
        <div className="tip">不需要安装其他软件。免费账号可在 ngrok 后台领取一个固定域名，重新发布后地址不变。</div>
      </>
    );
  }

  if (kind === "frp") {
    return (
      <>
        <div className="form-grid">
          {text("frp_server_addr", "frps 服务器地址", "frp.example.com 或 IP")}
          {number("frp_server_port", "服务端口", "7000")}
        </div>
        {text("frp_token", "Token（可选）", "与 frps 的 auth.token 一致", "password")}
        <div className="field">
          <label>代理类型</label>
          <div className="segmented">
            {(["http", "tcp"] as const).map((type) => (
              <button key={type} className={config.frp_proxy_type === type ? "active" : ""} onClick={() => set({ frp_proxy_type: type })}>
                {type === "http" ? "HTTP（按域名）" : "TCP（按端口）"}
              </button>
            ))}
          </div>
        </div>
        {config.frp_proxy_type === "tcp" ? (
          number("frp_remote_port", "远端端口", "例如 6000，需在 frps allowPorts 范围内")
        ) : (
          <div className="form-grid">
            {text("frp_custom_domain", "自定义域名", "已解析到 frps 服务器")}
            {number("frp_http_port", "frps HTTP 端口", "80")}
          </div>
        )}
        {text("frpc_path", "程序位置（可选）", "一般留空，需要时会自动下载")}
        <div className="tip">首次使用会自动下载所需程序，不需要手动安装。</div>
        {advanced}
      </>
    );
  }

  if (kind === "ssh_tunnel") {
    return (
      <>
        <div className="field">
          <label>服务</label>
          <select
            value={sshPreset}
            onChange={(event) => {
              const preset = SSH_PRESETS.find((item) => item.id === event.target.value);
              if (preset) set(preset.patch);
            }}
          >
            {SSH_PRESETS.map((preset) => (
              <option key={preset.id} value={preset.id}>
                {preset.label}
              </option>
            ))}
          </select>
        </div>
        <div className="form-grid">
          {text("ssh_host", "SSH 服务器", "your-vps.example.com")}
          {number("ssh_port", "SSH 端口", "22")}
          {text("ssh_user", "用户名", "root")}
          {number("ssh_remote_port", "远端端口", "0 = 服务器分配")}
        </div>
        {sshPreset === "own" && (
          <>
            {text("ssh_password", "密码（与私钥二选一）", "", "password")}
            <div className="field">
              <label>私钥（可选）</label>
              <textarea
                className="code"
                value={config.ssh_key}
                placeholder={"粘贴 -----BEGIN OPENSSH PRIVATE KEY----- 内容，或填写私钥文件路径"}
                onChange={(event) => set({ ssh_key: event.target.value })}
              />
            </div>
            {config.ssh_key.trim() && text("ssh_key_passphrase", "私钥口令（可选）", "", "password")}
          </>
        )}
        <div className="tip">
          不需要安装其他软件。使用自己的服务器时，需要在服务器的 SSH 设置里开启
          <code>GatewayPorts yes</code> 并放行远端端口；localhost.run / Pinggy 不需要任何设置。
        </div>
        {advanced}
      </>
    );
  }

  if (kind === "bore") {
    return (
      <>
        <div className="form-grid">
          {text("bore_server", "bore 服务器", "bore.pub")}
          {number("bore_port", "远端端口", "0 = 随机")}
        </div>
        {text("bore_secret", "Secret（可选）", "自建 bore server --secret 时填写", "password")}
        <div className="tip">不需要安装其他软件。公共服务器 bore.pub 免注册，但地址随机且不保证稳定，长期使用建议用自己的服务器。</div>
        {advanced}
      </>
    );
  }

  return null;
}
