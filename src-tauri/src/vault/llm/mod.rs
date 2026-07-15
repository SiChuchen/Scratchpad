// src-tauri/src/vault/llm/mod.rs
pub mod openai_compat;
pub mod presets;
pub mod prompt;

use async_trait::async_trait;
use serde::{Deserialize, Serialize};
use thiserror::Error;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ChatMessage {
    pub role: String, // "system" | "user" | "assistant"
    pub content: String,
}

impl ChatMessage {
    pub fn system(content: impl Into<String>) -> Self {
        Self { role: "system".into(), content: content.into() }
    }
    pub fn user(content: impl Into<String>) -> Self {
        Self { role: "user".into(), content: content.into() }
    }
}

#[derive(Debug, Clone)]
pub struct LlmRequest {
    pub messages: Vec<ChatMessage>,
    pub json_mode: bool,
    pub temperature: f32,
    pub max_tokens: Option<u32>,
}

impl Default for LlmRequest {
    fn default() -> Self {
        Self {
            messages: vec![],
            json_mode: false,
            temperature: 0.3,
            max_tokens: None,
        }
    }
}

#[derive(Debug, Clone)]
pub struct LlmResponse {
    pub content: String,
    pub tokens_used: Option<u32>,
}

#[derive(Debug, Clone, Error)]
pub enum LlmError {
    #[error("timeout")]
    Timeout,
    #[error("authentication failed")]
    Auth,
    #[error("rate limited")]
    RateLimit,
    #[error("network error: {0}")]
    Network(String),
    #[error("server error ({0}): {1}")]
    Server(u16, String),
    #[error("parse error: {0}")]
    Parse(String),
    #[error("invalid config: {0}")]
    InvalidConfig(String),
}

#[async_trait]
pub trait LlmAdapter: Send + Sync {
    async fn complete(&self, req: LlmRequest) -> Result<LlmResponse, LlmError>;
}
