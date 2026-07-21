// src-tauri/src/vault/llm/openai_compat.rs
use std::time::Duration;

use async_trait::async_trait;
use reqwest::Client;
use serde_json::{json, Value};

use super::{LlmAdapter, LlmError, LlmRequest, LlmResponse};

pub struct OpenAiCompatAdapter {
    base_url: String,
    api_key: String,
    model: String,
    client: Client,
}

impl OpenAiCompatAdapter {
    pub fn new(base_url: String, api_key: String, model: String) -> Result<Self, LlmError> {
        let client = Client::builder()
            .timeout(Duration::from_secs(30))
            .build()
            .map_err(|e| LlmError::Network(e.to_string()))?;
        Ok(Self {
            base_url,
            api_key,
            model,
            client,
        })
    }
}

fn request_body(model: &str, req: &LlmRequest) -> Value {
    let mut body = json!({
        "model": model,
        "messages": req.messages.iter().map(|m| json!({"role": m.role, "content": m.content})).collect::<Vec<_>>(),
        "temperature": req.temperature,
    });
    if req.json_mode {
        body["response_format"] = json!({"type": "json_object"});
    }
    if let Some(m) = req.max_tokens {
        body["max_tokens"] = json!(m);
    }
    if let Some(enabled) = req.thinking_enabled {
        body["thinking"] = json!({"type": if enabled { "enabled" } else { "disabled" }});
    }
    body
}

#[async_trait]
impl LlmAdapter for OpenAiCompatAdapter {
    async fn complete(&self, req: LlmRequest) -> Result<LlmResponse, LlmError> {
        let url = format!("{}/chat/completions", self.base_url.trim_end_matches('/'));
        let body = request_body(&self.model, &req);

        let resp = self
            .client
            .post(&url)
            .bearer_auth(&self.api_key)
            .json(&body)
            .send()
            .await
            .map_err(|e| {
                if e.is_timeout() {
                    LlmError::Timeout
                } else {
                    LlmError::Network(e.to_string())
                }
            })?;

        let status = resp.status();
        if !status.is_success() {
            let text = resp.text().await.unwrap_or_default();
            return Err(match status.as_u16() {
                401 | 403 => LlmError::Auth,
                429 => LlmError::RateLimit,
                s @ 500..=599 => LlmError::Server(s, text),
                s => LlmError::Server(s, text),
            });
        }

        let v: Value = resp
            .json()
            .await
            .map_err(|e| LlmError::Parse(e.to_string()))?;
        let finish_reason = v["choices"][0]["finish_reason"]
            .as_str()
            .map(ToString::to_string);
        if finish_reason.as_deref() == Some("length") {
            return Err(LlmError::Truncated);
        }
        let content = v["choices"][0]["message"]["content"]
            .as_str()
            .ok_or_else(|| LlmError::Parse("missing choices[0].message.content".into()))?
            .to_string();
        if content.trim().is_empty() {
            return Err(LlmError::Parse("empty choices[0].message.content".into()));
        }
        let tokens_used = v["usage"]["total_tokens"].as_u64().map(|n| n as u32);
        Ok(LlmResponse {
            content,
            tokens_used,
            finish_reason,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn request_body_omits_thinking_when_not_requested() {
        let body = request_body("model", &LlmRequest::default());
        assert!(body.get("thinking").is_none());
    }

    #[test]
    fn request_body_can_disable_thinking_for_supported_provider() {
        let request = LlmRequest {
            thinking_enabled: Some(false),
            ..Default::default()
        };
        let body = request_body("deepseek-v4-flash", &request);
        assert_eq!(body["thinking"]["type"], "disabled");
    }

    #[test]
    fn request_body_can_enable_thinking_when_user_requests_it() {
        let request = LlmRequest {
            thinking_enabled: Some(true),
            ..Default::default()
        };
        let body = request_body("deepseek-v4-flash", &request);
        assert_eq!(body["thinking"]["type"], "enabled");
    }
}
