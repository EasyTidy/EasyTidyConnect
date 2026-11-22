//! 分享卡片：发布到 S3/R2/Cloudflare Pages 的公开页面会写入分享元数据，
//! 微信、QQ、微博、X 等抓取链接时据此显示「标题 + 描述 + 缩略图」卡片。

use std::path::Path;

use anyhow::{bail, Result};

use crate::anniversary::{escape_html, render_with_db};
use crate::db::Database;
use crate::models::{AnniversaryProject, PublishResult, PublishTarget, ShareSettings};

/// 没有照片时用来生成分享缩略图的应用图标（512×512 PNG）。
pub const DEFAULT_SHARE_IMAGE: &[u8] = include_bytes!("../assets/share-default.png");

/// 发布时生成的分享缩略图文件名。
pub const SHARE_THUMB_NAME: &str = "share.jpg";
/// 微信分享缩略图：正方形、≥300px、尽量小于 32 KB，否则卡片可能不显示图片。
const THUMB_SIZE: u32 = 300;
const THUMB_MAX_BYTES: usize = 32 * 1024;

/// 用第一张照片（没有照片或无法解码时用应用图标）生成分享缩略图：
/// 居中裁成正方形、缩放到 300×300、透明区域铺白底，编码为 JPEG 并压到 32 KB 以内。
pub fn share_thumbnail(project: &AnniversaryProject, photos_dir: &Path) -> Vec<u8> {
    let photo = project
        .photos
        .first()
        .and_then(|file| std::fs::read(photos_dir.join(file)).ok())
        .and_then(|bytes| image::load_from_memory(&bytes).ok());
    let source = photo
        .or_else(|| image::load_from_memory(DEFAULT_SHARE_IMAGE).ok())
        .unwrap_or_else(|| image::DynamicImage::new_rgb8(THUMB_SIZE, THUMB_SIZE));
    let focus = project
        .photos
        .first()
        .and_then(|file| project.photo_focus.get(file))
        .copied()
        .unwrap_or([50.0, 50.0]);
    encode_thumbnail(&source, focus)
}

fn encode_thumbnail(source: &image::DynamicImage, focus: [f32; 2]) -> Vec<u8> {
    let (w, h) = (source.width(), source.height());
    let side = w.min(h).max(1);
    // 与 CSS object-position 同样的含义：x% 表示把多余宽度的 x% 放在左边
    let offset = |extra: u32, percent: f32| ((extra as f32) * percent.clamp(0.0, 100.0) / 100.0).round() as u32;
    let square = source.crop_imm(offset(w - side, focus[0]), offset(h - side, focus[1]), side, side);
    let resized = square.resize_exact(THUMB_SIZE, THUMB_SIZE, image::imageops::FilterType::Triangle).to_rgba8();
    // 透明 PNG 在微信里会显示成黑底：铺白底后再转 RGB
    let mut rgb = image::RgbImage::new(THUMB_SIZE, THUMB_SIZE);
    for (x, y, pixel) in resized.enumerate_pixels() {
        let alpha = pixel[3] as u32;
        let blend = |c: u8| ((c as u32 * alpha + 255 * (255 - alpha)) / 255) as u8;
        rgb.put_pixel(x, y, image::Rgb([blend(pixel[0]), blend(pixel[1]), blend(pixel[2])]));
    }
    let mut best = Vec::new();
    for quality in [85u8, 75, 65, 55, 45, 35, 25] {
        let mut out = Vec::new();
        let encoder = image::codecs::jpeg::JpegEncoder::new_with_quality(&mut out, quality);
        if rgb.write_with_encoder(encoder).is_err() {
            continue;
        }
        best = out;
        if best.len() <= THUMB_MAX_BYTES {
            break;
        }
    }
    best
}

/// 只有这些发布目标会生成长期有效的公开链接，才允许分享。
pub fn is_shareable_kind(kind: &str) -> bool {
    matches!(kind, "s3" | "r2" | "cloudflare_pages")
}

#[derive(Debug, Clone, PartialEq)]
pub struct ShareMeta {
    pub url: String,
    pub title: String,
    pub description: String,
    pub image: String,
}

/// 页面公开地址与资源目录（以 / 结尾）。非静态目标返回 None。
pub fn public_location(target: &PublishTarget, slug: &str) -> Option<(String, String)> {
    match target.kind.as_str() {
        "s3" | "r2" => {
            let base_key = crate::publish::base_key(target, slug);
            Some((
                crate::publish::public_url(target, &format!("{base_key}/index.html")),
                crate::publish::public_url(target, &format!("{base_key}/")),
            ))
        }
        "cloudflare_pages" => {
            let url = format!("{}/{}/", pages_origin(target), crate::pages::base_path(target, slug));
            Some((url.clone(), url))
        }
        _ => None,
    }
}

/// Cloudflare Pages 的稳定访问地址：自定义域名 > 生产分支 `{项目}.pages.dev` > 预览分支别名。
/// 不使用每次部署的哈希地址，否则重新发布后旧链接（包括已写入的 NFC 标签）看不到新内容。
pub fn pages_origin(target: &PublishTarget) -> String {
    let custom = target.public_base_url.trim().trim_end_matches('/');
    if !custom.is_empty() {
        return custom.to_string();
    }
    let project = target.project_name.trim().to_ascii_lowercase();
    let branch = target.branch.trim();
    if branch.is_empty() || branch == "main" || branch == "master" {
        return format!("https://{project}.pages.dev");
    }
    // 预览分支别名：小写，非字母数字转为 -，最长 28 个字符。
    let alias: String = branch
        .to_ascii_lowercase()
        .chars()
        .map(|c| if c.is_ascii_alphanumeric() { c } else { '-' })
        .take(28)
        .collect::<String>()
        .trim_matches('-')
        .to_string();
    format!("https://{alias}.{project}.pages.dev")
}

/// 生成分享元数据：自定义文案优先，否则取标题与正文第一句；缩略图用第一张照片。
pub fn build_meta(
    project: &AnniversaryProject,
    settings: &ShareSettings,
    page_url: &str,
    asset_base: &str,
) -> ShareMeta {
    let title = [settings.title.trim(), project.title.trim(), project.name.trim()]
        .into_iter()
        .find(|value| !value.is_empty())
        .unwrap_or("送给你的一封信")
        .to_string();
    let fallback_desc = [project.opening.as_str(), project.body.as_str(), project.closing.as_str()]
        .iter()
        .flat_map(|text| text.lines())
        .map(str::trim)
        .find(|line| !line.is_empty())
        .unwrap_or("")
        .to_string();
    let description = if settings.description.trim().is_empty() {
        fallback_desc
    } else {
        settings.description.trim().to_string()
    };
    // 发布时生成的 300×300 JPEG（来自第一张照片或应用图标），满足微信缩略图的大小要求
    let image = format!("{asset_base}{SHARE_THUMB_NAME}");
    ShareMeta {
        url: page_url.to_string(),
        title: truncate(&title, 40),
        description: truncate(&description, 80),
        image,
    }
}

/// 把分享元数据写进渲染好的页面：替换 <title>，在 </head> 前插入 OG / Twitter / itemprop 标签，
/// 并在 <body> 开头放一张不可见的缩略图（老版本微信会取页面里第一张足够大的图片）。
/// `wechat_signer` 为签名服务地址时，再插入微信 JS-SDK 脚本，在微信里打开时设置分享卡片。
pub fn inject(html: &str, meta: &ShareMeta, wechat_signer: Option<&str>) -> String {
    let title = escape_html(&meta.title);
    let desc = escape_html(&meta.description);
    let url = escape_html(&meta.url);
    let image = escape_html(&meta.image);
    let tags = format!(
        concat!(
            "<meta name=\"description\" content=\"{desc}\">\n",
            "<meta property=\"og:type\" content=\"website\">\n",
            "<meta property=\"og:title\" content=\"{title}\">\n",
            "<meta property=\"og:description\" content=\"{desc}\">\n",
            "<meta property=\"og:image\" content=\"{image}\">\n",
            "<meta property=\"og:url\" content=\"{url}\">\n",
            "<meta property=\"og:site_name\" content=\"EasyTidy Connect\">\n",
            "<meta name=\"twitter:card\" content=\"summary_large_image\">\n",
            "<meta name=\"twitter:title\" content=\"{title}\">\n",
            "<meta name=\"twitter:description\" content=\"{desc}\">\n",
            "<meta name=\"twitter:image\" content=\"{image}\">\n",
            "<meta itemprop=\"name\" content=\"{title}\">\n",
            "<meta itemprop=\"description\" content=\"{desc}\">\n",
            "<meta itemprop=\"image\" content=\"{image}\">\n",
            "<link rel=\"canonical\" href=\"{url}\">\n",
        ),
        title = title,
        desc = desc,
        image = image,
        url = url,
    );

    let mut out = match (html.find("<title>"), html.find("</title>")) {
        (Some(start), Some(end)) if end > start => {
            format!("{}<title>{title}</title>{}", &html[..start], &html[end + "</title>".len()..])
        }
        _ => html.to_string(),
    };
    if let Some(pos) = out.find("</head>") {
        out.insert_str(pos, &tags);
    }
    if let Some(start) = out.find("<body") {
        if let Some(end) = out[start..].find('>') {
            let hidden = format!(
                "\n<div aria-hidden=\"true\" style=\"position:absolute;width:1px;height:1px;overflow:hidden;opacity:0;pointer-events:none\"><img src=\"{image}\" alt=\"\"></div>"
            );
            out.insert_str(start + end + 1, &hidden);
        }
    }
    if let Some(signer) = wechat_signer.map(str::trim).filter(|s| !s.is_empty()) {
        let script = crate::wechat::page_script(signer, &meta.title, &meta.description, &meta.url, &meta.image);
        match out.rfind("</body>") {
            Some(pos) => out.insert_str(pos, &script),
            None => out.push_str(&script),
        }
    }
    out
}

/// 发布到 S3/R2/Cloudflare Pages：渲染页面、写入分享卡片元数据、上传。
pub async fn publish_static(
    db: &Database,
    data_dir: &Path,
    project: &AnniversaryProject,
    target: &PublishTarget,
) -> Result<PublishResult> {
    // 使用已授权的 Cloudflare 账户时：自动创建 bucket / Pages 项目，补全账户与令牌
    let resolved = crate::resolve_cloudflare_target(db, target).await?;
    let target = &resolved;
    let Some((page_url, asset_base)) = public_location(target, &project.slug) else {
        bail!("发布目标「{}」不是静态托管（S3/R2/Cloudflare Pages）", target.name);
    };
    let project_dir = data_dir.join("anniversaries").join(project.id.to_string());
    let photos_dir = project_dir.join("photos");
    let media_dir = project_dir.join("media");

    let settings = db.get_share_settings(project.id)?.unwrap_or_default();
    let meta = build_meta(project, &settings, &page_url, &asset_base);
    let wechat = db.get_wechat_config().unwrap_or_default();
    let signer = (wechat.enabled && !wechat.signer_url.trim().is_empty()).then_some(wechat.signer_url.as_str());
    let html = inject(&render_with_db(db, project, "photos/", "media/"), &meta, signer);

    if target.kind == "cloudflare_pages" {
        crate::pages::publish_project(target, project, &html, &photos_dir, &media_dir).await
    } else if target.kind == "r2" && crate::cloudflare::uses_auth(target) {
        // R2 授权模式：走 Cloudflare REST API 上传，不需要 S3 Access Key
        let auth = crate::cloudflare_auth(db).await?;
        let api = crate::cloudflare::Api::new(&auth.access_token)?;
        crate::cloudflare::publish_r2(&api, &auth.account_id, target, project, &html, &photos_dir, &media_dir).await
    } else {
        crate::publish::publish_project(target, project, &html, &photos_dir, &media_dir).await
    }
}

fn truncate(value: &str, limit: usize) -> String {
    let mut chars = value.chars();
    let head: String = chars.by_ref().take(limit).collect();
    if chars.next().is_some() {
        format!("{head}…")
    } else {
        head
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn target(kind: &str) -> PublishTarget {
        PublishTarget {
            id: 1,
            name: "t".into(),
            kind: kind.into(),
            endpoint: "https://s3.example.com/".into(),
            region: "auto".into(),
            bucket: "love".into(),
            access_key: String::new(),
            secret_key: String::new(),
            prefix: "/anniversary/".into(),
            public_base_url: String::new(),
            cloudflared_path: String::new(),
            tunnel_token: String::new(),
            hostname: String::new(),
            account_id: String::new(),
            project_name: "MyCards".into(),
            api_token: String::new(),
            branch: "main".into(),
            tunnel_config: String::new(),
            is_default: true,
            created_at: String::new(),
        }
    }

    fn project() -> AnniversaryProject {
        AnniversaryProject {
            id: 1,
            slug: "ab12cd34".into(),
            name: "结婚三周年".into(),
            event_date: String::new(),
            title: "三年前的今天".into(),
            recipient: String::new(),
            opening: "\n亲爱的小雨：".into(),
            body: String::new(),
            closing: String::new(),
            template_id: "minimal".into(),
            theme: "#2563eb".into(),
            photos: vec!["p1.jpg".into()],
            music: String::new(),
            video: String::new(),
            custom_css: String::new(),
            photo_focus: Default::default(),
            slideshow: false,
            created_at: String::new(),
            updated_at: String::new(),
        }
    }

    #[test]
    fn share_thumbnail_is_small_square_jpeg() {
        // 没有照片：用应用图标（透明 PNG）生成，透明处铺白
        let thumb = share_thumbnail(&project(), std::path::Path::new("does-not-exist"));
        assert!(thumb.starts_with(&[0xFF, 0xD8]), "JPEG");
        assert!(thumb.len() <= THUMB_MAX_BYTES, "{} bytes", thumb.len());
        let img = image::load_from_memory(&thumb).unwrap();
        assert_eq!((img.width(), img.height()), (300, 300));
        let corner = img.to_rgb8().get_pixel(0, 0).0;
        assert!(corner.iter().all(|c| *c > 240), "transparent corner should be white: {corner:?}");

        // 有照片：居中裁成正方形
        let dir = std::env::temp_dir().join(format!("connect-thumb-{}", uuid::Uuid::new_v4()));
        std::fs::create_dir_all(&dir).unwrap();
        let mut noisy = image::RgbImage::new(1600, 900);
        for (x, y, p) in noisy.enumerate_pixels_mut() {
            *p = image::Rgb([(x * 7 % 256) as u8, (y * 13 % 256) as u8, ((x ^ y) % 256) as u8]);
        }
        noisy.save(dir.join("p1.jpg")).unwrap();
        let thumb = share_thumbnail(&project(), &dir);
        assert!(thumb.len() <= THUMB_MAX_BYTES, "{} bytes", thumb.len());
        let img = image::load_from_memory(&thumb).unwrap();
        assert_eq!((img.width(), img.height()), (300, 300));
        let _ = std::fs::remove_dir_all(dir);
    }

    #[test]
    fn only_static_targets_are_shareable() {
        assert!(is_shareable_kind("s3") && is_shareable_kind("r2") && is_shareable_kind("cloudflare_pages"));
        assert!(!is_shareable_kind("lan_tunnel") && !is_shareable_kind("cloudflare_tunnel"));
        assert!(public_location(&target("lan_tunnel"), "x").is_none());
    }

    #[test]
    fn public_locations() {
        let (page, assets) = public_location(&target("s3"), "ab12cd34").unwrap();
        assert_eq!(page, "https://s3.example.com/love/anniversary/ab12cd34/index.html");
        assert_eq!(assets, "https://s3.example.com/love/anniversary/ab12cd34/");

        let (page, _) = public_location(&target("cloudflare_pages"), "ab12cd34").unwrap();
        assert_eq!(page, "https://mycards.pages.dev/anniversary/ab12cd34/");

        let mut preview = target("cloudflare_pages");
        preview.branch = "Feature/Spring_2026".into();
        assert_eq!(pages_origin(&preview), "https://feature-spring-2026.mycards.pages.dev");

        let mut custom = target("cloudflare_pages");
        custom.public_base_url = "https://love.example.com/".into();
        assert_eq!(pages_origin(&custom), "https://love.example.com");
    }

    #[test]
    fn meta_falls_back_and_injects() {
        let p = project();
        let meta = build_meta(&p, &ShareSettings::default(), "https://x.test/a/", "https://x.test/a/");
        assert_eq!(meta.title, "三年前的今天");
        assert_eq!(meta.description, "亲爱的小雨：");
        assert_eq!(meta.image, "https://x.test/a/share.jpg");

        let custom = ShareSettings { title: "给小雨的信 <3".into(), description: "点开看看".into() };
        let mut no_photo = p.clone();
        no_photo.photos.clear();
        let meta = build_meta(&no_photo, &custom, "https://x.test/a/", "https://x.test/a/");
        assert_eq!(meta.image, "https://x.test/a/share.jpg");

        let html = inject("<html><head><title>旧标题</title></head><body class=\"x\"><p>hi</p></body></html>", &meta, None);
        assert!(!html.contains("jweixin"));
        let with_wx = inject("<html><head></head><body><p>hi</p></body></html>", &meta, Some("https://sign.test/wx/jssdk"));
        assert!(with_wx.contains("jweixin-1.6.0.js"));
        assert!(with_wx.find("jweixin").unwrap() < with_wx.find("</body>").unwrap());
        assert!(html.contains("<title>给小雨的信 &lt;3</title>"));
        assert!(!html.contains("旧标题"));
        assert!(html.contains("<meta property=\"og:image\" content=\"https://x.test/a/share.jpg\">"));
        assert!(html.contains("<meta itemprop=\"description\" content=\"点开看看\">"));
        assert!(html.find("og:title").unwrap() < html.find("</head>").unwrap());
        assert!(html.find("<img src=\"https://x.test/a/share.jpg\"").unwrap() > html.find("<body").unwrap());
    }
}
