use crate::models::AnniversaryProject;

const MINIMAL: &str = include_str!("../templates/minimal.html");
const POLAROID: &str = include_str!("../templates/polaroid.html");
const LETTER: &str = include_str!("../templates/letter.html");
const TIMELINE: &str = include_str!("../templates/timeline.html");
const STARRY: &str = include_str!("../templates/starry.html");

pub const TEMPLATES: [(&str, &str); 5] = [
    ("minimal", "极简"),
    ("polaroid", "拍立得"),
    ("letter", "信纸"),
    ("timeline", "时间轴"),
    ("starry", "星空"),
];

pub fn render_project(
    project: &AnniversaryProject,
    photo_base: &str,
    media_base: &str,
) -> String {
    let template = match project.template_id.as_str() {
        "polaroid" => POLAROID,
        "letter" => LETTER,
        "timeline" => TIMELINE,
        "starry" => STARRY,
        _ => MINIMAL,
    };

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

    let photos = project
        .photos
        .iter()
        .map(|file| {
            format!(
                "<img src=\"{}{}\" alt=\"\" loading=\"lazy\">",
                photo_base,
                escape_html(file)
            )
        })
        .collect::<Vec<_>>()
        .join("\n");

    let media = render_media(project, media_base);
    let custom_css = format!("{}{}", project.custom_css, MEDIA_CSS);

    template
        .replace("{{theme}}", &theme)
        .replace("{{title}}", &escape_html(&project.title))
        .replace("{{recipient}}", &escape_html(&project.recipient))
        .replace("{{date}}", &escape_html(&project.event_date))
        .replace("{{opening}}", &escape_html(&project.opening))
        .replace("{{body}}", &body)
        .replace("{{closing}}", &escape_html(&project.closing))
        .replace("{{photos}}", &photos)
        .replace("{{media}}", &media)
        .replace("{{custom_css}}", &custom_css)
}

fn render_media(project: &AnniversaryProject, media_base: &str) -> String {
    let music = media_url(&project.music, media_base);
    let video = media_url(&project.video, media_base);
    let mut html = String::new();
    if !music.is_empty() {
        html.push_str(&format!(
            "<div class=\"et-media\"><div class=\"et-media-label\">音乐</div><audio controls loop preload=\"metadata\" src=\"{}\"></audio></div>",
            escape_html(&music)
        ));
    }
    if !video.is_empty() {
        html.push_str(&format!(
            "<div class=\"et-media\"><div class=\"et-media-label\">影像</div><video controls playsinline preload=\"metadata\" src=\"{}\"></video></div>",
            escape_html(&video)
        ));
    }
    html
}

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

const MEDIA_CSS: &str = r#"
.et-media{margin:26px auto;max-width:560px;background:rgba(255,255,255,.72);border:1px solid rgba(0,0,0,.08);border-radius:16px;padding:14px 16px;backdrop-filter:blur(10px)}
.et-media-label{font-size:11px;letter-spacing:.18em;color:var(--accent);margin-bottom:8px;text-transform:uppercase}
.et-media audio{width:100%;height:38px}
.et-media video{width:100%;max-height:420px;border-radius:10px;display:block;background:#000}
"#;

fn is_hex_color(value: &str) -> bool {
    let value = value.trim();
    value.len() == 7
        && value.starts_with('#')
        && value[1..].chars().all(|ch| ch.is_ascii_hexdigit())
}

pub fn escape_html(value: &str) -> String {
    value
        .replace('&', "&amp;")
        .replace('<', "&lt;")
        .replace('>', "&gt;")
        .replace('"', "&quot;")
        .replace('\'', "&#39;")
}
