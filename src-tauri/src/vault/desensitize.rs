// src-tauri/src/vault/desensitize.rs
use std::collections::HashMap;

use rand::Rng;
use regex::Regex;

use crate::storage::error::StorageError;
use crate::vault::models::{EntryKind, VaultEntry, VaultField};

/// 请求级 token 映射：value <-> placeholder。
///
/// 旧实现使用进程级 salt + SHA-256 派生的语义化 token，会暴露相同敏感值
/// 的出现次数等侧信道。新实现每次请求都用独立的随机 16 字节 ID 作为占位符，
/// 关闭即销毁，不可逆推。
pub struct TokenMap {
    forward: HashMap<String, String>, // value -> token
    reverse: HashMap<String, String>, // token -> value
}

impl TokenMap {
    pub fn new() -> Self {
        Self {
            forward: HashMap::new(),
            reverse: HashMap::new(),
        }
    }

    pub fn tokenize(&mut self, value: &str) -> String {
        if let Some(t) = self.forward.get(value) {
            return t.clone();
        }
        // 16 字节随机 ID，hex 编码为 32 个字符，无语义、无 salt 前缀
        let token = format!(
            "[SECRET:{}]",
            hex::encode(rand::thread_rng().gen::<[u8; 16]>())
        );
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

    /// 严格回填：扫描全部 `[SECRET:xxx]` 占位符，任一不在当前 map 时
    /// 立即返回 `StorageError::Validation`，避免把模型编造的占位符当成
    /// 普通文本写回数据库。只有所有占位符都已知时才执行回填。
    pub fn detokenize_strict(&self, text: &str) -> Result<String, StorageError> {
        let placeholder_re = Regex::new(r"\[SECRET:[^\]]+\]").unwrap();
        for m in placeholder_re.find_iter(text) {
            if !self.reverse.contains_key(m.as_str()) {
                return Err(StorageError::Validation(format!(
                    "unknown placeholder: {}",
                    m.as_str()
                )));
            }
        }
        Ok(self.detokenize(text))
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

    #[test]
    fn raw_text_masks_common_secret_assignments_and_tokens() {
        for (sample, secret) in [
            ("password=hunter2", "hunter2"),
            ("passwd: hunter2", "hunter2"),
            ("pwd = hunter2", "hunter2"),
            ("secret: abcdef123456", "abcdef123456"),
            ("token=ghp_1234567890abcdefghijklmnop", "ghp_1234567890abcdefghijklmnop"),
            ("api_key: sk-abcdefghijklmnopqrstuvwxyz", "sk-abcdefghijklmnopqrstuvwxyz"),
            ("github_pat_11AA0abcdefghijklmnopqrstuvwxyz", "github_pat_11AA0abcdefghijklmnopqrstuvwxyz"),
        ] {
            let mut map = TokenMap::new();
            let masked = desensitize_raw_text(sample, &[], &mut map);
            assert!(masked.contains("[SECRET:"), "not masked: {sample}");
            assert!(!masked.contains(secret));
        }
    }

    #[test]
    fn strict_detokenize_rejects_unknown_placeholder() {
        let map = TokenMap::new();
        assert!(map.detokenize_strict("[SECRET:unknown]").is_err());
    }

    #[test]
    fn manual_sensitive_values_are_masked_before_regexes() {
        let mut map = TokenMap::new();
        let masked = desensitize_raw_text("internal codename orion", &["orion".into()], &mut map);
        assert!(!masked.contains("orion"));
    }
}

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

/// 构造用于 free-text 脱敏的正则集合。
///
/// 顺序非常重要：先把语义性更强、更长的 token 模式跑完（GitHub PAT、
/// `sk-` 前缀），再跑通用赋值 `password=...`，最后跑容易误伤的长 base64
/// /银行卡号等模式。每个正则只替换自身匹配到的片段，不会跨界。
fn build_regex_set() -> Vec<Regex> {
    vec![
        // GitHub PAT v2 — 必须在 classic token 之前，避免被 `ghp_` 截断匹配
        Regex::new(r"\bgithub_pat_[A-Za-z0-9_]{20,}\b").unwrap(),
        // GitHub classic tokens: ghp_ gho_ ghs_ ghr_ ghu_
        Regex::new(r"\bgh[pousr]_[A-Za-z0-9_]{20,}\b").unwrap(),
        // OpenAI / Stripe 风格 API key 前缀
        Regex::new(r"\bsk-[A-Za-z0-9_-]{20,}\b").unwrap(),
        // 赋值型敏感值：保留 key 和分隔符，只 token 化 value
        // 捕获组 1 = 完整的 "key op" 前缀（含分隔符），组 2 = value
        // value 不能以 `[SECRET:` 开头（已被前序正则脱敏过，避免嵌套）
        Regex::new(
            r"(?i)\b(password|passwd|pwd|secret|token|api[_-]?key)\s*[:=]\s*([^\s,;]+)",
        )
        .unwrap(),
        // PEM 私钥
        Regex::new(r"(?s)-----BEGIN [A-Z ]+PRIVATE KEY-----.*?-----END [A-Z ]+PRIVATE KEY-----")
            .unwrap(),
        // JWT
        Regex::new(r"eyJ[A-Za-z0-9_-]{10,}\.eyJ[A-Za-z0-9_-]{10,}\.[A-Za-z0-9_-]{10,}").unwrap(),
        // URL embedded credentials
        Regex::new(r"[a-zA-Z][a-zA-Z0-9+.-]*://[^:/@\s]+:[^:/@\s]+@").unwrap(),
        // Bearer tokens
        Regex::new(r"(?i)bearer\s+[A-Za-z0-9._-]{20,}").unwrap(),
        // 长 base64：≥ 56 字符（排除 40 字符以下的 SHA-1 hex 等常见短串）
        Regex::new(r"\b[A-Za-z0-9+/]{56,}={0,2}\b").unwrap(),
        // AWS access key id
        Regex::new(r"\bAKIA[0-9A-Z]{16}\b").unwrap(),
        // 银行卡号
        Regex::new(r"\b(?:\d[ -]*?){13,16}\b").unwrap(),
    ]
}

/// 按正则集合顺序脱敏。对带捕获组的赋值正则只 token 化第 2 组（value），
/// 其余正则整体替换。若 value 已经是 `[SECRET:xxx]` 占位符（被前序正则
/// 脱敏过）则跳过，避免占位符被嵌套二次脱敏。
fn apply_regex_mask(text: &str, map: &mut TokenMap) -> String {
    let regexes = build_regex_set();
    let mut current = text.to_string();
    for re in &regexes {
        let mut replaced = String::new();
        let mut last_end = 0;
        for caps in re.captures_iter(&current) {
            let whole = match caps.get(0) {
                Some(m) => m,
                None => continue,
            };
            replaced.push_str(&current[last_end..whole.start()]);
            if let (Some(prefix_m), Some(value_m)) = (caps.get(1), caps.get(2)) {
                if value_m.as_str().starts_with("[SECRET:") {
                    // value 已经是占位符：原样保留整段匹配
                    replaced.push_str(whole.as_str());
                } else {
                    // 赋值型：保留 prefix（key + 分隔符），仅 token 化 value
                    replaced.push_str(prefix_m.as_str());
                    replaced.push_str(&map.tokenize(value_m.as_str()));
                }
            } else {
                replaced.push_str(&map.tokenize(whole.as_str()));
            }
            last_end = whole.end();
        }
        replaced.push_str(&current[last_end..]);
        current = replaced;
    }
    current
}

/// 在自由文本上执行请求级脱敏。
///
/// 顺序：
/// 1. `manual_sensitive_values` 按长度降序、过滤空值后整体 token 化
///    （先长后短，避免短值破坏长值的后续正则匹配）。
/// 2. 执行 `build_regex_set` 中的正则脱敏。
pub fn desensitize_raw_text(
    text: &str,
    manual_sensitive_values: &[String],
    map: &mut TokenMap,
) -> String {
    // 1. 手动标记的敏感值：去空、按长度降序、整体替换
    let mut manual: Vec<&String> = manual_sensitive_values
        .iter()
        .filter(|s| !s.is_empty())
        .collect();
    manual.sort_by(|a, b| b.len().cmp(&a.len()));

    let mut current = text.to_string();
    for value in manual {
        if current.contains(value.as_str()) {
            let token = map.tokenize(value);
            current = current.replace(value.as_str(), &token);
        }
    }

    // 2. 正则脱敏
    apply_regex_mask(&current, map)
}

/// 校验 AI 返回的元数据（tags / summary / search_aliases）不包含任何
/// 敏感原文或占位符。**该函数绝不执行 detokenize**。
///
/// 规则：
/// - 每项 trim 后丢空
/// - 超过 `max_items` 数量的尾部被截断
/// - 单项长度超过 `max_len` 拒绝
/// - 包含 `[SECRET:` 拒绝
/// - 与本请求 `TokenMap` 中任一敏感原文相同（按 Unicode 小写比较）拒绝
/// - 按 Unicode 小写去重
pub fn validate_non_sensitive_metadata(
    values: &[String],
    token_map: &TokenMap,
    max_items: usize,
    max_len: usize,
) -> Result<Vec<String>, String> {
    // 提前收集敏感原文的小写集合，避免每次比较都遍历 map
    let sensitive_lower: Vec<String> = token_map
        .forward
        .keys()
        .map(|k| k.to_lowercase())
        .collect();

    let mut seen: HashMap<String, ()> = HashMap::new();
    let mut out: Vec<String> = Vec::new();

    for raw in values {
        let trimmed = raw.trim();
        if trimmed.is_empty() {
            continue;
        }
        if out.len() >= max_items {
            break;
        }
        if trimmed.len() > max_len {
            return Err(format!("value too long ({} > {}): {}", trimmed.len(), max_len, trimmed));
        }
        if trimmed.contains("[SECRET:") {
            return Err(format!("value contains placeholder: {}", trimmed));
        }
        let lower = trimmed.to_lowercase();
        if sensitive_lower.iter().any(|s| s == &lower) {
            return Err(format!("value matches sensitive original: {}", trimmed));
        }
        if seen.contains_key(&lower) {
            continue;
        }
        seen.insert(lower, ());
        out.push(trimmed.to_string());
    }

    Ok(out)
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

    #[test]
    fn strict_detokenize_accepts_known_and_restores() {
        let mut m = TokenMap::new();
        let t = m.tokenize("topsecret");
        let text = format!("pw is {t}");
        assert_eq!(m.detokenize_strict(&text).unwrap(), "pw is topsecret");
    }

    #[test]
    fn strict_detokenize_rejects_partial_unknown_among_known() {
        let mut m = TokenMap::new();
        let t = m.tokenize("known");
        let text = format!("{t} and [SECRET:fake]");
        assert!(m.detokenize_strict(&text).is_err());
    }

    #[test]
    fn validate_metadata_trims_dedupes_and_caps_count() {
        let mut m = TokenMap::new();
        let _ = m.tokenize("irrelevant"); // map 非空但不影响
        let values = vec![
            "  work  ".to_string(),
            "WORK".to_string(), // dedup with "work" under Unicode lowercase
            "".to_string(),
            "   ".to_string(),
            "home".to_string(),
            "extra".to_string(),
        ];
        let out = validate_non_sensitive_metadata(&values, &m, 2, 100).unwrap();
        assert_eq!(out, vec!["work".to_string(), "home".to_string()]);
    }

    #[test]
    fn validate_metadata_rejects_oversize_value() {
        let m = TokenMap::new();
        let long = "x".repeat(11);
        let err = validate_non_sensitive_metadata(&[long], &m, 10, 10);
        assert!(err.is_err());
    }

    #[test]
    fn validate_metadata_rejects_embedded_placeholder() {
        let m = TokenMap::new();
        let err = validate_non_sensitive_metadata(&["leaked [SECRET:abc] tag".to_string()], &m, 10, 100);
        assert!(err.is_err());
    }

    #[test]
    fn validate_metadata_rejects_sensitive_original() {
        let mut m = TokenMap::new();
        let _ = m.tokenize("hunter2");
        let err = validate_non_sensitive_metadata(&["HUNTER2".to_string()], &m, 10, 100);
        assert!(err.is_err());
    }

    #[test]
    fn validate_metadata_does_not_detokenize() {
        // 即使把占位符字符串作为输入喂进去，函数也应原样拒绝（含 [SECRET:）
        // 而不是把它 detokenize 出敏感原文
        let mut m = TokenMap::new();
        let t = m.tokenize("topsecret");
        let err = validate_non_sensitive_metadata(&[t], &m, 10, 100);
        assert!(err.is_err());
    }
}
