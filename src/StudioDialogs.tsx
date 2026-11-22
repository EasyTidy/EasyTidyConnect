import { useState } from "react";
import { api, EMPTY_TUNNEL_CONFIG, type AiProviderInput, type PublishTargetInput, type TunnelConfig } from "./api";
import TunnelFields, { validateTunnel } from "./TunnelFields";
import CloudflareConnect from "./CloudflareConnect";

export async function readBase64(file: File): Promise<string> {
  return new Promise((resolve, reject) => {
    const reader = new FileReader();
    reader.onload = () => {
      const result = String(reader.result ?? "");
      resolve(result.includes(",") ? result.split(",")[1] : result);
    };
    reader.onerror = () => reject(reader.error ?? new Error("读取文件失败"));
    reader.readAsDataURL(file);
  });
}

export function ProviderDialog(props: {
  onClose: () => void;
  onSaved: () => void;
  onError: (message: string) => void;
}) {
  const [form, setForm] = useState<AiProviderInput>({
    name: "OpenAI",
    kind: "openai",
    base_url: "https://api.openai.com/v1",
    model: "gpt-4o-mini",
    api_key: "",
    is_default: true,
  });

  const save = async () => {
    if (!form.name.trim() || !form.model.trim()) {
      props.onError("请填写名称和模型");
      return;
    }
    try {
      await api.createAiProvider(form);
      props.onSaved();
    } catch (error) {
      props.onError(`保存失败：${String(error)}`);
    }
  };

  return (
    <div className="modal-backdrop">
      <div className="modal">
        <h2>添加 AI Provider</h2>
        <div className="grid two" style={{ gridTemplateColumns: "1fr 1fr" }}>
          <div className="field">
            <label>名称</label>
            <input value={form.name} onChange={(e) => setForm({ ...form, name: e.target.value })} />
          </div>
          <div className="field">
            <label>类型</label>
            <select value={form.kind} onChange={(e) => setForm({ ...form, kind: e.target.value })}>
              <option value="openai">OpenAI 兼容</option>
              <option value="anthropic">Anthropic</option>
              <option value="gemini">Google Gemini</option>
            </select>
          </div>
          <div className="field">
            <label>Base URL</label>
            <input
              value={form.base_url}
              onChange={(e) => setForm({ ...form, base_url: e.target.value })}
            />
          </div>
          <div className="field">
            <label>模型</label>
            <input value={form.model} onChange={(e) => setForm({ ...form, model: e.target.value })} />
          </div>
        </div>
        <div className="field">
          <label>API Key</label>
          <input
            type="password"
            value={form.api_key}
            onChange={(e) => setForm({ ...form, api_key: e.target.value })}
          />
        </div>
        <label className="inline">
          <input
            type="checkbox"
            checked={form.is_default}
            onChange={(e) => setForm({ ...form, is_default: e.target.checked })}
          />
          设为默认
        </label>
        <div className="modal-actions">
          <button className="btn" onClick={props.onClose}>
            取消
          </button>
          <button className="btn primary" onClick={() => void save()}>
            保存
          </button>
        </div>
      </div>
    </div>
  );
}

/** 切换类型时的默认名称 */
const KIND_NAMES: Record<string, string> = {
  local: "本机发布",
  s3: "MinIO",
  r2: "Cloudflare R2",
  cloudflare_pages: "Cloudflare Pages",
  ngrok: "ngrok",
  ssh_tunnel: "SSH 反向隧道",
  bore: "bore",
  frp: "frp",
  cloudflare_tunnel: "Cloudflare Tunnel",
  lan_tunnel: "Quick Tunnel",
};

export function TargetDialog(props: {
  onClose: () => void;
  onSaved: () => void;
  onError: (message: string) => void;
  notify?: (message: string, error?: boolean) => void;
}) {
  // 默认本机发布：不需要任何配置，同一 Wi-Fi 下即可查看
  const [form, setForm] = useState<PublishTargetInput>({
    name: "本机发布",
    kind: "local",
    endpoint: "http://127.0.0.1:9000",
    region: "us-east-1",
    bucket: "",
    access_key: "",
    secret_key: "",
    prefix: "anniversary",
    public_base_url: "",
    cloudflared_path: "",
    tunnel_token: "",
    hostname: "",
    account_id: "",
    project_name: "",
    api_token: "",
    branch: "main",
    tunnel_config: "",
    is_default: true,
  });
  const [tunnel, setTunnel] = useState<TunnelConfig>(EMPTY_TUNNEL_CONFIG);
  const [cfConnected, setCfConnected] = useState(false);
  /** 已授权时默认用授权账户；用户可切到「手动配置」显示完整表单 */
  const [manual, setManual] = useState(false);

  /** R2 / Pages / 命名隧道：授权后只需填名称（Tunnel 还需要域名） */
  const cfKind = form.kind === "r2" || form.kind === "cloudflare_pages" || form.kind === "cloudflare_tunnel";
  const useAuth = cfKind && cfConnected && !manual;
  /** 未授权时只显示授权面板；完整表单在「手动配置」或授权失败后才出现 */
  const awaitingAuth = cfKind && !cfConnected && !manual;
  const showManual = !useAuth && !awaitingAuth;
  const isS3 = (form.kind === "s3" || form.kind === "r2") && showManual;
  const isPages = form.kind === "cloudflare_pages" && showManual;
  const isCloudflareTunnel = (form.kind === "lan_tunnel" || form.kind === "cloudflare_tunnel") && showManual;
  const resourceLabel = form.kind === "r2" ? "Bucket 名称" : form.kind === "cloudflare_pages" ? "Pages 项目名称" : "";
  const resourceValue = form.kind === "r2" ? form.bucket : form.project_name;

  const save = async () => {
    if (awaitingAuth) {
      props.onError("请先授权 Cloudflare，或选择「手动配置」");
      return;
    }
    if (useAuth) {
      if (form.kind === "cloudflare_tunnel" && !form.hostname.trim()) {
        props.onError("请填写要使用的域名，例如 love.example.com");
        return;
      }
      if (form.kind !== "cloudflare_tunnel" && !resourceValue.trim()) {
        props.onError(`请填写${resourceLabel}`);
        return;
      }
      try {
        await api.createPublishTarget({
          ...form,
          // R2 走 Cloudflare API 上传，不需要 endpoint / 密钥；Pages 使用授权令牌
          endpoint: form.kind === "r2" ? "" : form.endpoint,
          region: form.kind === "r2" ? "auto" : form.region,
          branch: "main",
          tunnel_config: JSON.stringify({ cf_auth: true }),
        });
        props.onSaved();
      } catch (error) {
        props.onError(`保存失败：${String(error)}`);
      }
      return;
    }
    if (isS3 && !form.bucket.trim()) {
      props.onError("请填写 bucket");
      return;
    }
    if (isPages && (!form.account_id.trim() || !form.project_name.trim() || !form.api_token.trim())) {
      props.onError("请填写 Account ID、Project Name 和 API Token");
      return;
    }
    if (form.kind === "cloudflare_tunnel" && (!form.tunnel_token.trim() || !form.hostname.trim())) {
      props.onError("请填写 Tunnel Token 和公开 hostname");
      return;
    }
    const tunnelError = validateTunnel(form.kind, tunnel);
    if (tunnelError) {
      props.onError(tunnelError);
      return;
    }
    try {
      await api.createPublishTarget({ ...form, tunnel_config: JSON.stringify(tunnel) });
      props.onSaved();
    } catch (error) {
      props.onError(`保存失败：${String(error)}`);
    }
  };

  return (
    <div className="modal-backdrop">
      <div className="modal">
        <h2>添加发布目标</h2>
        <div className="grid two" style={{ gridTemplateColumns: "1fr 1fr" }}>
          <div className="field">
            <label>名称</label>
            <input value={form.name} onChange={(e) => setForm({ ...form, name: e.target.value })} />
          </div>
          <div className="field">
            <label>类型</label>
            <select
              value={form.kind}
              onChange={(e) => {
                const kind = e.target.value;
                setForm({ ...form, kind, name: KIND_NAMES[kind] ?? form.name });
              }}
            >
              <optgroup label="本机">
                <option value="local">本机发布（同一 Wi-Fi 下查看）</option>
              </optgroup>
              <optgroup label="云端发布（长期有效，可分享）">
                <option value="s3">S3 / MinIO</option>
                <option value="r2">Cloudflare R2</option>
                <option value="cloudflare_pages">Cloudflare Pages</option>
              </optgroup>
              <optgroup label="内网穿透（电脑需保持在线）">
                <option value="ngrok">ngrok</option>
                <option value="ssh_tunnel">SSH 反向隧道</option>
                <option value="bore">bore</option>
                <option value="frp">frp</option>
                <option value="cloudflare_tunnel">Cloudflare Tunnel</option>
                <option value="lan_tunnel">Cloudflare Quick Tunnel（免账号）</option>
              </optgroup>
            </select>
          </div>
        </div>

        {cfKind && !manual && (
          <CloudflareConnect
            compact
            notify={props.notify ?? (() => undefined)}
            onChange={(status) => setCfConnected(status.connected)}
            onManual={() => setManual(true)}
          />
        )}

        {useAuth && (
          <>
            {form.kind === "cloudflare_tunnel" ? (
              <div className="field">
                <label>域名</label>
                <input
                  value={form.hostname}
                  placeholder="love.example.com（需在这个 Cloudflare 账户下）"
                  onChange={(e) => setForm({ ...form, hostname: e.target.value.trim() })}
                />
                <span className="muted">首次发布时自动完成所有设置，不需要手动操作。</span>
              </div>
            ) : (
              <div className="field">
                <label>{resourceLabel}</label>
                <input
                  value={resourceValue}
                  placeholder={form.kind === "r2" ? "例如 love-cards（不存在时自动创建）" : "例如 love-cards（不存在时自动创建，全局唯一）"}
                  onChange={(e) =>
                    setForm(form.kind === "r2" ? { ...form, bucket: e.target.value } : { ...form, project_name: e.target.value })
                  }
                />
                <span className="muted">
                  {form.kind === "r2"
                    ? "不存在时会自动创建并开启公开访问，发布后可以换成自己的域名。"
                    : "自动创建 Pages 项目，发布地址为 https://项目名.pages.dev。"}
                </span>
              </div>
            )}
            <button className="btn small subtle" onClick={() => setManual(true)}>
              手动配置（不使用授权）
            </button>
          </>
        )}
        {cfKind && manual && (
          <button className="btn small subtle" style={{ marginBottom: 12 }} onClick={() => setManual(false)}>
            ← 使用 Cloudflare 授权
          </button>
        )}

        {isS3 && (
          <>
            <div className="grid two" style={{ gridTemplateColumns: "1fr 1fr" }}>
              <div className="field">
                <label>Endpoint</label>
                <input
                  value={form.endpoint}
                  onChange={(e) => setForm({ ...form, endpoint: e.target.value })}
                />
              </div>
              <div className="field">
                <label>Region</label>
                <input
                  value={form.region}
                  onChange={(e) => setForm({ ...form, region: e.target.value })}
                />
              </div>
              <div className="field">
                <label>Bucket</label>
                <input value={form.bucket} onChange={(e) => setForm({ ...form, bucket: e.target.value })} />
              </div>
              <div className="field">
                <label>路径前缀</label>
                <input value={form.prefix} onChange={(e) => setForm({ ...form, prefix: e.target.value })} />
              </div>
            </div>
            <div className="grid two" style={{ gridTemplateColumns: "1fr 1fr" }}>
              <div className="field">
                <label>Access Key</label>
                <input
                  value={form.access_key}
                  onChange={(e) => setForm({ ...form, access_key: e.target.value })}
                />
              </div>
              <div className="field">
                <label>Secret Key</label>
                <input
                  type="password"
                  value={form.secret_key}
                  onChange={(e) => setForm({ ...form, secret_key: e.target.value })}
                />
              </div>
            </div>
            <div className="field">
              <label>公开访问 Base URL（可选）</label>
              <input
                value={form.public_base_url}
                placeholder="https://cdn.example.com"
                onChange={(e) => setForm({ ...form, public_base_url: e.target.value })}
              />
            </div>
          </>
        )}

        {isPages && (
          <>
            <div className="grid two" style={{ gridTemplateColumns: "1fr 1fr" }}>
              <div className="field">
                <label>Account ID</label>
                <input
                  value={form.account_id}
                  onChange={(e) => setForm({ ...form, account_id: e.target.value })}
                />
              </div>
              <div className="field">
                <label>Project Name</label>
                <input
                  value={form.project_name}
                  onChange={(e) => setForm({ ...form, project_name: e.target.value })}
                />
              </div>
            </div>
            <div className="field">
              <label>Cloudflare API Token（Pages: Edit）</label>
              <input
                type="password"
                value={form.api_token}
                onChange={(e) => setForm({ ...form, api_token: e.target.value })}
              />
            </div>
            <div className="grid two" style={{ gridTemplateColumns: "1fr 1fr" }}>
              <div className="field">
                <label>分支</label>
                <input value={form.branch} onChange={(e) => setForm({ ...form, branch: e.target.value })} />
              </div>
              <div className="field">
                <label>路径前缀</label>
                <input value={form.prefix} onChange={(e) => setForm({ ...form, prefix: e.target.value })} />
              </div>
            </div>
            <div className="field">
              <label>自定义域名（可选）</label>
              <input
                value={form.public_base_url}
                placeholder="https://anniversary.example.com"
                onChange={(e) => setForm({ ...form, public_base_url: e.target.value })}
              />
            </div>
            <div className="tip">
              直接从这里上传发布，不需要安装其他软件；内容没变的文件不会重复上传。
            </div>
          </>
        )}

        {form.kind === "local" && (
          <div className="tip" style={{ marginBottom: 12 }}>
            页面由这台电脑直接提供，不上传到任何地方。手机连同一个 Wi-Fi 后即可打开；
            电脑需保持开机，换了网络后地址也会变化。
          </div>
        )}

        <TunnelFields kind={form.kind} config={tunnel} onChange={setTunnel} />

        {isCloudflareTunnel && (
          <>
            <div className="field">
              <label>程序位置（可选）</label>
              <input
                value={form.cloudflared_path}
                placeholder="一般留空，需要时会自动下载"
                onChange={(e) => setForm({ ...form, cloudflared_path: e.target.value })}
              />
            </div>
            {form.kind === "cloudflare_tunnel" && (
              <>
                <div className="field">
                  <label>Tunnel Token</label>
                  <input
                    type="password"
                    value={form.tunnel_token}
                    onChange={(e) => setForm({ ...form, tunnel_token: e.target.value })}
                  />
                </div>
                <div className="field">
                  <label>公开 hostname</label>
                  <input
                    value={form.hostname}
                    placeholder="anniversary.example.com"
                    onChange={(e) => setForm({ ...form, hostname: e.target.value })}
                  />
                </div>
                <div className="tip">
                  在 Cloudflare Zero Trust 中，把这个域名的服务地址设为
                  <code> http://127.0.0.1:53410</code>。外网只能访问纪念日页面。
                </div>
              </>
            )}
            {form.kind === "lan_tunnel" && (
              <div className="tip">
                不需要账号，自动生成一个临时网址；
                外网只能访问纪念日页面，其他功能不会对外开放。
              </div>
            )}
          </>
        )}

        <div className="modal-actions">
          <button className="btn" onClick={props.onClose}>
            取消
          </button>
          <button className="btn primary" onClick={() => void save()}>
            保存
          </button>
        </div>
      </div>
    </div>
  );
}
