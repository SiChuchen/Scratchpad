// src-tauri/src/vault/desensitize.rs
use std::collections::HashMap;

use rand::Rng;
use sha2::{Digest, Sha256};

/// 会话级 token 映射：value <-> token
/// salt 在每次应用启动时随机生成，进程生命周期内固定，关闭即销毁
pub struct TokenMap {
    salt: String,
    forward: HashMap<String, String>, // value -> token
    reverse: HashMap<String, String>, // token -> value
}

impl TokenMap {
    pub fn new() -> Self {
        let salt: u64 = rand::thread_rng().gen();
        Self {
            salt: salt.to_string(),
            forward: HashMap::new(),
            reverse: HashMap::new(),
        }
    }

    pub fn tokenize(&mut self, value: &str) -> String {
        if let Some(t) = self.forward.get(value) {
            return t.clone();
        }
        let mut h = Sha256::new();
        h.update(value.as_bytes());
        h.update(self.salt.as_bytes());
        let digest = h.finalize();
        let token = format!("[SECRET:{}]", hex::encode(digest));
        self.forward.insert(value.to_string(), token.clone());
        self.reverse.insert(token.clone(), value.to_string());
        token
    }

    pub fn detokenize(&self, text: &str) -> String {
        let mut out = text.to_string();
        for (token, value) in &self.reverse {
            out = out.replace(token, value);
        }
        out
    }
}

impl Default for TokenMap {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn tokenize_stable_within_session() {
        let mut m = TokenMap::new();
        let t1 = m.tokenize("hunter2");
        let t2 = m.tokenize("hunter2");
        assert_eq!(t1, t2);
        assert!(t1.starts_with("[SECRET:"));
    }

    #[test]
    fn tokenize_different_values_different_tokens() {
        let mut m = TokenMap::new();
        assert_ne!(m.tokenize("a"), m.tokenize("b"));
    }

    #[test]
    fn tokenize_different_across_sessions() {
        let mut m1 = TokenMap::new();
        let mut m2 = TokenMap::new();
        assert_ne!(m1.tokenize("x"), m2.tokenize("x"));
    }

    #[test]
    fn detokenize_restores_original() {
        let mut m = TokenMap::new();
        let t = m.tokenize("s3cr3t");
        let text = format!("the password is {t}");
        assert_eq!(m.detokenize(&text), "the password is s3cr3t");
    }

    #[test]
    fn detokenize_handles_multiple_tokens() {
        let mut m = TokenMap::new();
        let t1 = m.tokenize("alpha");
        let t2 = m.tokenize("beta");
        let text = format!("{t1} and {t2}");
        assert_eq!(m.detokenize(&text), "alpha and beta");
    }
}
