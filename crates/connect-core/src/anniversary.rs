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

pub fn render_project(project: &AnniversaryProject, photo_base: &str) -> String {
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

    template
        .replace("{{theme}}", &theme)
        .replace("{{title}}", &escape_html(&project.title))
        .replace("{{recipient}}", &escape_html(&project.recipient))
        .replace("{{date}}", &escape_html(&project.event_date))
        .replace("{{opening}}", &escape_html(&project.opening))
        .replace("{{body}}", &body)
        .replace("{{closing}}", &escape_html(&project.closing))
        .replace("{{photos}}", &photos)
        .replace("{{custom_css}}", &project.custom_css)
}

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
