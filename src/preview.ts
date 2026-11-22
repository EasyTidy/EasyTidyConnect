/**
 * 应用内预览用的页面（不影响发布出去的页面）：
 * - 隐藏滚动条（只是不显示，滚轮 / 触控板滚动照常）
 * - 内容更新后恢复之前的滚动位置（预览 iframe 每次改动都会重新加载，避免跳回顶部）
 * - 可选：滚动到指定元素（调整封面裁剪时自动定位到封面）
 */
const HIDE_SCROLLBARS =
  "<style>*{scrollbar-width:none!important}*::-webkit-scrollbar{display:none!important;width:0!important;height:0!important}</style>";

export type PreviewOptions = {
  /** 恢复到这个滚动位置 */
  scrollY?: number;
  /** 加载后把这个元素滚动到视口中央（优先于 scrollY） */
  focusSelector?: string;
};

/** 预览 iframe 通过 postMessage 上报滚动位置的消息类型。 */
export const PREVIEW_SCROLL_MESSAGE = "et-preview-scroll";

export function previewDocument(html: string, options: PreviewOptions = {}): string {
  if (!html) return html;
  const config = JSON.stringify({
    y: Math.max(0, Math.round(options.scrollY ?? 0)),
    focus: options.focusSelector ?? "",
    type: PREVIEW_SCROLL_MESSAGE,
  }).replace(/</g, "\\u003c");
  const script = `<script>window.ET_PREVIEW=1;(function(){var c=${config};
function restore(){var el=c.focus&&document.querySelector(c.focus);
if(el){el.scrollIntoView({block:"center"});}else if(c.y){window.scrollTo(0,c.y);}}
if(document.readyState==="loading"){document.addEventListener("DOMContentLoaded",restore);}else{restore();}
window.addEventListener("load",restore);
var t=0;window.addEventListener("scroll",function(){clearTimeout(t);t=setTimeout(function(){parent.postMessage({type:c.type,y:window.scrollY},"*");},80);},{passive:true});
})();</script>`;
  const extra = HIDE_SCROLLBARS + script;
  const head = html.indexOf("</head>");
  return head >= 0 ? html.slice(0, head) + extra + html.slice(head) : extra + html;
}
