use crate::db::Database;
use crate::models::{AnniversaryProject, CustomTemplate};

const MINIMAL: &str = include_str!("../templates/minimal.html");
const POLAROID: &str = include_str!("../templates/polaroid.html");
const LETTER: &str = include_str!("../templates/letter.html");
const TIMELINE: &str = include_str!("../templates/timeline.html");
const STARRY: &str = include_str!("../templates/starry.html");
const XITIE: &str = include_str!("../templates/xitie.html");
const GARDEN: &str = include_str!("../templates/garden.html");
const EDITORIAL: &str = include_str!("../templates/editorial.html");
const PARTY: &str = include_str!("../templates/party.html");
const FILM: &str = include_str!("../templates/film.html");
const CUSTOM: &str = include_str!("../templates/custom.html");

pub const TEMPLATES: [(&str, &str); 10] = [
    ("minimal", "极简"),
    ("polaroid", "拍立得"),
    ("letter", "信纸"),
    ("timeline", "时间轴"),
    ("starry", "星空"),
    ("xitie", "中式喜帖"),
    ("garden", "森系花园"),
    ("editorial", "轻奢杂志"),
    ("party", "派对邀请"),
    ("film", "胶片时光"),
];

/// 深色背景的模板，音乐按钮使用深色毛玻璃。
const DARK_TEMPLATES: [&str; 4] = ["starry", "xitie", "editorial", "film"];

/// AI 模板在项目里的 template_id 形如 `ai-12`。
pub const CUSTOM_TEMPLATE_PREFIX: &str = "ai-";

/// 给 AI 看的骨架结构（与 templates/custom.html 的 body 保持一致）。
pub const CUSTOM_SKELETON_OUTLINE: &str = r#"<body>
<div class="et-page">
  <div class="et-deco et-deco-1"></div><div class="et-deco et-deco-2"></div><div class="et-deco et-deco-3"></div>
  <main class="et-sheet">
    <header class="et-hero">
      <div class="et-date">日期</div>
      <h1 class="et-title">标题</h1>
      <div class="et-to">收件人名字</div>
    </header>
    <div class="et-divider"></div>
    <article class="et-letter">
      <p class="et-opening">开场</p>
      <div class="et-body"><p>正文段落</p><p>正文段落</p></div>
      <div class="et-gallery"><img><img></div>
      <figure class="et-media"><video></video></figure>
      <p class="et-closing">结尾</p>
    </article>
    <footer class="et-sign"><span class="et-sign-mark"></span></footer>
  </main>
</div>
</body>"#;

pub fn custom_template_id(template_id: &str) -> Option<i64> {
    template_id.strip_prefix(CUSTOM_TEMPLATE_PREFIX)?.parse().ok()
}

/// 渲染项目；AI 模板会从数据库读取对应 CSS，模板已被删除时回退到「极简」。
pub fn render_with_db(
    db: &Database,
    project: &AnniversaryProject,
    photo_base: &str,
    media_base: &str,
) -> String {
    let custom = custom_template_id(&project.template_id)
        .and_then(|id| db.get_custom_template(id).ok().flatten());
    render_project_with(project, photo_base, media_base, custom.as_ref())
}

pub fn render_project(
    project: &AnniversaryProject,
    photo_base: &str,
    media_base: &str,
) -> String {
    render_project_with(project, photo_base, media_base, None)
}

pub fn render_project_with(
    project: &AnniversaryProject,
    photo_base: &str,
    media_base: &str,
    custom: Option<&CustomTemplate>,
) -> String {
    let custom = custom.filter(|_| custom_template_id(&project.template_id).is_some());
    let template = match (custom, project.template_id.as_str()) {
        (Some(_), _) => CUSTOM,
        (None, "polaroid") => POLAROID,
        (None, "letter") => LETTER,
        (None, "timeline") => TIMELINE,
        (None, "starry") => STARRY,
        (None, "xitie") => XITIE,
        (None, "garden") => GARDEN,
        (None, "editorial") => EDITORIAL,
        (None, "party") => PARTY,
        (None, "film") => FILM,
        _ => MINIMAL,
    };
    let dark = custom.map(|t| t.dark).unwrap_or_else(|| DARK_TEMPLATES.contains(&project.template_id.as_str()));
    let template_css = custom.map(|t| sanitize_css(&t.css)).unwrap_or_default();

    let theme = if is_hex_color(&project.theme) {
        project.theme.clone()
    } else {
        "#2563eb".to_string()
    };

    let body = project
        .body
        .split('\n')
        .filter(|line| !line.trim().is_empty())
        .map(|line| format!("<p>{}</p>", escape_html(line.trim())))
        .collect::<Vec<_>>()
        .join("\n");

    // 照片裁剪焦点（object-position / background-position），只在需要裁剪的位置生效；默认居中。
    let focus_of = |file: &str| {
        let [x, y] = project.photo_focus.get(file).copied().unwrap_or([50.0, 50.0]);
        format!("{:.1}% {:.1}%", x.clamp(0.0, 100.0), y.clamp(0.0, 100.0))
    };
    let photo_tags: Vec<String> = project
        .photos
        .iter()
        .enumerate()
        .map(|(index, file)| {
            format!(
                "<img src=\"{}{}\" alt=\"\" loading=\"lazy\" data-et-index=\"{}\" style=\"object-position:{}\">",
                photo_base,
                escape_html(file),
                index,
                focus_of(file)
            )
        })
        .collect();
    let photos = photo_tags.join("\n");
    // 封面模板：第一张照片做封面，其余进相册。
    let photos_rest = photo_tags.iter().skip(1).cloned().collect::<Vec<_>>().join("\n");
    // 用于 style="--cover:{{cover_bg}}"；没有照片时为 none，避免 url('') 请求页面自身。
    // 同时带上封面的裁剪焦点 --cover-pos，模板用 background-position:var(--cover-pos,center)。
    let (cover_bg, cover_class) = match project.photos.first() {
        Some(file) => (
            format!(
                "url('{}');--cover-pos:{}",
                escape_html(&format!("{photo_base}{file}")),
                focus_of(file)
            ),
            "has-cover",
        ),
        None => ("none".to_string(), "no-cover"),
    };
    let date = DateParts::parse(&project.event_date);

    let lightbox = if project.photos.is_empty() {
        String::new()
    } else {
        let flag = if project.slideshow { "window.ET_SLIDESHOW=true;" } else { "" };
        format!("<style>{LIGHTBOX_CSS}</style><script>{flag}{LIGHTBOX_JS}</script>")
    };
    let media = render_video(project, media_base);
    let music = render_music(project, media_base, dark);
    // custom_css 放在 <style> 里，禁止 `</` 以免提前闭合 style 标签注入脚本。
    // 视频基础样式放在模板样式之前：AI 模板可以覆盖它，用户的自定义 CSS 最后生效
    let media_css = format!(
        "{MEDIA_CSS}.et-media{{--et-bleed:{}px}}\n",
        media_bleed(if custom.is_some() { "" } else { project.template_id.as_str() })
    );
    let user_css = project.custom_css.replace("</", "<\\/");
    let (template_css, custom_css) = if custom.is_some() {
        (format!("{media_css}{template_css}"), user_css)
    } else {
        (template_css, format!("{media_css}{user_css}"))
    };

    template
        .replace("{{theme}}", &theme)
        .replace("{{title}}", &escape_html(&project.title))
        .replace("{{recipient}}", &escape_html(&project.recipient))
        .replace("{{date}}", &escape_html(&project.event_date))
        .replace("{{opening}}", &escape_html(&project.opening))
        .replace("{{body}}", &body)
        .replace("{{closing}}", &escape_html(&project.closing))
        .replace("{{photos_rest}}", &photos_rest)
        .replace("{{photos}}", &photos)
        .replace("{{cover_bg}}", &cover_bg)
        .replace("{{cover_class}}", cover_class)
        .replace("{{year}}", &date.year)
        .replace("{{month}}", &date.month)
        .replace("{{day}}", &date.day)
        .replace("{{weekday}}", &date.weekday)
        .replace("{{date_cn}}", &date.chinese)
        .replace("{{date_dot}}", &date.dot)
        .replace("{{date_md}}", &date.md)
        .replace("{{template_js}}", PRESET_JS)
        .replace("{{media}}", &media)
        .replace("{{template_css}}", &template_css)
        .replace("{{custom_css}}", &custom_css)
        // 背景音乐按钮是 position:fixed，放在 body 末尾，避免被模板里带 transform/overflow 的容器影响。
        .replacen("</body>", &format!("{lightbox}{music}</body>"), 1)
}

fn render_video(project: &AnniversaryProject, media_base: &str) -> String {
    let video = media_url(&project.video, media_base);
    if video.is_empty() {
        return String::new();
    }
    // #t=0.1：iOS 在点击播放前显示首帧而不是黑块；x5-* / webkit-playsinline：微信内联播放不全屏
    let src = if video.contains('#') { video } else { format!("{video}#t=0.1") };
    format!(
        "<figure class=\"et-media\"><video controls playsinline webkit-playsinline x5-playsinline x5-video-player-type=\"h5-page\" preload=\"metadata\" controlslist=\"nodownload\" src=\"{}\"></video></figure>",
        escape_html(&src)
    )
}

/// 右上角悬浮的背景音乐按钮：小巧的毛玻璃圆钮 + 音符图标，播放时音符轻摆、外圈流光旋转并飘出小音符。
fn render_music(project: &AnniversaryProject, media_base: &str, dark: bool) -> String {
    let music = media_url(&project.music, media_base);
    if music.is_empty() {
        return String::new();
    }
    format!(
        concat!(
            "<style>{css}</style>",
            "<audio id=\"et-bgm\" src=\"{src}\" loop preload=\"auto\" playsinline></audio>",
            "<button id=\"et-music\" class=\"et-music{tone}\" type=\"button\" aria-label=\"背景音乐\" aria-pressed=\"false\" title=\"背景音乐\">",
            "<span class=\"et-ring\" aria-hidden=\"true\"></span>",
            "<svg class=\"et-icon\" viewBox=\"0 0 24 24\" aria-hidden=\"true\"><path fill=\"currentColor\" d=\"M10.2 3.6a1 1 0 0 1 1.3-.95l5.1 1.6a2.6 2.6 0 0 1 1.8 2.48v1.1a.9.9 0 0 1-1.17.86L12.2 7.2v9.65A3.35 3.35 0 1 1 10.2 13.8z\"/></svg>",
            "<span class=\"et-note\" style=\"--dx:-16px;--dy:22px\" aria-hidden=\"true\">&#9834;&#xFE0E;</span>",
            "<span class=\"et-note\" style=\"--dx:-26px;--dy:14px;--d:1.1s\" aria-hidden=\"true\">&#9835;&#xFE0E;</span>",
            "<span class=\"et-note\" style=\"--dx:-8px;--dy:28px;--d:2.2s\" aria-hidden=\"true\">&#9834;&#xFE0E;</span>",
            "</button>",
            "<div id=\"et-music-tip\" class=\"et-music-tip{tone} hide\" aria-hidden=\"true\">轻触开启音乐</div>",
            "<script>{js}</script>"
        ),
        css = MUSIC_CSS,
        src = escape_html(&music),
        tone = if dark { " dark" } else { "" },
        js = MUSIC_JS,
    )
}

/// 模板可通过 CSS 变量定制按钮：--et-music-bg / --et-music-fg / --et-music-ring。
const MUSIC_CSS: &str = r#"
.et-music{--sz:34px;--bg:var(--et-music-bg,rgba(255,255,255,.72));--fg:var(--et-music-fg,var(--accent,#e11d48));--rg:var(--et-music-ring,var(--accent,#e11d48));position:fixed;top:max(12px,env(safe-area-inset-top));right:max(12px,env(safe-area-inset-right));z-index:2147483000;width:var(--sz);height:var(--sz);padding:0;margin:0;border:0;border-radius:50%;display:grid;place-items:center;cursor:pointer;color:var(--fg);background:var(--bg);-webkit-backdrop-filter:blur(12px) saturate(1.6);backdrop-filter:blur(12px) saturate(1.6);box-shadow:0 0 0 .5px rgba(0,0,0,.08),0 4px 14px -4px rgba(0,0,0,.28);-webkit-tap-highlight-color:transparent;transition:transform .25s cubic-bezier(.2,.8,.2,1),box-shadow .25s ease,opacity .3s ease;opacity:.88}
.et-music.dark{--bg:var(--et-music-bg,rgba(20,24,40,.55));--fg:var(--et-music-fg,#fff);box-shadow:0 0 0 .5px rgba(255,255,255,.18),0 4px 16px -4px rgba(0,0,0,.6)}
.et-music.playing{opacity:1}
.et-music:active{transform:scale(.88)}
.et-music:focus-visible{outline:2px solid var(--rg);outline-offset:3px}
.et-icon{position:relative;width:52%;height:52%;transform-origin:45% 80%}
.et-music.playing .et-icon{animation:et-sway 1.6s ease-in-out infinite}
.et-ring{position:absolute;inset:-2px;border-radius:50%;padding:1.5px;background:conic-gradient(from 0deg,transparent 0 55%,var(--rg) 85%,transparent 100%);-webkit-mask:linear-gradient(#000 0 0) content-box,linear-gradient(#000 0 0);-webkit-mask-composite:xor;mask-composite:exclude;opacity:0;transition:opacity .4s ease}
.et-music.playing .et-ring{opacity:1;animation:et-spin 2.8s linear infinite}
.et-note{position:absolute;left:30%;top:55%;font:11px/1 "Segoe UI Symbol","Apple Symbols",sans-serif;color:var(--rg);opacity:0;pointer-events:none}
.et-music.playing .et-note{animation:et-float 3.3s ease-out var(--d,0s) infinite}
.et-music-tip{position:fixed;top:calc(max(12px,env(safe-area-inset-top)) + 7px);right:calc(max(12px,env(safe-area-inset-right)) + 42px);z-index:2147483000;padding:3px 9px;border-radius:999px;background:rgba(255,255,255,.78);color:#334155;box-shadow:0 0 0 .5px rgba(0,0,0,.08),0 4px 12px -4px rgba(0,0,0,.2);font:11px/1.5 -apple-system,BlinkMacSystemFont,"PingFang SC","Microsoft YaHei",sans-serif;letter-spacing:.06em;white-space:nowrap;-webkit-backdrop-filter:blur(12px);backdrop-filter:blur(12px);pointer-events:none;transition:opacity .4s ease,transform .4s ease}
.et-music-tip.dark{background:rgba(20,24,40,.6);color:#e2e8f0}
.et-music-tip.hide{opacity:0;transform:translateX(6px)}
@keyframes et-spin{to{transform:rotate(360deg)}}
@keyframes et-sway{0%,100%{transform:rotate(-7deg) translateY(0)}50%{transform:rotate(7deg) translateY(-1px)}}
@keyframes et-float{0%{transform:translate(0,0) scale(.5);opacity:0}18%{opacity:.9}100%{transform:translate(var(--dx),var(--dy)) rotate(-25deg) scale(1);opacity:0}}
@media (hover:hover){.et-music:hover{opacity:1;transform:scale(1.08);box-shadow:0 0 0 .5px rgba(0,0,0,.1),0 8px 20px -6px rgba(0,0,0,.35)}}
@media (min-width:768px){.et-music{--sz:38px;top:20px;right:20px}.et-music-tip{top:29px;right:68px}}
@media (max-width:340px){.et-music{--sz:30px;top:max(8px,env(safe-area-inset-top));right:max(8px,env(safe-area-inset-right))}.et-music-tip{display:none}}
@media (prefers-reduced-motion:reduce){.et-music .et-icon,.et-music .et-ring,.et-music .et-note{animation:none!important}.et-music.playing .et-ring{background:var(--rg)}}
@media (forced-colors:active){.et-music{border:1px solid ButtonText;color:ButtonText}}
@media print{.et-music,.et-music-tip{display:none!important}}
"#;

// 浏览器（尤其 iOS / 微信）禁止有声自动播放：先尝试播放，被拦截就在用户第一次轻触页面时开始。
// 在编辑器预览（iframe）里不自动播放，避免每次改字都重新响起。
const MUSIC_JS: &str = r#"(function(){
var a=document.getElementById('et-bgm'),b=document.getElementById('et-music'),t=document.getElementById('et-music-tip');
if(!a||!b)return;
var embedded=window.self!==window.top,userPaused=false,resume=false,events=['click','touchend','keydown'];
function ui(){var on=!a.paused;b.classList.toggle('playing',on);b.setAttribute('aria-pressed',on?'true':'false');if(on&&t)t.classList.add('hide');}
function play(showTip){var r=a.play();if(r&&r.catch)r.catch(function(){if(showTip&&t&&!userPaused&&a.paused)t.classList.remove('hide');});}
function off(){events.forEach(function(n){document.removeEventListener(n,first,true);});}
function first(e){if(e.target&&e.target.closest&&e.target.closest('#et-music'))return;if(userPaused||!a.paused){off();return;}play(false);}
a.addEventListener('play',ui);a.addEventListener('pause',ui);a.addEventListener('playing',off);
b.addEventListener('click',function(){if(a.paused){userPaused=false;play(false);}else{userPaused=true;a.pause();}off();});
document.addEventListener('visibilitychange',function(){if(document.hidden){if(!a.paused){resume=true;a.pause();}}else if(resume&&!userPaused){resume=false;play(false);}});
if(embedded)return;
events.forEach(function(n){document.addEventListener(n,first,true);});
document.addEventListener('WeixinJSBridgeReady',function(){play(false);},false);
play(true);
})();"#;

fn media_url(value: &str, media_base: &str) -> String {
    let value = value.trim();
    if value.is_empty() {
        return String::new();
    }
    if value.starts_with("http://") || value.starts_with("https://") || value.starts_with("data:") {
        return value.to_string();
    }
    let value = value.trim_start_matches("media/");
    format!("{media_base}{value}")
}

/// 照片点击放大：全屏查看，左右滑动 / 按钮 / 方向键切换，点空白处或下滑关闭。所有模板共用。
/// 项目开启「自动放大轮播」时（window.ET_SLIDESHOW），照片滚动进入视野后自动全屏逐张播放，
/// 播完最后一张自动关闭；点照片暂停 / 继续。应用内预览（window.ET_PREVIEW）不自动弹出，点照片即可看效果。
const LIGHTBOX_CSS: &str = r#"
.photos img,.gallery img,.stack img,.frames img,.et-gallery img{cursor:zoom-in}
.et-lb{position:fixed;inset:0;z-index:2147483600;display:flex;align-items:center;justify-content:center;overflow:hidden;background:rgba(10,10,14,.92);-webkit-backdrop-filter:blur(6px);backdrop-filter:blur(6px);visibility:hidden;opacity:0;transition:opacity .25s ease,visibility 0s linear .25s;touch-action:pan-y}
.et-lb.open{visibility:visible;opacity:1;transition:opacity .25s ease}
.et-lb img{display:block;max-width:94vw;max-height:86vh;max-height:86dvh;width:auto;height:auto;object-fit:contain;border-radius:6px;box-shadow:0 24px 60px rgba(0,0,0,.5);transform:scale(.96);transition:transform .3s ease,opacity .35s ease;user-select:none;-webkit-user-select:none}
.et-lb.open img{transform:none}
.et-lb img.fading{opacity:0}
.et-lb.playing img{animation:et-lb-zoom var(--et-lb-delay,3.5s) ease-out both}
@keyframes et-lb-zoom{from{transform:scale(1)}to{transform:scale(1.045)}}
.et-lb button{position:absolute;z-index:2;width:42px;height:42px;padding:0;border:0;border-radius:50%;background:rgba(255,255,255,.14);color:#fff;font:400 26px/42px -apple-system,"Segoe UI",sans-serif;text-align:center;cursor:pointer;-webkit-backdrop-filter:blur(8px);backdrop-filter:blur(8px);-webkit-tap-highlight-color:transparent}
.et-lb button:hover{background:rgba(255,255,255,.24)}
.et-lb-close{top:max(14px,env(safe-area-inset-top));right:max(14px,env(safe-area-inset-right))}
.et-lb-play{top:max(14px,env(safe-area-inset-top));right:calc(max(14px,env(safe-area-inset-right)) + 52px);display:none}
.et-lb.auto .et-lb-play{display:block}
.et-lb-play:before{content:"";position:absolute;left:50%;top:50%;width:0;height:0;margin:-7px 0 0 -4px;border-style:solid;border-width:7px 0 7px 12px;border-color:transparent transparent transparent #fff}
.et-lb.playing .et-lb-play:before{width:4px;height:14px;margin:-7px 0 0 -5px;border-width:0 4px;border-color:#fff}
.et-lb-prev{left:12px;top:50%;margin-top:-21px}
.et-lb-next{right:12px;top:50%;margin-top:-21px}
.et-lb.single .et-lb-prev,.et-lb.single .et-lb-next,.et-lb.single .et-lb-count{display:none}
.et-lb-count{position:absolute;z-index:2;left:0;right:0;bottom:max(18px,env(safe-area-inset-bottom));text-align:center;color:rgba(255,255,255,.82);font:13px -apple-system,"PingFang SC",sans-serif;letter-spacing:.12em}
.et-lb-bar{position:absolute;left:0;right:0;top:0;height:3px;display:none}
.et-lb.playing .et-lb-bar{display:block}
.et-lb-bar i{display:block;height:100%;width:100%;background:rgba(255,255,255,.75);transform-origin:left;animation:et-lb-bar var(--et-lb-delay,3.5s) linear both}
@keyframes et-lb-bar{from{transform:scaleX(0)}to{transform:scaleX(1)}}
@media (hover:none){.et-lb-prev,.et-lb-next{display:none}}
@media (prefers-reduced-motion:reduce){.et-lb.playing img{animation:none}}
html.et-lb-lock,html.et-lb-lock body{overflow:hidden}
"#;

const LIGHTBOX_JS: &str = r#"(function(){
var SEL='.photos img,.gallery img,.stack img,.frames img,.et-gallery img';
var AUTO=!!window.ET_SLIDESHOW,DELAY=3500;
var box,img,count,bar,group=[],index=0,sx=0,sy=0,timer=0,playing=false;
function build(){
  box=document.createElement('div');box.className='et-lb'+(AUTO?' auto':'');box.setAttribute('role','dialog');box.setAttribute('aria-label','查看照片');
  box.style.setProperty('--et-lb-delay',DELAY+'ms');
  box.innerHTML='<div class="et-lb-bar"><i></i></div><button class="et-lb-play" type="button" aria-label="播放 / 暂停"></button><button class="et-lb-close" type="button" aria-label="关闭">×</button><button class="et-lb-prev" type="button" aria-label="上一张">‹</button><img alt=""><button class="et-lb-next" type="button" aria-label="下一张">›</button><div class="et-lb-count"></div>';
  document.body.appendChild(box);img=box.querySelector('img');count=box.querySelector('.et-lb-count');bar=box.querySelector('.et-lb-bar i');
  img.addEventListener('load',function(){img.classList.remove('fading');});
  box.addEventListener('click',function(e){
    if(e.target.closest('.et-lb-prev')){go(-1);return;}
    if(e.target.closest('.et-lb-next')){go(1);return;}
    if(e.target.closest('.et-lb-play')||(AUTO&&e.target===img)){playing?pause():play();return;}
    if(e.target!==img)close();
  });
  box.addEventListener('touchstart',function(e){sx=e.touches[0].clientX;sy=e.touches[0].clientY;},{passive:true});
  box.addEventListener('touchend',function(e){
    var dx=e.changedTouches[0].clientX-sx,dy=e.changedTouches[0].clientY-sy;
    if(Math.abs(dx)>50&&Math.abs(dx)>Math.abs(dy))go(dx<0?1:-1);else if(dy>90)close();
  });
}
function restart(el){el.style.animation='none';void el.offsetWidth;el.style.animation='';}
function schedule(){
  clearTimeout(timer);if(!playing)return;
  restart(bar);restart(img);
  timer=setTimeout(function(){if(index>=group.length-1)close();else go(1);},DELAY);
}
function show(){
  var t=group[index],src=t.currentSrc||t.src;
  if(img.getAttribute('src')!==src){img.classList.add('fading');img.src=src;if(img.complete)img.classList.remove('fading');}
  count.textContent=(index+1)+' / '+group.length;box.classList.toggle('single',group.length<2);schedule();
}
function go(d){if(group.length<2)return;index=(index+d+group.length)%group.length;show();}
function play(){playing=true;box.classList.add('playing');schedule();}
function pause(){playing=false;clearTimeout(timer);box.classList.remove('playing');}
function sorted(container){return [].slice.call(container.querySelectorAll('img')).sort(function(a,b){return (+a.dataset.etIndex||0)-(+b.dataset.etIndex||0);});}
function open(t,autoplay){
  if(!box)build();
  group=sorted(t.parentNode);index=Math.max(0,group.indexOf(t));
  box.classList.add('open');document.documentElement.classList.add('et-lb-lock');
  if(autoplay)play();else pause();
  show();
}
function close(){pause();box.classList.remove('open');document.documentElement.classList.remove('et-lb-lock');}
document.addEventListener('click',function(e){
  var t=e.target;if(!t||!t.matches||!t.matches(SEL))return;
  if(t.dataset.dragged==='1'){t.dataset.dragged='';return;}
  e.preventDefault();open(t,AUTO);
});
document.addEventListener('keydown',function(e){
  if(!box||!box.classList.contains('open'))return;
  if(e.key==='Escape')close();else if(e.key==='ArrowRight')go(1);else if(e.key==='ArrowLeft')go(-1);
  else if(AUTO&&e.key===' '){e.preventDefault();playing?pause():play();}
});
// 自动轮播：第一组照片滚动进入视野时自动放大播放一遍（每次打开页面只自动触发一次）
if(AUTO&&!window.ET_PREVIEW&&'IntersectionObserver' in window){
  var first=document.querySelector(SEL);
  if(first){
    var io=new IntersectionObserver(function(entries){
      if(!entries.some(function(x){return x.isIntersecting;}))return;
      io.disconnect();
      setTimeout(function(){if(!box||!box.classList.contains('open'))open(sorted(first.parentNode)[0],true);},600);
    },{threshold:.35});
    io.observe(first.parentNode);
  }
}
})();"#;

/// 视频：不加卡片外框，直接铺满内容区并向两侧外扩（--et-bleed，按模板内边距设置），圆角 + 柔和阴影。
const MEDIA_CSS: &str = r#"
.et-media{--et-bleed:0px;position:relative;z-index:1;margin:30px calc(-1 * var(--et-bleed));padding:0;border:0;background:none}
.et-media video{display:block;width:100%;height:auto;max-height:80vh;object-fit:contain;border-radius:14px;background:#000;box-shadow:0 22px 48px -22px rgba(0,0,0,.6),0 0 0 1px rgba(127,127,127,.12)}
@media (min-width:768px){.et-media{margin-top:36px;margin-bottom:36px}.et-media video{border-radius:18px}}
"#;

/// 各模板放视频的容器左右内边距减去 10–16px：视频几乎铺满卡片，又不贴边。
fn media_bleed(template_id: &str) -> u32 {
    match template_id {
        "minimal" => 44,  // .card 左右 58px
        "letter" => 48,   // .letter 62px
        "starry" => 32,   // .card 46px
        "timeline" => 18, // .item 32px
        "polaroid" => 14, // .note 28px
        "editorial" => 14, // .page 26px
        "xitie" | "garden" | "party" | "film" => 12, // 22px
        _ => 8,           // AI 模板 .et-page 18px
    }
}

/// 请柬类预设模板共用的脚本：滚动入场动画 + 纪念日倒计时。
/// 倒计时文案可用 data-before / data-today / data-after 覆盖。
/// 没有脚本时内容照常显示（动画依赖 html.js 类名）。
const PRESET_JS: &str = r#"(function(){
var root=document.documentElement;root.classList.add('js');
var els=[].slice.call(document.querySelectorAll('[data-reveal]'));
if('IntersectionObserver' in window){var io=new IntersectionObserver(function(list){list.forEach(function(e){if(e.isIntersecting){e.target.classList.add('in');io.unobserve(e.target);}});},{threshold:.12});els.forEach(function(el){io.observe(el);});}
else{els.forEach(function(el){el.classList.add('in');});}
var cd=document.querySelector('[data-countdown]');if(!cd)return;
var m=/^(\d{4})-(\d{2})-(\d{2})$/.exec(cd.getAttribute('data-countdown')||'');if(!m)return;
var day=new Date(+m[1],+m[2]-1,+m[3]),now=new Date();now.setHours(0,0,0,0);
var diff=Math.round((day-now)/86400000),num=cd.querySelector('.cd-num'),label=cd.querySelector('.cd-label'),unit=cd.querySelector('.cd-unit');
var ds=cd.dataset;
if(diff>0){label.textContent=ds.before||'距离那天还有';num.textContent=diff;}
else if(diff===0){label.textContent=ds.today||'就是今天';num.textContent='♥';if(unit)unit.textContent='';}
else{label.textContent=ds.after||'我们已相伴';num.textContent=-diff;}
cd.hidden=false;
})();"#;

/// 日期拆分，供请柬类模板排版（大号数字、中文日期、星期）。解析失败时全部为空。
#[derive(Default)]
struct DateParts {
    year: String,
    month: String,
    day: String,
    weekday: String,
    chinese: String,
    /// 2026.12.24
    dot: String,
    /// 12.24
    md: String,
}

impl DateParts {
    fn parse(value: &str) -> Self {
        use chrono::Datelike;
        let Ok(date) = chrono::NaiveDate::parse_from_str(value.trim(), "%Y-%m-%d") else {
            return Self::default();
        };
        const WEEKDAYS: [&str; 7] = ["星期一", "星期二", "星期三", "星期四", "星期五", "星期六", "星期日"];
        let year_cn: String = date
            .year()
            .to_string()
            .chars()
            .map(|c| match c {
                '0' => '〇',
                d => CN_DIGITS[d.to_digit(10).unwrap_or(0) as usize],
            })
            .collect();
        Self {
            year: date.year().to_string(),
            month: format!("{:02}", date.month()),
            day: format!("{:02}", date.day()),
            weekday: WEEKDAYS[date.weekday().num_days_from_monday() as usize].to_string(),
            dot: date.format("%Y.%m.%d").to_string(),
            md: date.format("%m.%d").to_string(),
            chinese: format!(
                "{year_cn}年{}月{}日",
                chinese_number(date.month()),
                chinese_number(date.day())
            ),
        }
    }
}

const CN_DIGITS: [char; 10] = ['〇', '一', '二', '三', '四', '五', '六', '七', '八', '九'];

/// 1–31 转中文：十、十二、二十、二十四……
fn chinese_number(n: u32) -> String {
    match n {
        0..=9 => CN_DIGITS[n as usize].to_string(),
        10 => "十".to_string(),
        11..=19 => format!("十{}", CN_DIGITS[(n % 10) as usize]),
        _ if n % 10 == 0 => format!("{}十", CN_DIGITS[(n / 10) as usize]),
        _ => format!("{}十{}", CN_DIGITS[(n / 10) as usize], CN_DIGITS[(n % 10) as usize]),
    }
}

fn is_hex_color(value: &str) -> bool {
    let value = value.trim();
    value.len() == 7
        && value.starts_with('#')
        && value[1..].chars().all(|ch| ch.is_ascii_hexdigit())
}

/// AI 生成 CSS 的上限，防止异常输出撑爆页面。
const MAX_TEMPLATE_CSS: usize = 60 * 1024;

/// 清理 AI 生成的 CSS：不允许闭合 style 标签、@import、外部 url()、旧式脚本表达式，
/// 也不允许改动音乐按钮（.et-music）的样式。只保留 data:image 内联图片。
pub fn sanitize_css(css: &str) -> String {
    let mut css: String = css.chars().take(MAX_TEMPLATE_CSS).collect();
    css = css.replace("</", "<\\/").replace("<!--", "").replace("-->", "");

    let lower = css.to_ascii_lowercase();
    let mut out = String::with_capacity(css.len());
    let mut i = 0;
    while i < css.len() {
        let rest = &lower[i..];
        if rest.starts_with("@import") {
            // 跳过整条 @import 语句。
            let end = rest.find(';').map(|n| i + n + 1).unwrap_or(css.len());
            i = end;
            continue;
        }
        if rest.starts_with("url(") {
            // 带引号的 url("...") 里可能出现 ')'（如 SVG data URI），要先跳过引号内的内容。
            let arg = &rest[4..];
            let lead = arg.len() - arg.trim_start().len();
            let search_from = match arg[lead..].chars().next() {
                Some(q @ ('"' | '\'')) => arg[lead + 1..]
                    .find(q)
                    .map(|n| 4 + lead + 1 + n + 1)
                    .unwrap_or(rest.len()),
                _ => 4,
            };
            let Some(n) = rest.get(search_from..).and_then(|tail| tail.find(')')) else {
                out.push_str("none");
                break;
            };
            let close = i + search_from + n;
            let inner = css[i + 4..close]
                .trim()
                .trim_matches(|c| c == '"' || c == '\'')
                .trim();
            if inner.to_ascii_lowercase().starts_with("data:image/") {
                out.push_str(&css[i..=close]);
            } else {
                out.push_str("none");
            }
            i = close + 1;
            continue;
        }
        let ch = css[i..].chars().next().unwrap_or(' ');
        out.push(ch);
        i += ch.len_utf8();
    }

    for banned in ["expression(", "javascript:", "behavior:", "-moz-binding"] {
        while let Some(pos) = out.to_ascii_lowercase().find(banned) {
            out.replace_range(pos..pos + banned.len(), "");
        }
    }
    // 音乐按钮的样式由系统统一控制。
    out.replace(".et-music", ".et-x-music")
}

pub fn escape_html(value: &str) -> String {
    value
        .replace('&', "&amp;")
        .replace('<', "&lt;")
        .replace('>', "&gt;")
        .replace('"', "&quot;")
        .replace('\'', "&#39;")
}

#[cfg(test)]
mod tests {
    use super::*;

    fn project(music: &str) -> AnniversaryProject {
        AnniversaryProject {
            id: 1,
            slug: "demo".into(),
            name: "demo".into(),
            event_date: String::new(),
            title: "t".into(),
            recipient: String::new(),
            opening: String::new(),
            body: String::new(),
            closing: String::new(),
            template_id: "minimal".into(),
            theme: "#db2777".into(),
            photos: vec![],
            music: music.into(),
            video: String::new(),
            custom_css: String::new(),
            photo_focus: Default::default(),
            slideshow: false,
            created_at: String::new(),
            updated_at: String::new(),
        }
    }

    #[test]
    fn music_renders_as_floating_button_before_body_end() {
        for (id, _) in TEMPLATES {
            let mut p = project("song.mp3");
            p.template_id = id.to_string();
            let html = render_project(&p, "photos/", "media/");
            assert!(html.contains("id=\"et-music\""), "{id}");
            assert!(html.contains("src=\"media/song.mp3\""), "{id}");
            assert!(!html.contains("<audio controls"), "{id}");
            let button = html.find("id=\"et-music\"").unwrap();
            assert!(button < html.rfind("</body>").unwrap(), "{id}");
        }
    }

    #[test]
    fn sanitize_css_strips_dangerous_parts() {
        let css = r#"@import url("https://evil.test/x.css");
h1{color:red}
body{background:url(https://evil.test/a.png) center}
.a{background:url("data:image/svg+xml;utf8,<svg>(1)</svg>")}
.b{width:expression(alert(1));behavior:url(x.htc)}
</style><script>alert(1)</script>
.et-music{display:none}
.c{background:url("#;
        let out = sanitize_css(css);
        assert!(!out.contains("@import"));
        assert!(!out.contains("evil.test"));
        assert!(out.contains("h1{color:red}"));
        assert!(out.contains("background:none center"));
        assert!(out.contains(r#"url("data:image/svg+xml;utf8,<svg>(1)<\/svg>")"#));
        assert!(!out.to_ascii_lowercase().contains("expression("));
        assert!(!out.contains("</style>"));
        assert!(!out.contains(".et-music{"));
        assert!(out.ends_with("background:none"));
    }

    #[test]
    fn custom_template_uses_skeleton_and_css() {
        let mut p = project("song.mp3");
        p.template_id = "ai-7".into();
        let template = CustomTemplate {
            id: 7,
            name: "胶片".into(),
            prompt: String::new(),
            css: ".et-title{color:#123456}".into(),
            dark: true,
            created_at: String::new(),
            updated_at: String::new(),
        };
        let html = render_project_with(&p, "photos/", "media/", Some(&template));
        assert!(html.contains("class=\"et-sheet\""));
        assert!(html.contains(".et-title{color:#123456}"));
        assert!(html.contains("class=\"et-music dark\""));
        assert!(!html.contains("{{"));

        // 模板缺失时回退到极简，且不残留占位符。
        let fallback = render_project_with(&p, "photos/", "media/", None);
        assert!(fallback.contains("FOREVER"));
        assert!(!fallback.contains("{{"));
    }

    #[test]
    fn every_template_fills_all_placeholders() {
        for (id, _) in TEMPLATES {
            for (photos, date) in [(vec![], ""), (vec!["a.jpg".to_string(), "b.jpg".to_string()], "2026-12-24")] {
                let mut p = project("");
                p.template_id = id.to_string();
                p.photos = photos.clone();
                p.event_date = date.to_string();
                let html = render_project(&p, "photos/", "media/");
                assert!(!html.contains("{{"), "{id} 残留占位符");
                if !photos.is_empty() && ["xitie", "garden", "editorial", "party", "film"].contains(&id) {
                    assert!(html.contains("--cover:url('photos/a.jpg')"), "{id} 封面");
                    assert!(html.contains("class=\"has-cover\""), "{id}");
                }
            }
        }
    }

    #[test]
    fn photo_focus_drives_cover_and_object_position() {
        let mut p = project("");
        p.template_id = "xitie".into();
        p.photos = vec!["a.jpg".into(), "b.jpg".into()];
        p.photo_focus.insert("a.jpg".into(), [20.0, 80.0]);
        p.photo_focus.insert("b.jpg".into(), [150.0, -5.0]);
        let html = render_project(&p, "photos/", "media/");
        assert!(html.contains("--cover:url('photos/a.jpg');--cover-pos:20.0% 80.0%"));
        assert!(html.contains("var(--cover) var(--cover-pos,center)/cover"));
        // 超出范围的值被限制在 0–100
        assert!(html.contains("src=\"photos/b.jpg\" alt=\"\" loading=\"lazy\" data-et-index=\"1\" style=\"object-position:100.0% 0.0%\""));
        // 拍立得：照片堆叠在 .stack 里，并带有滑动脚本
        p.template_id = "polaroid".into();
        let html = render_project(&p, "photos/", "media/");
        assert!(html.contains("<div class=\"stack\"><img src=\"photos/a.jpg\""));
        assert!(html.contains("pointerdown"));
    }

    #[test]
    fn date_parts_and_chinese_date() {
        let d = DateParts::parse("2026-12-24");
        assert_eq!(d.dot, "2026.12.24");
        assert_eq!(d.md, "12.24");
        assert_eq!(d.weekday, "星期四");
        assert_eq!(d.chinese, "二〇二六年十二月二十四日");
        assert_eq!(DateParts::parse("2027-03-10").chinese, "二〇二七年三月十日");
        assert_eq!(DateParts::parse("2027-10-30").chinese, "二〇二七年十月三十日");
        assert_eq!(DateParts::parse("2027-01-15").chinese, "二〇二七年一月十五日");
        assert!(DateParts::parse("").dot.is_empty());
    }

    #[test]
    fn no_music_no_button() {
        let html = render_project(&project(""), "photos/", "media/");
        assert!(!html.contains("et-music"));
    }
}
