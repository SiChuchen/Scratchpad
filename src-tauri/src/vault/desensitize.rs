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

use regex::Regex;

use crate::vault::models::{EntryKind, VaultEntry, VaultField};

#[derive(Debug, Clone)]
pub struct DesensitizedField {
    pub key: String,
    pub value: String, // 原文或 [SECRET:xxx]
    pub was_sensitive: bool,
}

#[derive(Debug, Clone)]
pub struct DesensitizedEntry {
    pub id: String,
    pub kind: EntryKind,
    pub title: String,
    pub notes: String,
    pub fields: Vec<DesensitizedField>,
    pub tags: Vec<String>,
}

fn build_regex_set() -> Vec<Regex> {
    vec![
        Regex::new(r"(?s)-----BEGIN [A-Z ]+PRIVATE KEY-----.*?-----END [A-Z ]+PRIVATE KEY-----").unwrap(),
        Regex::new(r"eyJ[A-Za-z0-9_-]{10,}\.eyJ[A-Za-z0-9_-]{10,}\.[A-Za-z0-9_-]{10,}").unwrap(),
        Regex::new(r"[a-zA-Z][a-zA-Z0-9+.-]*://[^:/@\s]+:[^:/@\s]+@").unwrap(),
        Regex::new(r"(?i)bearer\s+[A-Za-z0-9._-]{20,}").unwrap(),
        // 长 base64：≥ 56 字符（排除 40 字符以下的 SHA-1 hex 等常见短串）
        Regex::new(r"\b[A-Za-z0-9+/]{56,}={0,2}\b").unwrap(),
        Regex::new(r"\bAKIA[0-9A-Z]{16}\b").unwrap(),
        Regex::new(r"\b(?:\d[ -]*?){13,16}\b").unwrap(),
    ]
}

fn apply_regex_mask(text: &str, map: &mut TokenMap) -> String {
    let regexes = build_regex_set();
    let mut current = text.to_string();
    for re in &regexes {
        let mut replaced = String::new();
        let mut last_end = 0;
        for m in re.find_iter(&current) {
            replaced.push_str(&current[last_end..m.start()]);
            replaced.push_str(&map.tokenize(m.as_str()));
            last_end = m.end();
        }
        replaced.push_str(&current[last_end..]);
        current = replaced;
    }
    current
}

pub fn desensitize_entry(
    entry: &VaultEntry,
    fields: &[VaultField],
    tags: &[String],
    map: &mut TokenMap,
) -> DesensitizedEntry {
    let d_fields = fields
        .iter()
        .map(|f| {
            if f.is_sensitive {
                DesensitizedField {
                    key: f.key.clone(),
                    value: map.tokenize(&f.value),
                    was_sensitive: true,
                }
            } else {
                DesensitizedField {
                    key: f.key.clone(),
                    value: apply_regex_mask(&f.value, map),
                    was_sensitive: false,
                }
            }
        })
        .collect();

    let notes = apply_regex_mask(entry.notes.as_deref().unwrap_or(""), map);

    DesensitizedEntry {
        id: entry.id.clone(),
        kind: entry.kind,
        title: entry.title.clone(),
        notes,
        fields: d_fields,
        tags: tags.to_vec(),
    }
}

#[cfg(test)]
mod regex_tests {
    use super::*;

    fn mk_entry() -> VaultEntry {
        VaultEntry {
            id: "v1".into(),
            kind: EntryKind::Credential,
            title: "T".into(),
            notes: None,
            created_at: "t".into(),
            updated_at: "t".into(),
        }
    }

    #[test]
    fn desensitize_masks_sensitive_field() {
        let mut m = TokenMap::new();
        let mut e = mk_entry();
        e.title = "Prod".into();
        let fields = vec![VaultField {
            id: "f1".into(),
            entry_id: "v1".into(),
            key: "password".into(),
            value: "hunter2".into(),
            is_sensitive: true,
            sort_order: 0,
        }];
        let d = desensitize_entry(&e, &fields, &[], &mut m);
        assert_eq!(d.title, "Prod"); // title 不脱敏
        assert!(d.fields[0].value.starts_with("[SECRET:"));
        assert!(d.fields[0].was_sensitive);
    }

    #[test]
    fn desensitize_keeps_nonsensitive_field_as_plaintext_when_no_regex_hit() {
        let mut m = TokenMap::new();
        let e = mk_entry();
        let fields = vec![VaultField {
            id: "f1".into(),
            entry_id: "v1".into(),
            key: "user".into(),
            value: "admin".into(),
            is_sensitive: false,
            sort_order: 0,
        }];
        let d = desensitize_entry(&e, &fields, &[], &mut m);
        assert_eq!(d.fields[0].value, "admin");
    }

    #[test]
    fn regex_catches_pem_private_key() {
        let mut m = TokenMap::new();
        let pem = "-----BEGIN RSA PRIVATE KEY-----\nMIIEowIBAAKCAQEA...\n-----END RSA PRIVATE KEY-----";
        let masked = apply_regex_mask(pem, &mut m);
        assert!(masked.starts_with("[SECRET:"));
        assert!(!masked.contains("MIIEowIBAAKCAQEA"));
    }

    #[test]
    fn regex_catches_jwt() {
        let mut m = TokenMap::new();
        let jwt = "eyJhbGciOiJIUzI1NiJ9.eyJzdWIiOiIxMjM0NTY3ODkwIn0.signatureabcdef123456";
        let masked = apply_regex_mask(jwt, &mut m);
        assert!(masked.starts_with("[SECRET:"));
    }

    #[test]
    fn regex_catches_url_with_embedded_credentials() {
        let mut m = TokenMap::new();
        let url = "postgres://user:pass@host:5432/db";
        let masked = apply_regex_mask(url, &mut m);
        assert!(masked.contains("[SECRET:"));
        assert!(!masked.contains(":pass@"));
    }

    #[test]
    fn regex_skips_plain_email() {
        let mut m = TokenMap::new();
        let email = "user@example.com";
        let masked = apply_regex_mask(email, &mut m);
        assert_eq!(masked, email);
    }

    #[test]
    fn regex_skips_plain_ipv4() {
        let mut m = TokenMap::new();
        let ip = "10.0.0.1";
        let masked = apply_regex_mask(ip, &mut m);
        assert_eq!(masked, ip);
    }
}
