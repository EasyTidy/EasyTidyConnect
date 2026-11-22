use anyhow::{bail, Context, Result};
use reqwest::Client;
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use std::time::Duration;

use crate::models::{AiProvider, GeneratedCopy};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AiGenerateInput {
    pub relationship: String,
    pub tone: String,
    pub keywords: String,
    pub language: String,
    pub length: String,
}

pub async fn generate_copy(
    provider: &AiProvider,
    input: &AiGenerateInput,
) -> Result<GeneratedCopy> {
    let client = Client::builder()
        .timeout(Duration::from_secs(90))
        .build()?;

    let prompt = build_prompt(input);
    let content = complete(&client, provider, &prompt, 1200).await?;
    parse_copy(&content)
}

/// 按 provider 类型发送单轮对话，返回模型输出的文本。
async fn complete(
    client: &Client,
    provider: &AiProvider,
    prompt: &str,
    max_tokens: u32,
) -> Result<String> {
    match provider.kind.as_str() {
        "anthropic" => call_anthropic(client, provider, prompt, max_tokens).await,
        "gemini" => call_gemini(client, provider, prompt, max_tokens).await,
        _ => call_openai_compatible(client, provider, prompt, max_tokens).await,
    }
}

fn build_prompt(input: &AiGenerateInput) -> String {
    format!(
        "你是一名温柔克制的中文文案作者。请为纪念日网页生成结构化文案。\n\
         关系：{}\n语气：{}\n关键词：{}\n语言：{}\n长度：{}\n\n\
         只输出 JSON，不要 Markdown 代码块，不要解释。JSON 字段：\n\
         {{\"title\":\"标题\",\"opening\":\"开场 1-2 句\",\"body\":\"正文，2-4 段，用\\n分隔\",\"closing\":\"结尾 1-2 句\"}}\n\
         不要编造具体日期、地点或人物经历；不要输出 HTML。",
        input.relationship, input.tone, input.keywords, input.language, input.length
    )
}

async fn call_openai_compatible(
    client: &Client,
    provider: &AiProvider,
    prompt: &str,
    max_tokens: u32,
) -> Result<String> {
    let base = provider
        .base_url
        .trim()
        .trim_end_matches('/')
        .to_string();
    let url = if base.ends_with("/chat/completions") {
        base
    } else if base.ends_with("/v1") {
        format!("{base}/chat/completions")
    } else {
        format!("{base}/v1/chat/completions")
    };

    let response = client
        .post(url)
        .bearer_auth(&provider.api_key)
        .json(&json!({
            "model": provider.model,
            "temperature": 0.8,
            "max_tokens": max_tokens,
            "messages": [
                {"role": "system", "content": "You output strict JSON only."},
                {"role": "user", "content": prompt}
            ]
        }))
        .send()
        .await
        .context("OpenAI-compatible request failed")?;

    let status = response.status();
    let body = response.text().await.unwrap_or_default();
    if !status.is_success() {
        bail!("AI HTTP {status}: {}", truncate(&body, 500));
    }

    let value: Value = serde_json::from_str(&body).context("invalid AI JSON response")?;
    value
        .pointer("/choices/0/message/content")
        .and_then(|v| v.as_str())
        .map(|v| v.to_string())
        .context("AI response missing choices[0].message.content")
}

async fn call_anthropic(
    client: &Client,
    provider: &AiProvider,
    prompt: &str,
    max_tokens: u32,
) -> Result<String> {
    let base = provider.base_url.trim().trim_end_matches('/');
    let url = if base.ends_with("/v1/messages") {
        base.to_string()
    } else {
        format!("{base}/v1/messages")
    };

    let response = client
        .post(url)
        .header("x-api-key", &provider.api_key)
        .header("anthropic-version", "2023-06-01")
        .json(&json!({
            "model": provider.model,
            "max_tokens": max_tokens,
            "temperature": 0.8,
            "messages": [{"role": "user", "content": prompt}]
        }))
        .send()
        .await
        .context("Anthropic request failed")?;

    let status = response.status();
    let body = response.text().await.unwrap_or_default();
    if !status.is_success() {
        bail!("AI HTTP {status}: {}", truncate(&body, 500));
    }

    let value: Value = serde_json::from_str(&body).context("invalid Anthropic response")?;
    let text = value
        .pointer("/content/0/text")
        .and_then(|v| v.as_str())
        .context("Anthropic response missing content[0].text")?;
    Ok(text.to_string())
}

async fn call_gemini(
    client: &Client,
    provider: &AiProvider,
    prompt: &str,
    max_tokens: u32,
) -> Result<String> {
    let base = provider
        .base_url
        .trim()
        .trim_end_matches('/')
        .to_string();
    let base = if base.is_empty() {
        "https://generativelanguage.googleapis.com/v1beta".to_string()
    } else {
        base
    };
    let url = format!(
        "{base}/models/{}:generateContent?key={}",
        provider.model, provider.api_key
    );

    let response = client
        .post(url)
        .json(&json!({
            "contents": [{"parts": [{"text": prompt}]}],
            "generationConfig": {"temperature": 0.8, "maxOutputTokens": max_tokens}
        }))
        .send()
        .await
        .context("Gemini request failed")?;

    let status = response.status();
    let body = response.text().await.unwrap_or_default();
    if !status.is_success() {
        bail!("AI HTTP {status}: {}", truncate(&body, 500));
    }

    let value: Value = serde_json::from_str(&body).context("invalid Gemini response")?;
    let text = value
        .pointer("/candidates/0/content/parts/0/text")
        .and_then(|v| v.as_str())
        .context("Gemini response missing candidates[0].content.parts[0].text")?;
    Ok(text.to_string())
}

fn parse_copy(raw: &str) -> Result<GeneratedCopy> {
    let cleaned = raw
        .trim()
        .trim_start_matches("```json")
        .trim_start_matches("```")
        .trim_end_matches("```")
        .trim();

    if let Ok(copy) = serde_json::from_str::<GeneratedCopy>(cleaned) {
        return Ok(copy);
    }

    // 容错：提取第一个 JSON 对象。
    if let (Some(start), Some(end)) = (cleaned.find('{'), cleaned.rfind('}')) {
        if end > start {
            if let Ok(copy) = serde_json::from_str::<GeneratedCopy>(&cleaned[start..=end]) {
                return Ok(copy);
            }
        }
    }

    bail!("AI 返回内容不是有效 JSON：{}", truncate(raw, 300))
}

/// AI 设计出的模板（尚未入库）。
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct GeneratedTemplate {
    #[serde(default)]
    pub name: String,
    #[serde(default)]
    pub dark: bool,
    pub css: String,
}

/// 让 AI 为固定的 HTML 骨架设计 CSS。`base_css` 不为空时表示在现有设计上按要求修改。
pub async fn generate_template(
    provider: &AiProvider,
    request: &str,
    theme: &str,
    base_css: Option<&str>,
) -> Result<GeneratedTemplate> {
    if request.trim().is_empty() {
        bail!("请描述想要的模板风格");
    }
    let client = Client::builder()
        .timeout(Duration::from_secs(180))
        .build()?;
    let prompt = build_template_prompt(request, theme, base_css);
    let content = complete(&client, provider, &prompt, 6000).await?;
    let mut template = parse_template(&content)?;
    template.css = crate::anniversary::sanitize_css(&template.css);
    if template.css.trim().is_empty() {
        bail!("AI 没有返回可用的 CSS");
    }
    if template.name.trim().is_empty() {
        template.name = truncate(request.trim(), 8);
    }
    template.name = truncate(template.name.trim(), 16);
    Ok(template)
}

fn build_template_prompt(request: &str, theme: &str, base_css: Option<&str>) -> String {
    let task = match base_css {
        Some(css) if !css.trim().is_empty() => format!(
            "下面是当前模板的 CSS。请在它的基础上按用户要求修改，输出修改后的完整 CSS。\n\
             用户的修改要求：{request}\n\n当前 CSS：\n{css}"
        ),
        _ => format!("用户想要的风格：{request}"),
    };
    format!(
        "你是一名资深网页视觉设计师，为「纪念日贺卡」网页设计模板。页面在手机上打开（NFC 碰一下打开），也可能在电脑上看。\n\
         页面 HTML 骨架固定，你只负责写 CSS，用类名定位元素：\n\
         {skeleton}\n\n\
         说明：\n\
         - .et-body 内是若干 <p>；.et-gallery 内是 0-9 张 <img>（没有照片时为空）；.et-media 是可选的视频（<figure> 内一个 <video>，已有圆角与阴影，可调整外边距或圆角，不要再给它加外框/背景卡片）。\n\
         - .et-deco-1/.et-deco-2/.et-deco-3 是绝对定位的空装饰层，可用渐变、伪元素、SVG data URI 画装饰，不需要时 display:none。\n\
         - 已有基础样式：box-sizing、.et-page 最小高度 100vh、.et-sheet 居中最大宽 720px、.et-gallery 两列瀑布流（照片保持原比例，横竖都完整显示）；你可以覆盖它们，但不要给相册照片设置固定宽高比裁剪。\n\
         - 主题色用 var(--accent)（当前为 {theme}），让用户换色时整体协调。\n\
         - 右上角有一个悬浮音乐按钮，请避免在右上角 60px 内放重要内容。\n\n\
         设计要求：\n\
         1. 移动端优先（375px 宽好看），≥768px 时也要大气；正文 15-17px，行高 1.8 以上，文字与背景对比度足够。\n\
         2. 可以用 @keyframes 做克制的入场或氛围动画，并用 @media (prefers-reduced-motion: reduce) 关闭。\n\
         3. 禁止：@import、外部字体、http(s) 图片或任何外部 url()、JavaScript、修改 .et-music 相关样式。字体只用系统字体栈，中文优先 \"PingFang SC\",\"Noto Serif SC\",\"Songti SC\",\"Microsoft YaHei\" 等。\n\
         4. 页面整体是深色背景时 dark 设为 true。\n\n\
         {task}\n\n\
         只输出一个 JSON 对象，不要 Markdown 代码块、不要解释：\n\
         {{\"name\":\"模板名，不超过 8 个字\",\"dark\":false,\"css\":\"完整 CSS，换行用 \\n\"}}",
        skeleton = crate::anniversary::CUSTOM_SKELETON_OUTLINE,
    )
}

fn parse_template(raw: &str) -> Result<GeneratedTemplate> {
    let cleaned = raw
        .trim()
        .trim_start_matches("```json")
        .trim_start_matches("```")
        .trim_end_matches("```")
        .trim();
    if let Ok(template) = serde_json::from_str::<GeneratedTemplate>(cleaned) {
        return Ok(template);
    }
    if let (Some(start), Some(end)) = (cleaned.find('{'), cleaned.rfind('}')) {
        if end > start {
            if let Ok(template) = serde_json::from_str::<GeneratedTemplate>(&cleaned[start..=end]) {
                return Ok(template);
            }
        }
    }
    // 容错：模型直接输出了 ```css 代码块。
    if let Some(start) = raw.find("```css") {
        let rest = &raw[start + 6..];
        let css = rest.split("```").next().unwrap_or_default().trim();
        if !css.is_empty() {
            return Ok(GeneratedTemplate {
                name: String::new(),
                dark: false,
                css: css.to_string(),
            });
        }
    }
    bail!("AI 返回内容不是有效的模板 JSON：{}", truncate(raw, 300))
}

fn truncate(value: &str, limit: usize) -> String {
    value.chars().take(limit).collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_template_json_and_fallbacks() {
        let json = r#"```json
{"name":"胶片","dark":true,"css":".et-title{color:red}\n.et-page{background:#000}"}
```"#;
        let t = parse_template(json).unwrap();
        assert_eq!(t.name, "胶片");
        assert!(t.dark);
        assert!(t.css.contains(".et-page{background:#000}"));

        let wrapped = "好的，下面是模板：{\"name\":\"樱花\",\"css\":\"body{color:pink}\"} 希望喜欢";
        let t = parse_template(wrapped).unwrap();
        assert_eq!(t.name, "樱花");
        assert!(!t.dark);

        let css_block = "这里是 CSS：\n```css\n.et-title{font-size:30px}\n```";
        assert_eq!(parse_template(css_block).unwrap().css, ".et-title{font-size:30px}");

        assert!(parse_template("抱歉，我无法完成").is_err());
    }
}
