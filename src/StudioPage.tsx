import { useEffect, useMemo, useRef, useState } from "react";
import {
  api,
  type AiProvider,
  type AiProviderInput,
  type AnniversaryProject,
  type ProjectInput,
  type ProjectVersion,
  type PublishResult,
  type PublishTarget,
  type PublishTargetInput,
  type Status,
  type TemplateInfo,
} from "./api";

const EMPTY: ProjectInput = {
  name: "",
  event_date: "",
  title: "",
  recipient: "",
  opening: "",
  body: "",
  closing: "",
  template_id: "minimal",
  theme: "#2563eb",
  custom_css: "",
};

function toInput(project: AnniversaryProject): ProjectInput {
  return {
    name: project.name,
    event_date: project.event_date,
    title: project.title,
    recipient: project.recipient,
    opening: project.opening,
    body: project.body,
    closing: project.closing,
    template_id: project.template_id,
    theme: project.theme,
    custom_css: project.custom_css,
  };
}

export default function StudioPage({
  status,
  notify,
}: {
  status: Status | null;
  notify: (message: string, error?: boolean) => void;
}) {
  const [projects, setProjects] = useState<AnniversaryProject[]>([]);
  const [templates, setTemplates] = useState<TemplateInfo[]>([]);
  const [providers, setProviders] = useState<AiProvider[]>([]);
  const [targets, setTargets] = useState<PublishTarget[]>([]);
  const [selected, setSelected] = useState<AnniversaryProject | null>(null);
  const [draft, setDraft] = useState<ProjectInput>(EMPTY);
  const [versions, setVersions] = useState<ProjectVersion[]>([]);
  const [busy, setBusy] = useState(false);
  const [aiProviderId, setAiProviderId] = useState<number | null>(null);
  const [aiBusy, setAiBusy] = useState(false);
  const [aiForm, setAiForm] = useState({
    relationship: "伴侣",
    tone: "温柔",
    keywords: "",
    language: "中文",
    length: "中等",
  });
  const [targetId, setTargetId] = useState<number | null>(null);
  const [publishBusy, setPublishBusy] = useState(false);
  const [publishResult, setPublishResult] = useState<PublishResult | null>(null);
  const [showProviderForm, setShowProviderForm] = useState(false);
  const [showTargetForm, setShowTargetForm] = useState(false);
  const fileRef = useRef<HTMLInputElement | null>(null);

  const load = async () => {
    try {
      const [nextProjects, nextTemplates, nextProviders, nextTargets] = await Promise.all([
        api.projects(),
        api.templates(),
        api.aiProviders(),
        api.publishTargets(),
      ]);
      setProjects(nextProjects);
      setTemplates(nextTemplates);
      setProviders(nextProviders);
      setTargets(nextTargets);
      setAiProviderId((current) => current ?? nextProviders.find((p) => p.is_default)?.id ?? nextProviders[0]?.id ?? null);
      setTargetId((current) => current ?? nextTargets.find((p) => p.is_default)?.id ?? nextTargets[0]?.id ?? null);
      if (nextProjects[0]) {
        selectProject(nextProjects[0]);
      }
    } catch (error) {
      notify(`加载纪念日失败：${String(error)}`, true);
    }
  };

  useEffect(() => {
    void load();
  }, []);

  const selectProject = (project: AnniversaryProject) => {
    setSelected(project);
    setDraft(toInput(project));
    setPublishResult(null);
    void api.projectVersions(project.id).then(setVersions).catch(() => setVersions([]));
  };

  const save = async (): Promise<AnniversaryProject | null> => {
    if (!draft.name.trim()) {
      notify("请填写项目名称", true);
      return null;
    }
    setBusy(true);
    try {
      let saved: AnniversaryProject;
      if (selected) {
        await api.updateProject(selected.id, draft);
        const updated = await api.project(selected.id);
        if (!updated) throw new Error("项目不存在");
        saved = updated;
      } else {
        saved = await api.createProject(draft);
      }
      setSelected(saved);
      setDraft(toInput(saved));
      setProjects((current) => {
        const rest = current.filter((item) => item.id !== saved.id);
        return [saved, ...rest];
      });
      notify("纪念日已保存");
      return saved;
    } catch (error) {
      notify(`保存失败：${String(error)}`, true);
      return null;
    } finally {
      setBusy(false);
    }
  };

  const createNew = () => {
    setSelected(null);
    setDraft(EMPTY);
    setPublishResult(null);
    setVersions([]);
  };

  const remove = async () => {
    if (!selected) return;
    if (!window.confirm(`删除“${selected.name}”？`)) return;
    await api.deleteProject(selected.id);
    setProjects((current) => current.filter((item) => item.id !== selected.id));
    createNew();
    notify("项目已删除");
  };

  const addPhotos = async (files: FileList | null) => {
    if (!files || !selected) return;
    setBusy(true);
    try {
      let current: AnniversaryProject | null = selected;
      for (const file of Array.from(files)) {
        if (!file.type.startsWith("image/")) continue;
        const base64 = await readBase64(file);
        current = await api.addProjectPhoto(selected.id, file.name, base64);
      }
      if (current) {
        setSelected(current);
        setDraft(toInput(current));
      }
      notify("照片已添加");
    } catch (error) {
      notify(`添加照片失败：${String(error)}`, true);
    } finally {
      setBusy(false);
      if (fileRef.current) fileRef.current.value = "";
    }
  };

  const removePhoto = async (file: string) => {
    if (!selected) return;
    const updated = { ...selected, photos: selected.photos.filter((item) => item !== file) };
    await api.removeProjectPhoto(selected.id, file);
    setSelected(updated);
    setDraft(toInput(updated));
  };

  const generate = async () => {
    if (!aiProviderId) {
      notify("请先添加 AI Provider", true);
      return;
    }
    setAiBusy(true);
    try {
      const copy = await api.generateCopy(aiProviderId, aiForm);
      setDraft((current) => ({
        ...current,
        title: copy.title || current.title,
        opening: copy.opening || current.opening,
        body: copy.body || current.body,
        closing: copy.closing || current.closing,
      }));
      notify("AI 文案已生成，可继续编辑");
    } catch (error) {
      notify(`AI 生成失败：${String(error)}`, true);
    } finally {
      setAiBusy(false);
    }
  };

  const publish = async () => {
    if (!selected) {
      notify("请先保存项目", true);
      return;
    }
    if (!targetId) {
      notify("请先添加发布目标", true);
      return;
    }
    setPublishBusy(true);
    try {
      const saved = await save();
      if (!saved) return;
      const result = await api.publishProject(saved.id, targetId);
      setPublishResult(result);
      setVersions(await api.projectVersions(saved.id));
      notify(`发布成功：${result.url}`);
    } catch (error) {
      notify(`发布失败：${String(error)}`, true);
    } finally {
      setPublishBusy(false);
    }
  };

  const previewUrl = useMemo(() => {
    if (!selected || !status) return "";
    return `http://127.0.0.1:${status.port}/a/${selected.slug}?v=${selected.updated_at}`;
  }, [selected, status]);

  return (
    <>
      <header className="page-head">
        <div>
          <h1>纪念日工作室</h1>
          <p>写一封可以碰一下打开的信。模板、AI 文案、S3/R2 发布。</p>
        </div>
        <button className="btn primary" onClick={createNew}>
          + 新建纪念日
        </button>
      </header>

      <div className="studio-layout">
        <aside className="card studio-list">
          <div className="inline" style={{ justifyContent: "space-between", marginBottom: 12 }}>
            <h2 style={{ margin: 0 }}>项目</h2>
            <button className="btn small" onClick={() => void load()}>
              刷新
            </button>
          </div>
          {projects.length === 0 && <div className="empty">还没有纪念日项目。</div>}
          {projects.map((project) => (
            <button
              key={project.id}
              className={`studio-item ${selected?.id === project.id ? "active" : ""}`}
              onClick={() => selectProject(project)}
            >
              <strong>{project.name}</strong>
              <span>{project.event_date || "未设置日期"}</span>
            </button>
          ))}
        </aside>

        <section className="card studio-editor">
          <div className="grid two" style={{ gridTemplateColumns: "1fr 1fr" }}>
            <div className="field">
              <label>项目名称</label>
              <input
                value={draft.name}
                placeholder="例如：结婚纪念日"
                onChange={(event) => setDraft({ ...draft, name: event.target.value })}
              />
            </div>
            <div className="field">
              <label>日期</label>
              <input
                type="date"
                value={draft.event_date}
                onChange={(event) => setDraft({ ...draft, event_date: event.target.value })}
              />
            </div>
            <div className="field">
              <label>标题</label>
              <input
                value={draft.title}
                placeholder="写给她/他的一句话"
                onChange={(event) => setDraft({ ...draft, title: event.target.value })}
              />
            </div>
            <div className="field">
              <label>收件人</label>
              <input
                value={draft.recipient}
                placeholder="名字"
                onChange={(event) => setDraft({ ...draft, recipient: event.target.value })}
              />
            </div>
          </div>

          <div className="grid two" style={{ gridTemplateColumns: "1fr 1fr" }}>
            <div className="field">
              <label>模板</label>
              <select
                value={draft.template_id}
                onChange={(event) => setDraft({ ...draft, template_id: event.target.value })}
              >
                {templates.map((template) => (
                  <option key={template.id} value={template.id}>
                    {template.name}
                  </option>
                ))}
              </select>
            </div>
            <div className="field">
              <label>主题色</label>
              <input
                type="color"
                value={draft.theme}
                onChange={(event) => setDraft({ ...draft, theme: event.target.value })}
              />
            </div>
          </div>

          <div className="field">
            <label>开场</label>
            <textarea
              value={draft.opening}
              onChange={(event) => setDraft({ ...draft, opening: event.target.value })}
            />
          </div>
          <div className="field">
            <label>正文</label>
            <textarea
              style={{ minHeight: 150 }}
              value={draft.body}
              onChange={(event) => setDraft({ ...draft, body: event.target.value })}
            />
          </div>
          <div className="field">
            <label>结尾</label>
            <textarea
              value={draft.closing}
              onChange={(event) => setDraft({ ...draft, closing: event.target.value })}
            />
          </div>

          <div className="field">
            <label>照片</label>
            <div className="inline">
              <input
                ref={fileRef}
                type="file"
                accept="image/*"
                multiple
                disabled={!selected || busy}
                onChange={(event) => void addPhotos(event.target.files)}
              />
              {!selected && <span className="muted">保存项目后可以添加照片</span>}
            </div>
            {selected && selected.photos.length > 0 && (
              <div className="studio-photos">
                {selected.photos.map((photo) => (
                  <div key={photo} className="studio-photo">
                    <img src={previewUrl.replace(/\/a\/.*/, `/a/${selected.slug}/photos/${photo}`)} alt="" />
                    <button className="btn small danger" onClick={() => void removePhoto(photo)}>
                      移除
                    </button>
                  </div>
                ))}
              </div>
            )}
          </div>

          <div className="field">
            <label>自定义 CSS（可选）</label>
            <textarea
              value={draft.custom_css}
              onChange={(event) => setDraft({ ...draft, custom_css: event.target.value })}
            />
          </div>

          <div className="inline" style={{ flexWrap: "wrap" }}>
            <button className="btn primary" onClick={() => void save()} disabled={busy}>
              {busy ? "保存中..." : "保存"}
            </button>
            {selected && (
              <>
                <button className="btn" onClick={() => setShowProviderForm(true)}>
                  添加 AI Provider
                </button>
                <button className="btn" onClick={() => setShowTargetForm(true)}>
                  添加发布目标
                </button>
                <button className="btn danger" onClick={() => void remove()}>
                  删除项目
                </button>
              </>
            )}
          </div>
        </section>

        <aside className="card studio-preview">
          <div className="inline" style={{ justifyContent: "space-between" }}>
            <h2 style={{ margin: 0 }}>预览</h2>
            {previewUrl && (
              <a href={previewUrl} target="_blank" rel="noreferrer" className="btn small">
                新窗口
              </a>
            )}
          </div>
          {previewUrl ? (
            <iframe key={previewUrl} src={previewUrl} title="纪念日预览" />
          ) : (
            <div className="empty">保存后显示预览</div>
          )}
        </aside>
      </div>

      <div className="grid two" style={{ marginTop: 16 }}>
        <section className="card">
          <h2>AI 文案</h2>
          <div className="grid two" style={{ gridTemplateColumns: "1fr 1fr" }}>
            <div className="field">
              <label>Provider</label>
              <select
                value={aiProviderId ?? ""}
                onChange={(event) => setAiProviderId(Number(event.target.value))}
              >
                <option value="" disabled>
                  {providers.length === 0 ? "先添加 Provider" : "请选择"}
                </option>
                {providers.map((provider) => (
                  <option key={provider.id} value={provider.id}>
                    {provider.name} · {provider.model}
                  </option>
                ))}
              </select>
            </div>
            <div className="field">
              <label>语气</label>
              <select
                value={aiForm.tone}
                onChange={(event) => setAiForm({ ...aiForm, tone: event.target.value })}
              >
                <option>温柔</option>
                <option>幽默</option>
                <option>正式</option>
                <option>克制</option>
              </select>
            </div>
            <div className="field">
              <label>关系</label>
              <input
                value={aiForm.relationship}
                onChange={(event) => setAiForm({ ...aiForm, relationship: event.target.value })}
              />
            </div>
            <div className="field">
              <label>关键词</label>
              <input
                value={aiForm.keywords}
                placeholder="旅行、猫、第一次见面"
                onChange={(event) => setAiForm({ ...aiForm, keywords: event.target.value })}
              />
            </div>
          </div>
          <div className="inline">
            <button className="btn primary" onClick={() => void generate()} disabled={aiBusy}>
              {aiBusy ? "生成中..." : "生成文案"}
            </button>
            <button className="btn" onClick={() => setShowProviderForm(true)}>
              管理 Provider
            </button>
          </div>
        </section>

        <section className="card">
          <h2>发布</h2>
          <div className="field">
            <label>发布目标</label>
            <select
              value={targetId ?? ""}
              onChange={(event) => setTargetId(Number(event.target.value))}
            >
              <option value="" disabled>
                {targets.length === 0 ? "先添加 S3/R2 目标" : "请选择"}
              </option>
              {targets.map((target) => (
                <option key={target.id} value={target.id}>
                  {target.name} · {target.bucket || target.kind}
                </option>
              ))}
            </select>
          </div>
          <div className="inline">
            <button className="btn primary" onClick={() => void publish()} disabled={publishBusy}>
              {publishBusy ? "发布中..." : "保存并发布"}
            </button>
            <button className="btn" onClick={() => setShowTargetForm(true)}>
              管理发布目标
            </button>
          </div>
          {publishResult && (
            <div className="tip" style={{ marginTop: 12 }}>
              已发布：{" "}
              <a href={publishResult.url} target="_blank" rel="noreferrer">
                {publishResult.url}
              </a>
            </div>
          )}
          {versions.length > 0 && (
            <div className="list" style={{ marginTop: 12 }}>
              {versions.slice(0, 4).map((version) => (
                <div key={version.id} className="row-sub">
                  v{version.version} · {version.published_url}
                </div>
              ))}
            </div>
          )}
        </section>
      </div>

      {showProviderForm && (
        <ProviderDialog
          onClose={() => setShowProviderForm(false)}
          onSaved={async () => {
            setShowProviderForm(false);
            await load();
            notify("AI Provider 已保存");
          }}
          onError={(message) => notify(message, true)}
        />
      )}
      {showTargetForm && (
        <TargetDialog
          onClose={() => setShowTargetForm(false)}
          onSaved={async () => {
            setShowTargetForm(false);
            await load();
            notify("发布目标已保存");
          }}
          onError={(message) => notify(message, true)}
        />
      )}
    </>
  );
}

async function readBase64(file: File): Promise<string> {
  return new Promise((resolve, reject) => {
    const reader = new FileReader();
    reader.onload = () => {
      const result = String(reader.result ?? "");
      resolve(result.includes(",") ? result.split(",")[1] : result);
    };
    reader.onerror = () => reject(reader.error ?? new Error("读取图片失败"));
    reader.readAsDataURL(file);
  });
}

function ProviderDialog(props: {
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

function TargetDialog(props: {
  onClose: () => void;
  onSaved: () => void;
  onError: (message: string) => void;
}) {
  const [form, setForm] = useState<PublishTargetInput>({
    name: "MinIO",
    kind: "s3",
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
    is_default: true,
  });

  const isS3 = form.kind === "s3" || form.kind === "r2";
  const isPages = form.kind === "cloudflare_pages";
  const isTunnel = form.kind === "lan_tunnel" || form.kind === "cloudflare_tunnel";

  const save = async () => {
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
    try {
      await api.createPublishTarget(form);
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
            <select value={form.kind} onChange={(e) => setForm({ ...form, kind: e.target.value })}>
              <option value="s3">S3 / MinIO</option>
              <option value="r2">Cloudflare R2</option>
              <option value="cloudflare_pages">Cloudflare Pages</option>
              <option value="lan_tunnel">局域网穿透（Quick Tunnel）</option>
              <option value="cloudflare_tunnel">Cloudflare Tunnel</option>
            </select>
          </div>
        </div>

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
              使用 Direct Upload：BLAKE3 哈希、资源去重、创建 deployment；不需要安装 wrangler。
            </div>
          </>
        )}

        {isTunnel && (
          <>
            <div className="field">
              <label>cloudflared 路径（可选，默认 PATH）</label>
              <input
                value={form.cloudflared_path}
                placeholder="C:\\Program Files\\cloudflared\\cloudflared.exe"
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
                  在 Cloudflare Zero Trust 中把该 hostname 的 ingress 指向
                  <code> http://127.0.0.1:53410</code>。Connect 只在该端口暴露 /a/*。
                </div>
              </>
            )}
            {form.kind === "lan_tunnel" && (
              <div className="tip">
                使用 cloudflared Quick Tunnel，随机生成 trycloudflare.com 地址；
                公开端口只暴露纪念日页面，不暴露 NFC 触发和写入接口。
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
