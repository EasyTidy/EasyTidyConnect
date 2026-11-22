import { useEffect, useState } from "react";
import { api, customTemplateId, type AiProvider, type CustomTemplate } from "./api";
import { previewDocument } from "./preview";

/** 预设的 AI 模板提示词，按场景分组；点击后填入输入框，可再修改。 */
const PROMPT_PRESETS: { group: string; name: string; prompt: string }[] = [
  {
    group: "婚礼",
    name: "中式喜帖",
    prompt:
      "中式婚礼请柬：正红色背景配烫金细线双边框，标题用大号书法感衬线字并加宽字距，四角用 SVG 画祥云纹，信纸区域为米白宣纸质感并带一枚红色圆形印章，照片加金色描边，整体喜庆、精致、不俗气。",
  },
  {
    group: "婚礼",
    name: "法式浪漫",
    prompt:
      "法式浪漫婚礼：奶油白与香槟金配色，大量留白，标题用优雅的细衬线字体，装饰为手绘感细线玫瑰与藤蔓（SVG 线条），照片做成拱门造型，段落轻柔地淡入，整体安静高级。",
  },
  {
    group: "派对",
    name: "生日派对",
    prompt:
      "生日派对邀请：明快的糖果色（粉、柠檬黄、薄荷绿），标题加粗并略微倾斜、带荧光笔底色，背景有彩色纸屑缓缓飘落的动画，信息卡片用粗黑描边加偏移投影的新粗野主义风格，照片像贴纸一样随机旋转。",
  },
  {
    group: "派对",
    name: "霓虹夜光",
    prompt:
      "霓虹夜光派对：深紫近黑的背景，标题有霓虹灯管发光效果和轻微闪烁动画，卡片为半透明玻璃，强调色用荧光粉与电光蓝，照片加霓虹描边，分隔线像发光的灯带。",
  },
  {
    group: "纪念日",
    name: "恋爱纪念",
    prompt:
      "恋爱纪念日：温柔的日落渐变背景（蜜桃粉到暖橙），标题有手写感，正文像一封情书放在带纸纹的信纸卡片上，照片做成拍立得并配小木夹，页面里有小爱心缓慢上升的动画。",
  },
  {
    group: "纪念日",
    name: "结婚周年",
    prompt:
      "结婚周年纪念：沉稳的墨绿配香槟金，像高级餐厅菜单那样居中排版，标题用衬线大字，分隔线是细金线加小菱形，照片为细金框的方形网格，整体克制、有仪式感。",
  },
  {
    group: "告白",
    name: "星夜告白",
    prompt:
      "告白信：深夜蓝背景配细小星点和偶尔划过的流星动画，标题柔光白，正文逐段淡入，照片带柔和的光晕边框，结尾那句话放大并轻轻发光。",
  },
  {
    group: "家庭",
    name: "家庭相册",
    prompt:
      "家庭纪念相册：温暖的米色牛皮纸质感，像一本翻开的老相册，照片带白边和四个相角贴，标题有手写感，文字为暖棕色，适合送给父母的结婚纪念日或生日。",
  },
];

export default function AiTemplates(props: {
  templates: CustomTemplate[];
  currentId: string;
  providers: AiProvider[];
  providerId: number | null;
  theme: string;
  onProviderChange: (id: number) => void;
  renderThumb: (templateId: string) => Promise<string>;
  onSelect: (templateId: string) => void;
  onTemplatesChange: (templates: CustomTemplate[]) => void;
  onAddProvider: () => void;
  notify: (message: string, error?: boolean) => void;
}) {
  const { templates, currentId, providers, providerId } = props;
  const current = templates.find((item) => customTemplateId(item.id) === currentId) ?? null;
  const [designing, setDesigning] = useState(false);
  const [request, setRequest] = useState("");
  const [refine, setRefine] = useState("");
  const [busy, setBusy] = useState<"create" | "refine" | null>(null);
  const [thumbs, setThumbs] = useState<Record<number, string>>({});
  const [renaming, setRenaming] = useState<string | null>(null);

  // 缩略图：用当前项目内容渲染每个 AI 模板。
  const thumbKey = templates.map((item) => `${item.id}:${item.updated_at}`).join(",") + props.theme;
  useEffect(() => {
    let cancelled = false;
    void Promise.all(
      templates.map(async (item) => [item.id, await props.renderThumb(customTemplateId(item.id)).catch(() => "")] as const),
    ).then((entries) => {
      if (!cancelled) setThumbs(Object.fromEntries(entries));
    });
    return () => {
      cancelled = true;
    };
  }, [thumbKey]);

  const ensureProvider = () => {
    if (providerId) return providerId;
    props.onAddProvider();
    return null;
  };

  const create = async () => {
    const id = ensureProvider();
    if (!id) return;
    if (!request.trim()) {
      props.notify("先描述一下想要的风格", true);
      return;
    }
    setBusy("create");
    try {
      const template = await api.generateCustomTemplate(id, request.trim(), props.theme);
      props.onTemplatesChange([template, ...templates]);
      props.onSelect(customTemplateId(template.id));
      setDesigning(false);
      setRequest("");
      props.notify(`「${template.name}」已生成并应用`);
    } catch (error) {
      props.notify(`AI 设计失败：${String(error)}`, true);
    } finally {
      setBusy(null);
    }
  };

  const adjust = async () => {
    const id = ensureProvider();
    if (!id || !current) return;
    if (!refine.trim()) {
      props.notify("写下想怎么调整，例如“标题再大一些”", true);
      return;
    }
    setBusy("refine");
    try {
      const updated = await api.generateCustomTemplate(id, refine.trim(), props.theme, current.id);
      props.onTemplatesChange(templates.map((item) => (item.id === updated.id ? updated : item)));
      setRefine("");
      props.notify("模板已按要求调整");
    } catch (error) {
      props.notify(`调整失败：${String(error)}`, true);
    } finally {
      setBusy(null);
    }
  };

  const rename = async () => {
    if (!current || renaming == null) return;
    try {
      const updated = await api.renameCustomTemplate(current.id, renaming);
      props.onTemplatesChange(templates.map((item) => (item.id === updated.id ? updated : item)));
      setRenaming(null);
    } catch (error) {
      props.notify(String(error), true);
    }
  };

  const remove = async () => {
    if (!current) return;
    if (!window.confirm(`删除 AI 模板「${current.name}」？使用它的纪念日会改用「极简」模板。`)) return;
    try {
      await api.deleteCustomTemplate(current.id);
      props.onTemplatesChange(templates.filter((item) => item.id !== current.id));
      props.onSelect("minimal");
      props.notify("模板已删除");
    } catch (error) {
      props.notify(`删除失败：${String(error)}`, true);
    }
  };

  const providerPicker =
    providers.length > 0 ? (
      <select
        className="compact-select"
        value={providerId ?? ""}
        onChange={(event) => props.onProviderChange(Number(event.target.value))}
        aria-label="AI Provider"
      >
        {providers.map((provider) => (
          <option key={provider.id} value={provider.id}>
            {provider.name} · {provider.model}
          </option>
        ))}
      </select>
    ) : (
      <button className="btn small" onClick={props.onAddProvider}>
        添加 AI Provider
      </button>
    );

  return (
    <>
      <div className="section-label">
        我的 AI 模板 <span className="label-hint">描述想要的样子，AI 为你设计整页样式</span>
      </div>
      <div className="template-grid">
        {templates.map((item) => {
          const id = customTemplateId(item.id);
          return (
            <button
              key={item.id}
              className={`template-card ${currentId === id ? "active" : ""}`}
              onClick={() => {
                setDesigning(false);
                props.onSelect(id);
              }}
              title={item.prompt}
            >
              <span className="template-thumb live">
                {thumbs[item.id] ? (
                  <iframe srcDoc={previewDocument(thumbs[item.id])} sandbox="" tabIndex={-1} aria-hidden="true" title="" />
                ) : null}
                <span className="ai-badge">AI</span>
              </span>
              <strong>{item.name}</strong>
              <span>{item.dark ? "深色" : "浅色"} · AI 设计</span>
            </button>
          );
        })}
        <button
          className={`template-card add ${designing ? "active" : ""}`}
          onClick={() => setDesigning((open) => !open)}
        >
          <span className="template-thumb add-thumb">
            <span>✦</span>
          </span>
          <strong>AI 设计</strong>
          <span>新建一个专属模板</span>
        </button>
      </div>

      {designing && (
        <div className="ai-panel open designer">
          <div className="ai-body" style={{ borderTop: 0 }}>
            <div className="field" style={{ marginBottom: 10 }}>
              <label>想要什么样的页面？</label>
              <textarea
                autoFocus
                value={request}
                placeholder="例如：复古胶片风，暖棕色调，标题用手写感的大字，照片像冲洗出来的相纸，背景有淡淡的颗粒感"
                onChange={(event) => setRequest(event.target.value)}
              />
            </div>
            <div className="section-label" style={{ fontSize: 13, marginBottom: 8 }}>
              灵感 <span className="label-hint">点一下填入，可以再改</span>
            </div>
            <div className="prompt-presets">
              {PROMPT_PRESETS.map((preset) => (
                <button
                  key={preset.name}
                  className={`prompt-preset ${request === preset.prompt ? "active" : ""}`}
                  onClick={() => setRequest(preset.prompt)}
                  title={preset.prompt}
                >
                  <span className="head">
                    {preset.name}
                    <span className="tag">{preset.group}</span>
                  </span>
                  <p>{preset.prompt}</p>
                </button>
              ))}
            </div>
            <div className="inline wrap" style={{ marginTop: 14 }}>
              {providerPicker}
              <span className="grow" />
              <button className="btn" onClick={() => setDesigning(false)} disabled={busy === "create"}>
                取消
              </button>
              <button className="btn primary" onClick={() => void create()} disabled={busy === "create"}>
                {busy === "create" ? "AI 设计中，约 30–60 秒…" : "生成模板"}
              </button>
            </div>
          </div>
        </div>
      )}

      {!designing && current && (
        <div className="ai-panel open designer">
          <div className="ai-body" style={{ borderTop: 0 }}>
            <div className="inline" style={{ justifyContent: "space-between", marginBottom: 10 }}>
              {renaming == null ? (
                <div className="inline">
                  <strong style={{ fontSize: 14 }}>{current.name}</strong>
                  <button className="btn small subtle" onClick={() => setRenaming(current.name)}>
                    重命名
                  </button>
                </div>
              ) : (
                <div className="inline">
                  <input
                    className="compact-input"
                    autoFocus
                    value={renaming}
                    maxLength={16}
                    onChange={(event) => setRenaming(event.target.value)}
                    onKeyDown={(event) => {
                      if (event.key === "Enter") void rename();
                      if (event.key === "Escape") setRenaming(null);
                    }}
                  />
                  <button className="btn small primary" onClick={() => void rename()}>
                    确定
                  </button>
                  <button className="btn small" onClick={() => setRenaming(null)}>
                    取消
                  </button>
                </div>
              )}
              <button className="btn small danger" onClick={() => void remove()}>
                删除模板
              </button>
            </div>
            {current.prompt && <div className="muted prompt-line">设计要求：{current.prompt}</div>}
            <div className="field" style={{ margin: "12px 0 10px" }}>
              <label>继续调整</label>
              <input
                value={refine}
                placeholder="例如：标题再大一些，背景换成浅米色，照片加白边"
                onChange={(event) => setRefine(event.target.value)}
                onKeyDown={(event) => {
                  if (event.key === "Enter" && !busy) void adjust();
                }}
              />
            </div>
            <div className="inline wrap">
              {providerPicker}
              <span className="grow" />
              <button className="btn primary" onClick={() => void adjust()} disabled={busy === "refine"}>
                {busy === "refine" ? "调整中…" : "按要求调整"}
              </button>
            </div>
          </div>
        </div>
      )}
    </>
  );
}
