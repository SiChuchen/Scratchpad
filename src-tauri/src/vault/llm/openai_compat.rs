// src-tauri/src/vault/llm/openai_compat.rs
use std::time::Duration;

use async_trait::async_trait;
use reqwest::Client;
use serde_json::{json, Value};

use super::{ChatMessage, LlmAdapter, LlmError, LlmRequest, LlmResponse};

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
        Ok(Self { base_url, api_key, model, client })
    }
}

#[async_trait]
impl LlmAdapter for OpenAiCompatAdapter {
    async fn complete(&self, req: LlmRequest) -> Result<LlmResponse, LlmError> {
        let url = format!(
            "{}/chat/completions",
            self.base_url.trim_end_matches('/')
        );
        let mut body = json!({
            "model": self.model,
            "messages": req.messages.iter().map(|m| json!({"role": m.role, "content": m.content})).collect::<Vec<_>>(),
            "temperature": req.temperature,
        });
        if req.json_mode {
            body["response_format"] = json!({"type": "json_object"});
        }
        if let Some(m) = req.max_tokens {
            body["max_tokens"] = json!(m);
        }

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

        let v: Value = resp.json().await.map_err(|e| LlmError::Parse(e.to_string()))?;
        let content = v["choices"][0]["message"]["content"]
            .as_str()
            .ok_or_else(|| LlmError::Parse("missing choices[0].message.content".into()))?
            .to_string();
        let tokens_used = v["usage"]["total_tokens"].as_u64().map(|n| n as u32);
        Ok(LlmResponse { content, tokens_used })
    }
}
