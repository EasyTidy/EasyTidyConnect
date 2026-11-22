import { useEffect, useRef, useState, type CSSProperties } from "react";
import { openUrl } from "@tauri-apps/plugin-opener";
import {
  api,
  type AiProvider,
  type AnniversaryProject,
  type ProjectInput,
  type ProjectVersion,
  type PublishResult,
  type PublishTarget,
  type CustomTemplate,
  type Status,
  type TemplateInfo,
  customTemplateId,
  isShareableKind,
} from "./api";
import AiTemplates from "./AiTemplates";
import ShareDialog from "./ShareDialog";
import NfcWriteDialog from "./NfcWriteDialog";
import { PREVIEW_SCROLL_MESSAGE, previewDocument } from "./preview";
import { COVER_RULES, CropEditor, PhotoGrid, fitsRule, usePhotoSizes } from "./PhotoManager";
import { copyText } from "./clipboard";
import { ProviderDialog, TargetDialog, readBase64 } from "./StudioDialogs";

type Tab = "content" | "style" | "media" | "publish";

const TABS: { id: Tab; label: string }[] = [
  { id: "content", label: "内容" },
  { id: "style", label: "样式" },
  { id: "media", label: "照片音乐" },
  { id: "publish", label: "发布" },
];

const TEMPLATE_HINTS: Record<string, string> = {
  minimal: "留白排版，衬线字体",
  polaroid: "拍立得照片墙",
  letter: "复古信纸，手写感",
  timeline: "按时间讲述故事",
  starry: "深色星空，适合夜晚",
  xitie: "红金喜帖，囍字封面",
  garden: "森系拱门，清新花草",
  editorial: "杂志封面，轻奢排版",
  party: "彩带门票，派对聚会",
  film: "胶片相册，在一起 N 天",
};

const THEME_SWATCHES = ["#2563eb", "#7c3aed", "#db2777", "#e11d48", "#f59e0b", "#10b981", "#0f766e", "#334155"];

const TARGET_KINDS: Record<string, string> = {
  local: "本机（局域网）",
  s3: "S3 / MinIO",
  r2: "Cloudflare R2",
  cloudflare_pages: "Cloudflare Pages",
  lan_tunnel: "Cloudflare Quick Tunnel",
  cloudflare_tunnel: "Cloudflare Tunnel",
  ngrok: "ngrok",
  frp: "frp",
  ssh_tunnel: "SSH 反向隧道",
  bore: "bore",
};

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
  music: "",
  video: "",
  custom_css: "",
  photo_focus: {},
  slideshow: false,
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
    music: project.music,
    video: project.video,
    custom_css: project.custom_css,
    photo_focus: project.photo_focus ?? {},
    slideshow: project.slideshow ?? false,
  };
}

function isDirty(draft: ProjectInput, base: ProjectInput) {
  return (Object.keys(base) as (keyof ProjectInput)[]).some((key) =>
    typeof base[key] === "object" ? JSON.stringify(draft[key]) !== JSON.stringify(base[key]) : draft[key] !== base[key],
  );
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
  const [tab, setTab] = useState<Tab>("content");
  const [busy, setBusy] = useState(false);
  const [aiOpen, setAiOpen] = useState(false);
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
  const [publishKind, setPublishKind] = useState("");
  const [shareUrl, setShareUrl] = useState<string | null>(null);
  const [nfcTarget, setNfcTarget] = useState<{ url: string; kind: string } | null>(null);
  const [showProviderForm, setShowProviderForm] = useState(false);
  const [showTargetForm, setShowTargetForm] = useState(false);
  const [previewHtml, setPreviewHtml] = useState("");
  /** 正在调整裁剪的照片 */
  const [cropFile, setCropFile] = useState<string | null>(null);
  /** 预览的滚动位置：重新渲染后恢复，避免每次改动都跳回顶部 */
  const previewScroll = useRef(0);
  useEffect(() => {
    const onMessage = (event: MessageEvent) => {
      if (event.data?.type === PREVIEW_SCROLL_MESSAGE) previewScroll.current = Number(event.data.y) || 0;
    };
    window.addEventListener("message", onMessage);
    return () => window.removeEventListener("message", onMessage);
  }, []);
  const [mediaBusy, setMediaBusy] = useState(false);
  const [customTemplates, setCustomTemplates] = useState<CustomTemplate[]>([]);
  const photoRef = useRef<HTMLInputElement | null>(null);
  const musicRef = useRef<HTMLInputElement | null>(null);
  const videoRef = useRef<HTMLInputElement | null>(null);

  const dirty = isDirty(draft, selected ? toInput(selected) : EMPTY);
  const set = (patch: Partial<ProjectInput>) => setDraft((current) => ({ ...current, ...patch }));

  const reloadProviders = async () => {
    const list = await api.aiProviders();
    setProviders(list);
    setAiProviderId((current) =>
      list.some((p) => p.id === current) ? current : list.find((p) => p.is_default)?.id ?? list[0]?.id ?? null,
    );
  };

  const reloadTargets = async () => {
    const list = await api.publishTargets();
    setTargets(list);
    setTargetId((current) =>
      list.some((p) => p.id === current) ? current : list.find((p) => p.is_default)?.id ?? list[0]?.id ?? null,
    );
  };

  useEffect(() => {
    Promise.all([api.projects(), api.templates(), api.customTemplates(), reloadProviders(), reloadTargets()])
      .then(([nextProjects, nextTemplates, nextCustom]) => {
        setProjects(nextProjects);
        setTemplates(nextTemplates);
        setCustomTemplates(nextCustom);
        if (nextProjects[0]) applyProject(nextProjects[0]);
      })
      .catch((error) => notify(`加载纪念日失败：${String(error)}`, true));
  }, []);

  /** 用当前编辑内容拼出一个项目，供预览和模板缩略图渲染。 */
  const previewProject = (templateId = draft.template_id): AnniversaryProject => ({
    id: selected?.id ?? 0,
    slug: selected?.slug ?? "preview",
    ...draft,
    template_id: templateId,
    photos: selected?.photos ?? [],
    created_at: selected?.created_at ?? "",
    updated_at: selected?.updated_at ?? "",
  });

  // AI 模板的 CSS 变了也要刷新预览（draft 本身没变）。
  const currentCustom = customTemplates.find((item) => customTemplateId(item.id) === draft.template_id);
  useEffect(() => {
    const timer = window.setTimeout(() => {
      api
        .renderPreview(previewProject())
        .then(setPreviewHtml)
        .catch(() => setPreviewHtml(""));
    }, 250);
    return () => window.clearTimeout(timer);
  }, [draft, selected, currentCustom?.updated_at]);

  const applyProject = (project: AnniversaryProject) => {
    setSelected(project);
    setDraft(toInput(project));
    setPublishResult(null);
    void api.projectVersions(project.id).then(setVersions).catch(() => setVersions([]));
  };

  const confirmDiscard = () => !dirty || window.confirm("当前修改还没有保存，确定放弃这些修改吗？");

  const selectProject = (project: AnniversaryProject) => {
    if (project.id === selected?.id || !confirmDiscard()) return;
    applyProject(project);
  };

  const createNew = () => {
    if (!confirmDiscard()) return;
    setSelected(null);
    setDraft(EMPTY);
    setPublishResult(null);
    setVersions([]);
    setTab("content");
  };

  /** 只更新项目里的照片/媒体字段，保留正在编辑、尚未保存的文字。 */
  const applyMediaUpdate = (updated: AnniversaryProject) => {
    setSelected(updated);
    setDraft((current) => ({ ...current, music: updated.music, video: updated.video }));
    setProjects((current) => current.map((item) => (item.id === updated.id ? updated : item)));
  };

  const save = async (quiet = false): Promise<AnniversaryProject | null> => {
    if (!draft.name.trim()) {
      setTab("content");
      notify("请先填写项目名称", true);
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
      setProjects((current) => [saved, ...current.filter((item) => item.id !== saved.id)]);
      if (!quiet) notify("纪念日已保存");
      return saved;
    } catch (error) {
      notify(`保存失败：${String(error)}`, true);
      return null;
    } finally {
      setBusy(false);
    }
  };

  // Ctrl+S / ⌘S 保存。用 ref 拿到最新的 save，避免监听器里读到旧的 draft。
  const saveRef = useRef(save);
  saveRef.current = save;
  useEffect(() => {
    const onKey = (event: KeyboardEvent) => {
      if ((event.ctrlKey || event.metaKey) && event.key.toLowerCase() === "s") {
        event.preventDefault();
        void saveRef.current();
      }
    };
    window.addEventListener("keydown", onKey);
    return () => window.removeEventListener("keydown", onKey);
  }, []);

  /** 上传照片/媒体前确保项目已存在；新项目会先自动保存。 */
  const ensureProject = async () => selected ?? (await save(true));

  const remove = async () => {
    if (!selected) return;
    if (!window.confirm(`删除“${selected.name}”？此操作无法撤销。`)) return;
    try {
      await api.deleteProject(selected.id);
      const rest = projects.filter((item) => item.id !== selected.id);
      setProjects(rest);
      if (rest[0]) {
        applyProject(rest[0]);
      } else {
        setSelected(null);
        setDraft(EMPTY);
        setVersions([]);
      }
      notify("项目已删除");
    } catch (error) {
      notify(`删除失败：${String(error)}`, true);
    }
  };

  const addPhotos = async (files: FileList | null) => {
    const list = Array.from(files ?? []).filter((file) => file.type.startsWith("image/"));
    if (photoRef.current) photoRef.current.value = "";
    if (list.length === 0) return;
    const project = await ensureProject();
    if (!project) return;
    setMediaBusy(true);
    try {
      let current = project;
      for (const file of list) {
        current = await api.addProjectPhoto(project.id, file.name, await readBase64(file));
      }
      applyMediaUpdate(current);
      notify(`已添加 ${list.length} 张照片`);
    } catch (error) {
      notify(`添加照片失败：${String(error)}`, true);
    } finally {
      setMediaBusy(false);
    }
  };

  const removePhoto = async (file: string) => {
    if (!selected) return;
    try {
      await api.removeProjectPhoto(selected.id, file);
      applyMediaUpdate({ ...selected, photos: selected.photos.filter((item) => item !== file) });
    } catch (error) {
      notify(`移除照片失败：${String(error)}`, true);
    }
  };

  const uploadMedia = async (kind: "music" | "video", file: File | undefined) => {
    if (musicRef.current) musicRef.current.value = "";
    if (videoRef.current) videoRef.current.value = "";
    if (!file) return;
    const project = await ensureProject();
    if (!project) return;
    setMediaBusy(true);
    try {
      const updated = await api.addProjectMedia(project.id, kind, file.name, await readBase64(file));
      applyMediaUpdate(updated);
      notify(kind === "music" ? "背景音乐已添加" : "视频已添加");
    } catch (error) {
      notify(`媒体上传失败：${String(error)}`, true);
    } finally {
      setMediaBusy(false);
    }
  };

  const removeMedia = async (kind: "music" | "video") => {
    if (!selected) return;
    try {
      await api.removeProjectMedia(selected.id, kind);
      const updated = await api.project(selected.id);
      if (updated) applyMediaUpdate(updated);
    } catch (error) {
      notify(`移除失败：${String(error)}`, true);
    }
  };

  const generate = async () => {
    if (!aiProviderId) {
      setShowProviderForm(true);
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
      notify("AI 文案已填入，可继续修改");
    } catch (error) {
      notify(`AI 生成失败：${String(error)}`, true);
    } finally {
      setAiBusy(false);
    }
  };

  const openPreview = async () => {
    const saved = dirty || !selected ? await save(true) : selected;
    if (!saved || !status) return;
    try {
      await openUrl(`http://127.0.0.1:${status.port}/a/${saved.slug}`);
    } catch (error) {
      notify(`打开浏览器失败：${String(error)}`, true);
    }
  };

  const publish = async () => {
    if (!targetId) {
      setShowTargetForm(true);
      return;
    }
    setPublishBusy(true);
    try {
      const saved = await save(true);
      if (!saved) return;
      const result = await api.publishProject(saved.id, targetId);
      setPublishResult(result);
      setPublishKind(targets.find((item) => item.id === targetId)?.kind ?? "");
      setVersions(await api.projectVersions(saved.id));
      notify("发布成功");
    } catch (error) {
      notify(`发布失败：${String(error)}`, true);
    } finally {
      setPublishBusy(false);
    }
  };

  /** 分享弹窗里修改卡片文案后重新发布：优先用当前选中的静态目标，否则用第一个静态目标。 */
  const republishForShare = async () => {
    if (!selected) return;
    const target =
      targets.find((item) => item.id === targetId && isShareableKind(item.kind)) ??
      targets.find((item) => isShareableKind(item.kind));
    if (!target) {
      notify("还没有可长期分享的发布目标，请先添加一个云端发布目标", true);
      return;
    }
    const result = await api.publishProject(selected.id, target.id);
    setPublishResult(result);
    setPublishKind(target.kind);
    setShareUrl(result.url);
    setVersions(await api.projectVersions(selected.id));
    notify("分享卡片已更新");
  };

  const shareHint = "只有发布到云端的链接可以长期分享；内网穿透链接需要电脑一直在线";

  const copyLink = async (url: string) => {
    const ok = await copyText(url);
    notify(ok ? "链接已复制" : "复制失败，请手动选中复制", !ok);
  };

  const photoUrl = (file: string) =>
    selected && status ? `http://127.0.0.1:${status.port}/a/${selected.slug}/photos/${file}` : "";
  const photoCount = selected?.photos.length ?? 0;
  const photoSizes = usePhotoSizes(selected?.photos ?? [], photoUrl);
  const coverRule = COVER_RULES[draft.template_id];
  const coverFile = selected?.photos[0];
  const coverMisfit = Boolean(coverRule && coverFile && !fitsRule(coverRule, photoSizes[coverFile]));
  const coverShapeName = coverRule?.shape === "square" ? "方形" : "竖版";
  /** 合适做封面的其他照片（用于「设为封面」建议） */
  const betterCover = coverRule ? selected?.photos.slice(1).find((file) => fitsRule(coverRule, photoSizes[file]) && photoSizes[file]) : undefined;

  const reorderPhotos = async (photos: string[]) => {
    if (!selected) return;
    try {
      applyMediaUpdate(await api.reorderProjectPhotos(selected.id, photos));
    } catch (error) {
      notify(`调整顺序失败：${String(error)}`, true);
    }
  };
  const setFocus = (file: string, focus: [number, number]) =>
    setDraft((current) => ({ ...current, photo_focus: { ...current.photo_focus, [file]: focus } }));
  const mediaCount = (draft.music ? 1 : 0) + (draft.video ? 1 : 0);

  return (
    <>
      <header className="page-head">
        <div>
          <h1>纪念日工作室</h1>
          <p>写一封碰一下就能打开的信：填内容 → 选模板 → 加照片音乐 → 发布。</p>
        </div>
        <button className="btn primary" onClick={createNew}>
          + 新建纪念日
        </button>
      </header>

      <div className="studio-projects">
        {projects.length === 0 && !selected && (
          <span className="muted">还没有纪念日项目，在下方填写后保存即可创建。</span>
        )}
        {!selected && projects.length > 0 && (
          <span className="studio-chip active">
            <strong>{draft.name || "未命名新项目"}</strong>
            <span>新建中</span>
          </span>
        )}
        {projects.map((project) => (
          <button
            key={project.id}
            className={`studio-chip ${selected?.id === project.id ? "active" : ""}`}
            onClick={() => selectProject(project)}
          >
            <strong>{selected?.id === project.id ? draft.name || "未命名" : project.name}</strong>
            <span>{project.event_date || "未设置日期"}</span>
          </button>
        ))}
      </div>

      <div className="studio">
        <section className="card studio-editor">
          <div className="studio-toolbar">
            <div className="tabs" role="tablist">
              {TABS.map((item) => (
                <button
                  key={item.id}
                  role="tab"
                  aria-selected={tab === item.id}
                  className={tab === item.id ? "active" : ""}
                  onClick={() => setTab(item.id)}
                >
                  {item.label}
                  {item.id === "media" && photoCount + mediaCount > 0 && (
                    <span className="tab-count">{photoCount + mediaCount}</span>
                  )}
                </button>
              ))}
            </div>
            <div className="studio-save">
              <span className={`save-state ${dirty ? "dirty" : ""}`}>
                {dirty ? "未保存" : selected ? "已保存" : ""}
              </span>
              <button className="btn primary" onClick={() => void save()} disabled={busy} title="Ctrl+S">
                {busy ? "保存中..." : "保存"}
              </button>
            </div>
          </div>

          <div className="studio-body">
            {tab === "content" && (
              <>
                <div className="form-grid">
                  <div className="field">
                    <label>项目名称 *</label>
                    <input
                      value={draft.name}
                      placeholder="例如：结婚三周年"
                      onChange={(event) => set({ name: event.target.value })}
                    />
                  </div>
                  <div className="field">
                    <label>纪念日期</label>
                    <input
                      type="date"
                      value={draft.event_date}
                      onChange={(event) => set({ event_date: event.target.value })}
                    />
                  </div>
                  <div className="field">
                    <label>标题</label>
                    <input
                      value={draft.title}
                      placeholder="写给 TA 的一句话"
                      onChange={(event) => set({ title: event.target.value })}
                    />
                  </div>
                  <div className="field">
                    <label>收件人</label>
                    <input
                      value={draft.recipient}
                      placeholder="TA 的名字"
                      onChange={(event) => set({ recipient: event.target.value })}
                    />
                  </div>
                </div>

                <div className={`ai-panel ${aiOpen ? "open" : ""}`}>
                  <button className="ai-head" onClick={() => setAiOpen((open) => !open)}>
                    <span className="ai-icon">✦</span>
                    <span className="grow">
                      <strong>AI 帮我写</strong>
                      <span>根据关系、语气和关键词生成标题、开场、正文与结尾</span>
                    </span>
                    <span className="ai-caret">{aiOpen ? "收起" : "展开"}</span>
                  </button>
                  {aiOpen && (
                    <div className="ai-body">
                      <div className="form-grid">
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
                        <div className="field">
                          <label>语气</label>
                          <select value={aiForm.tone} onChange={(event) => setAiForm({ ...aiForm, tone: event.target.value })}>
                            <option>温柔</option>
                            <option>幽默</option>
                            <option>正式</option>
                            <option>克制</option>
                          </select>
                        </div>
                        <div className="field">
                          <label>篇幅</label>
                          <select value={aiForm.length} onChange={(event) => setAiForm({ ...aiForm, length: event.target.value })}>
                            <option>简短</option>
                            <option>中等</option>
                            <option>详细</option>
                          </select>
                        </div>
                      </div>
                      <div className="inline wrap">
                        {providers.length > 0 ? (
                          <select
                            className="compact-select"
                            value={aiProviderId ?? ""}
                            onChange={(event) => setAiProviderId(Number(event.target.value))}
                          >
                            {providers.map((provider) => (
                              <option key={provider.id} value={provider.id}>
                                {provider.name} · {provider.model}
                              </option>
                            ))}
                          </select>
                        ) : (
                          <span className="muted">还没有 AI Provider</span>
                        )}
                        <button className="btn small" onClick={() => setShowProviderForm(true)}>
                          添加 Provider
                        </button>
                        <span className="grow" />
                        <button className="btn primary" onClick={() => void generate()} disabled={aiBusy}>
                          {aiBusy ? "生成中..." : providers.length ? "生成文案" : "先添加 Provider"}
                        </button>
                      </div>
                      <div className="muted" style={{ marginTop: 8 }}>
                        生成结果会覆盖标题、开场、正文和结尾，记得生成前保存想保留的内容。
                      </div>
                    </div>
                  )}
                </div>

                <div className="field">
                  <label>开场</label>
                  <textarea value={draft.opening} onChange={(event) => set({ opening: event.target.value })} />
                </div>
                <div className="field">
                  <label>
                    正文 <span className="label-hint">每行一段</span>
                  </label>
                  <textarea
                    className="tall"
                    value={draft.body}
                    onChange={(event) => set({ body: event.target.value })}
                  />
                </div>
                <div className="field">
                  <label>结尾</label>
                  <textarea value={draft.closing} onChange={(event) => set({ closing: event.target.value })} />
                </div>
              </>
            )}
            {tab === "style" && (
              <>
                <div className="section-label">预设模板</div>
                <div className="template-grid">
                  {templates.map((template) => (
                    <button
                      key={template.id}
                      className={`template-card ${draft.template_id === template.id ? "active" : ""}`}
                      onClick={() => set({ template_id: template.id })}
                    >
                      <span className={`template-thumb t-${template.id}`} style={{ "--accent": draft.theme } as CSSProperties} />
                      <strong>{template.name}</strong>
                      <span>{TEMPLATE_HINTS[template.id] ?? ""}</span>
                    </button>
                  ))}
                </div>

                <AiTemplates
                  templates={customTemplates}
                  currentId={draft.template_id}
                  providers={providers}
                  providerId={aiProviderId}
                  theme={draft.theme}
                  onProviderChange={setAiProviderId}
                  renderThumb={(templateId) => api.renderPreview(previewProject(templateId))}
                  onSelect={(templateId) => set({ template_id: templateId })}
                  onTemplatesChange={setCustomTemplates}
                  onAddProvider={() => setShowProviderForm(true)}
                  notify={notify}
                />

                {coverMisfit && (
                  <div className="cover-warning">
                    「{templates.find((t) => t.id === draft.template_id)?.name}」的{coverRule!.slot}适合{coverShapeName}照片，当前封面会被裁剪。
                    <button className="btn small" onClick={() => setTab("media")}>
                      去调整
                    </button>
                  </div>
                )}

                <div className="section-label" style={{ marginTop: 24 }}>主题色</div>
                <div className="swatches">
                  {THEME_SWATCHES.map((color) => (
                    <button
                      key={color}
                      className={`swatch ${draft.theme.toLowerCase() === color ? "active" : ""}`}
                      style={{ background: color }}
                      onClick={() => set({ theme: color })}
                      aria-label={`主题色 ${color}`}
                    />
                  ))}
                  <label className="swatch custom" title="自定义颜色">
                    <input type="color" value={draft.theme} onChange={(event) => set({ theme: event.target.value })} />
                  </label>
                  <span className="mono-inline">{draft.theme}</span>
                </div>

                <div className="field" style={{ marginTop: 22 }}>
                  <label>
                    自定义 CSS <span className="label-hint">可选，高级</span>
                  </label>
                  <textarea
                    className="code"
                    value={draft.custom_css}
                    placeholder="h1 { letter-spacing: .1em; }"
                    onChange={(event) => set({ custom_css: event.target.value })}
                  />
                </div>
              </>
            )}

            {tab === "media" && (
              <>
                <div className="section-label">
                  照片 <span className="label-hint">{photoCount} 张 · 单张不超过 12 MB</span>
                </div>
                <div className={`photo-rule ${coverMisfit ? "warn" : ""}`}>
                  {coverRule ? (
                    <>
                      当前模板的<b>{coverRule.slot}</b>使用第一张照片，适合<b>{coverShapeName}</b>
                      {coverRule.shape === "portrait" ? `（${coverRule.ratio === 0.75 ? "3:4" : "整屏"}）` : "（1:1）"}；其余照片按原比例完整显示。
                      {coverMisfit && (
                        <>
                          {" "}当前封面会被裁剪，可以
                          <button className="link-btn" onClick={() => coverFile && setCropFile(coverFile)}>
                            调整裁剪区域
                          </button>
                          {betterCover && (
                            <>
                              ，或
                              <button
                                className="link-btn"
                                onClick={() => void reorderPhotos([betterCover, ...(selected?.photos ?? []).filter((f) => f !== betterCover)])}
                              >
                                把一张{coverShapeName}照片设为封面
                              </button>
                            </>
                          )}
                          。
                        </>
                      )}
                    </>
                  ) : (
                    <>这个模板里所有照片都按原比例完整显示，横版、竖版都可以。</>
                  )}
                  <span className="label-hint">拖动照片调整顺序</span>
                </div>
                <PhotoGrid
                  photos={selected?.photos ?? []}
                  urlOf={photoUrl}
                  sizes={photoSizes}
                  rule={coverRule}
                  busy={mediaBusy}
                  editing={cropFile}
                  onReorder={(photos) => void reorderPhotos(photos)}
                  onRemove={(file) => void removePhoto(file)}
                  onAdjust={(file) => setCropFile(file)}
                  onAdd={() => photoRef.current?.click()}
                />
                {cropFile && coverRule && selected?.photos[0] === cropFile && (
                  <CropEditor
                    url={photoUrl(cropFile)}
                    size={photoSizes[cropFile]}
                    ratio={coverRule.ratio}
                    slot={coverRule.slot}
                    focus={draft.photo_focus[cropFile] ?? [50, 50]}
                    onChange={(focus) => setFocus(cropFile, focus)}
                    onClose={() => setCropFile(null)}
                  />
                )}
                <input
                  ref={photoRef}
                  type="file"
                  accept="image/*"
                  multiple
                  hidden
                  onChange={(event) => void addPhotos(event.target.files)}
                />

                <div className="section-label" style={{ marginTop: 24 }}>
                  音乐与视频 <span className="label-hint">单个文件不超过 40 MB</span>
                </div>
                <div className="list">
                  {(["music", "video"] as const).map((kind) => {
                    const value = draft[kind];
                    const isMusic = kind === "music";
                    return (
                      <div key={kind} className="row-card">
                        <span className="media-icon" aria-hidden="true">{isMusic ? "\uE8D6" : "\uE714"}</span>
                        <div className="grow">
                          <div className="row-title">{isMusic ? "背景音乐" : "视频"}</div>
                          <div className="row-sub">
                            {value || (isMusic ? "支持 mp3 / m4a / wav / ogg" : "支持 mp4 / webm / mov")}
                          </div>
                        </div>
                        <div className="row-actions">
                          <button
                            className="btn small"
                            disabled={mediaBusy}
                            onClick={() => (isMusic ? musicRef : videoRef).current?.click()}
                          >
                            {value ? "替换" : "上传"}
                          </button>
                          {value && (
                            <button className="btn small danger" onClick={() => void removeMedia(kind)}>
                              移除
                            </button>
                          )}
                        </div>
                      </div>
                    );
                  })}
                </div>
                <input
                  ref={musicRef}
                  type="file"
                  accept="audio/*"
                  hidden
                  onChange={(event) => void uploadMedia("music", event.target.files?.[0])}
                />
                <input
                  ref={videoRef}
                  type="file"
                  accept="video/*"
                  hidden
                  onChange={(event) => void uploadMedia("video", event.target.files?.[0])}
                />
                {!selected && <div className="tip" style={{ marginTop: 14 }}>新项目会在第一次添加文件时自动保存。</div>}
              </>
            )}
            {tab === "publish" && (
              <>
                <div className="section-label">发布目标</div>
                {targets.length === 0 ? (
                  <div className="empty">
                    还没有发布目标。
                    <br />
                    <button className="btn primary" style={{ marginTop: 12 }} onClick={() => setShowTargetForm(true)}>
                      添加发布目标
                    </button>
                  </div>
                ) : (
                  <div className="target-list">
                    {targets.map((target) => (
                      <label key={target.id} className={`target-option ${targetId === target.id ? "active" : ""}`}>
                        <input type="radio" checked={targetId === target.id} onChange={() => setTargetId(target.id)} />
                        <span className="grow">
                          <strong>{target.name}</strong>
                          <span>
                            {TARGET_KINDS[target.kind] ?? target.kind}
                            {target.bucket ? ` · ${target.bucket}` : ""}
                          </span>
                        </span>
                      </label>
                    ))}
                    <button className="btn small" style={{ alignSelf: "flex-start" }} onClick={() => setShowTargetForm(true)}>
                      + 添加发布目标
                    </button>
                  </div>
                )}

                <div className="section-label" style={{ marginTop: 20 }}>页面效果</div>
                <label className={`target-option ${draft.slideshow ? "active" : ""}`}>
                  <input
                    type="checkbox"
                    checked={draft.slideshow}
                    onChange={(event) => setDraft((current) => ({ ...current, slideshow: event.target.checked }))}
                  />
                  <span className="grow">
                    <strong>照片自动放大轮播</strong>
                    <span>打开网页滚动到照片时，自动全屏逐张播放，播完自动关闭；点照片可暂停。预览里不会自动弹出，点击照片即可看效果。</span>
                  </span>
                </label>

                <button
                  className="btn primary publish-btn"
                  onClick={() => void publish()}
                  disabled={publishBusy || targets.length === 0}
                >
                  {publishBusy ? "发布中..." : "保存并发布"}
                </button>

                {publishResult && (
                  <div className="publish-result">
                    <div className="pill ok">发布成功</div>
                    <a href={publishResult.url} onClick={(event) => { event.preventDefault(); void openUrl(publishResult.url); }}>
                      {publishResult.url}
                    </a>
                    <button className="btn small" onClick={() => void copyLink(publishResult.url)}>
                      复制链接
                    </button>
                    <button className="btn small" onClick={() => setNfcTarget({ url: publishResult.url, kind: publishKind })}>
                      写入 NFC
                    </button>
                    {isShareableKind(publishKind) ? (
                      <button className="btn small primary" onClick={() => setShareUrl(publishResult.url)}>
                        分享
                      </button>
                    ) : (
                      <span className="muted" title={shareHint}>
                        临时链接，不支持分享
                      </span>
                    )}
                  </div>
                )}

                {versions.length > 0 && (
                  <>
                    <div className="section-label" style={{ marginTop: 24 }}>发布记录</div>
                    <div className="version-list">
                      {versions.slice(0, 6).map((version) => (
                        <div key={version.id} className="version-row">
                          <span className="version-no">v{version.version}</span>
                          <span className="grow mono-inline">{version.published_url}</span>
                          <span className="muted">{new Date(version.published_at).toLocaleString()}</span>
                          <button className="btn small" onClick={() => void copyLink(version.published_url)}>
                            复制
                          </button>
                          <button
                            className="btn small"
                            title="把这个网页地址写入 NFC 标签"
                            onClick={() => setNfcTarget({ url: version.published_url, kind: version.target_kind })}
                          >
                            NFC
                          </button>
                          <button
                            className="btn small"
                            disabled={!isShareableKind(version.target_kind)}
                            title={isShareableKind(version.target_kind) ? "分享到微信、微博等" : shareHint}
                            onClick={() => setShareUrl(version.published_url)}
                          >
                            分享
                          </button>
                        </div>
                      ))}
                    </div>
                  </>
                )}

                <div className="tip" style={{ marginTop: 20 }}>
                  想让 TA 碰一下 NFC 就打开？发布后点「写入 NFC」，把网页地址直接写进标签。发布到云端
                  的地址在哪里都能打开，本机发布的地址需要手机和电脑连同一个 Wi-Fi。
                </div>

                {selected && (
                  <div className="danger-zone">
                    <div>
                      <strong>删除项目</strong>
                      <span>会删除项目内容和发布记录，已发布的网页不受影响。</span>
                    </div>
                    <button className="btn small danger" onClick={() => void remove()}>
                      删除项目
                    </button>
                  </div>
                )}
              </>
            )}
          </div>
        </section>

        <aside className="card studio-preview">
          <div className="inline" style={{ justifyContent: "space-between", marginBottom: 14 }}>
            <div>
              <h2 style={{ margin: 0 }}>实时预览</h2>
              <div className="muted">手机上碰标签后看到的样子</div>
            </div>
            <button className="btn small" onClick={() => void openPreview()} disabled={!status}>
              在浏览器打开
            </button>
          </div>
          <div className="phone">
            <div className="phone-notch" />
            {previewHtml ? (
              <iframe key={draft.template_id} srcDoc={previewDocument(previewHtml, {
                  scrollY: previewScroll.current,
                  focusSelector: cropFile && coverRule ? coverRule.selector : undefined,
                })} sandbox="allow-scripts" title="纪念日预览" />
            ) : (
              <div className="phone-empty">正在生成预览...</div>
            )}
          </div>
        </aside>
      </div>

      {showProviderForm && (
        <ProviderDialog
          onClose={() => setShowProviderForm(false)}
          onSaved={async () => {
            setShowProviderForm(false);
            await reloadProviders();
            setAiOpen(true);
            notify("AI Provider 已保存");
          }}
          onError={(message) => notify(message, true)}
        />
      )}
      {nfcTarget && selected && (
        <NfcWriteDialog
          url={nfcTarget.url}
          name={selected.name}
          targetKind={nfcTarget.kind}
          onClose={() => setNfcTarget(null)}
          notify={notify}
        />
      )}
      {shareUrl && selected && (
        <ShareDialog
          project={selected}
          url={shareUrl}
          localThumb={selected.photos[0] ? photoUrl(selected.photos[0]) : "/app-icon.svg"}
          onClose={() => setShareUrl(null)}
          onRepublish={republishForShare}
          notify={notify}
        />
      )}
      {showTargetForm && (
        <TargetDialog
          notify={notify}
          onClose={() => setShowTargetForm(false)}
          onSaved={async () => {
            setShowTargetForm(false);
            await reloadTargets();
            notify("发布目标已保存");
          }}
          onError={(message) => notify(message, true)}
        />
      )}
    </>
  );
}
