import { useEffect, useRef, useState, type PointerEvent as ReactPointerEvent } from "react";

export type PhotoFocus = Record<string, [number, number]>;
type Size = { w: number; h: number };

/**
 * 需要固定比例（会裁剪）的位置：只有这些模板的封面（第一张照片）需要特定形状，
 * 其余照片在所有模板里都按原比例完整显示。
 */
export const COVER_RULES: Record<string, { slot: string; shape: "portrait" | "square"; ratio: number; selector: string }> = {
  xitie: { slot: "封面相框", shape: "portrait", ratio: 3 / 4, selector: ".photo" },
  garden: { slot: "拱门相框", shape: "portrait", ratio: 3 / 4, selector: ".arch" },
  editorial: { slot: "整屏封面", shape: "portrait", ratio: 9 / 16, selector: ".cover" },
  party: { slot: "贴纸", shape: "square", ratio: 1, selector: ".sticker" },
};

const SHAPE_NAMES = { portrait: "竖版", landscape: "横版", square: "方形" } as const;

export function orientationOf(size?: Size): keyof typeof SHAPE_NAMES | null {
  if (!size) return null;
  const ratio = size.w / size.h;
  if (ratio > 1.1) return "landscape";
  if (ratio < 0.9) return "portrait";
  return "square";
}

/** 照片形状是否适合模板的封面位置。 */
export function fitsRule(rule: (typeof COVER_RULES)[string], size?: Size) {
  if (!size) return true;
  const ratio = size.w / size.h;
  return rule.shape === "portrait" ? ratio <= 0.9 : ratio >= 0.8 && ratio <= 1.25;
}

/** 读取每张照片的原始尺寸（用于判断横竖版）。 */
export function usePhotoSizes(files: string[], urlOf: (file: string) => string) {
  const [sizes, setSizes] = useState<Record<string, Size>>({});
  const key = files.join("|");
  useEffect(() => {
    let cancelled = false;
    files.forEach((file) => {
      if (sizes[file]) return;
      const img = new Image();
      img.onload = () => {
        if (!cancelled) setSizes((current) => ({ ...current, [file]: { w: img.naturalWidth, h: img.naturalHeight } }));
      };
      img.src = urlOf(file);
    });
    return () => {
      cancelled = true;
    };
  }, [key]);
  return sizes;
}

/**
 * 照片网格：鼠标 / 触控拖拽排序（指针事件实现，不依赖 HTML5 拖放——Tauri 在 Windows 上会拦截它）。
 * 第一张是封面；需要固定形状的模板会标出不合适的封面并提供「调整裁剪」「设为封面」。
 */
export function PhotoGrid(props: {
  photos: string[];
  urlOf: (file: string) => string;
  sizes: Record<string, Size>;
  rule?: (typeof COVER_RULES)[string];
  busy: boolean;
  editing: string | null;
  onReorder: (photos: string[]) => void;
  onRemove: (file: string) => void;
  onAdjust: (file: string) => void;
  onAdd: () => void;
}) {
  const [order, setOrder] = useState(props.photos);
  const [dragging, setDragging] = useState<string | null>(null);
  const [offset, setOffset] = useState({ x: 0, y: 0 });
  const offsetRef = useRef({ x: 0, y: 0 });
  /** 按下时指针相对照片左上角的位置，拖动时照片跟随指针 */
  const drag = useRef<{ file: string; startX: number; startY: number; grabX: number; grabY: number; moved: boolean } | null>(null);
  const gridRef = useRef<HTMLDivElement | null>(null);

  useEffect(() => setOrder(props.photos), [props.photos.join("|")]);

  const moveTo = (x: number, y: number) => {
    offsetRef.current = { x, y };
    setOffset({ x, y });
  };

  const onPointerDown = (event: ReactPointerEvent<HTMLDivElement>, file: string) => {
    if ((event.target as HTMLElement).closest("button") || event.button !== 0) return;
    const rect = event.currentTarget.getBoundingClientRect();
    drag.current = { file, startX: event.clientX, startY: event.clientY, grabX: event.clientX - rect.left, grabY: event.clientY - rect.top, moved: false };
    event.currentTarget.setPointerCapture(event.pointerId);
  };

  const onPointerMove = (event: ReactPointerEvent<HTMLDivElement>) => {
    const state = drag.current;
    if (!state) return;
    if (!state.moved && Math.hypot(event.clientX - state.startX, event.clientY - state.startY) < 6) return;
    if (!state.moved) {
      state.moved = true;
      setDragging(state.file);
    }
    const tiles = [...(gridRef.current?.querySelectorAll<HTMLElement>("[data-photo]") ?? [])];
    // 指针下的另一张照片：把拖动的照片插到它的位置
    const over = tiles.find((tile) => {
      if (tile.dataset.photo === state.file) return false;
      const r = tile.getBoundingClientRect();
      return event.clientX >= r.left && event.clientX <= r.right && event.clientY >= r.top && event.clientY <= r.bottom;
    });
    if (over) {
      const target = over.dataset.photo!;
      const from = order.indexOf(state.file);
      const to = order.indexOf(target);
      const next = order.filter((item) => item !== state.file);
      next.splice(to, 0, state.file);
      if (from !== to) setOrder(next);
    }
    // 照片跟随指针：当前格子位置（去掉已应用的位移）+ 指针偏移
    const self = tiles.find((tile) => tile.dataset.photo === state.file);
    if (self) {
      const rect = self.getBoundingClientRect();
      const slotX = rect.left - offsetRef.current.x;
      const slotY = rect.top - offsetRef.current.y;
      moveTo(event.clientX - state.grabX - slotX, event.clientY - state.grabY - slotY);
    }
  };

  const onPointerUp = () => {
    const state = drag.current;
    drag.current = null;
    setDragging(null);
    moveTo(0, 0);
    if (state?.moved && order.join("|") !== props.photos.join("|")) props.onReorder(order);
  };

  return (
    <div className="studio-photos" ref={gridRef}>
      {order.map((file, index) => {
        const size = props.sizes[file];
        const shape = orientationOf(size);
        const isCover = index === 0;
        const misfit = isCover && props.rule && !fitsRule(props.rule, size);
        return (
          <div
            key={file}
            data-photo={file}
            className={`studio-photo ${dragging === file ? "dragging" : ""} ${props.editing === file ? "editing" : ""}`}
            style={dragging === file ? { transform: `translate(${offset.x}px, ${offset.y}px) scale(1.05)` } : undefined}
            onPointerDown={(event) => onPointerDown(event, file)}
            onPointerMove={onPointerMove}
            onPointerUp={onPointerUp}
            onPointerCancel={onPointerUp}
            title="拖动调整顺序"
          >
            <img src={props.urlOf(file)} alt="" draggable={false} />
            <div className="photo-badges">
              {isCover && props.rule && <span className="photo-badge cover">封面</span>}
              {shape && <span className="photo-badge">{SHAPE_NAMES[shape]}</span>}
            </div>
            {misfit && (
              <button className="photo-warn" onClick={() => props.onAdjust(file)} title={`${props.rule!.slot}建议使用${props.rule!.shape === "portrait" ? "竖版" : "方形"}照片`}>
                将被裁剪 · 调整
              </button>
            )}
            {isCover && props.rule && !misfit && (
              <button className="photo-adjust" onClick={() => props.onAdjust(file)}>
                调整裁剪
              </button>
            )}
            <button className="photo-remove" onClick={() => props.onRemove(file)} aria-label="移除照片">
              ×
            </button>
          </div>
        );
      })}
      <button className="photo-add" onClick={props.onAdd} disabled={props.busy}>
        <span>+</span>
        {props.busy ? "上传中..." : "添加照片"}
      </button>
    </div>
  );
}

/**
 * 裁剪区域调整：在原图上拖动取景框（按模板位置的比例），焦点实时写回草稿，右侧预览同步更新。
 */
export function CropEditor(props: {
  url: string;
  size?: Size;
  ratio: number;
  slot: string;
  focus: [number, number];
  onChange: (focus: [number, number]) => void;
  onClose: () => void;
}) {
  const box = useRef<HTMLDivElement | null>(null);
  const drag = useRef<{ x: number; y: number; focus: [number, number] } | null>(null);
  const size = props.size ?? { w: 4, h: 3 };
  const imageRatio = size.w / size.h;
  // 显示尺寸：最大 420×300
  const scale = Math.min(420 / size.w, 300 / size.h);
  const dw = size.w * scale;
  const dh = size.h * scale;
  const frameW = imageRatio > props.ratio ? dh * props.ratio : dw;
  const frameH = imageRatio > props.ratio ? dh : dw / props.ratio;
  const slackX = dw - frameW;
  const slackY = dh - frameH;
  const left = (props.focus[0] / 100) * slackX;
  const top = (props.focus[1] / 100) * slackY;
  const noCrop = slackX < 1 && slackY < 1;

  const onPointerDown = (event: ReactPointerEvent<HTMLDivElement>) => {
    drag.current = { x: event.clientX, y: event.clientY, focus: props.focus };
    event.currentTarget.setPointerCapture(event.pointerId);
  };
  const onPointerMove = (event: ReactPointerEvent<HTMLDivElement>) => {
    const state = drag.current;
    if (!state) return;
    const clamp = (value: number) => Math.max(0, Math.min(100, value));
    const nx = slackX > 0 ? clamp(state.focus[0] + ((event.clientX - state.x) / slackX) * 100) : 50;
    const ny = slackY > 0 ? clamp(state.focus[1] + ((event.clientY - state.y) / slackY) * 100) : 50;
    props.onChange([Math.round(nx * 10) / 10, Math.round(ny * 10) / 10]);
  };

  return (
    <div className="crop-editor">
      <div className="crop-head">
        <strong>调整{props.slot}的裁剪区域</strong>
        <span className="muted">拖动取景框，右侧预览实时更新</span>
      </div>
      <div className="crop-stage" ref={box} style={{ width: dw, height: dh }}>
        <img src={props.url} alt="" draggable={false} />
        <div
          className="crop-frame"
          style={{ left, top, width: frameW, height: frameH }}
          onPointerDown={onPointerDown}
          onPointerMove={onPointerMove}
          onPointerUp={() => (drag.current = null)}
          onPointerCancel={() => (drag.current = null)}
        />
      </div>
      {noCrop && <div className="muted">这张照片的比例正合适，不需要裁剪。</div>}
      <div className="inline wrap" style={{ marginTop: 10 }}>
        <button className="btn small" onClick={() => props.onChange([50, 50])}>
          居中
        </button>
        <button className="btn small primary" onClick={props.onClose}>
          完成
        </button>
        <span className="muted">保存项目后生效于发布页面</span>
      </div>
    </div>
  );
}