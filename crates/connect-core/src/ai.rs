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
    let content = match provider.kind.as_str() {
        "anthropic" => call_anthropic(&client, provider, &prompt).await?,
        "gemini" => call_gemini(&client, provider, &prompt).await?,
        _ => call_openai_compatible(&client, provider, &prompt).await?,
    };

    parse_copy(&content)
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
            "max_tokens": 1200,
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

async fn call_anthropic(client: &Client, provider: &AiProvider, prompt: &str) -> Result<String> {
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
            "max_tokens": 1200,
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

async fn call_gemini(client: &Client, provider: &AiProvider, prompt: &str) -> Result<String> {
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
            "generationConfig": {"temperature": 0.8, "maxOutputTokens": 1200}
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

fn truncate(value: &str, limit: usize) -> String {
    value.chars().take(limit).collect()
}
