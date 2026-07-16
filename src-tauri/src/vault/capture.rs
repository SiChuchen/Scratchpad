// src-tauri/src/vault/capture.rs
// 本地录入解析：把粘贴的文本按固定优先级转成 `CaptureDraft`。
//
// 优先级固定为：
//   1. 连接 URL（postgres://, mysql:// 等）
//   2. SSH（ssh user@host [-p N]）
//   3. user:pass@host:port（无 scheme）
//   4. 多行 key=value / key:value
//   5. 单 URL（bookmark）
//   6. 普通笔记（兜底）
//
// 除空输入返回 Err 外，其它输入都能产生可保存的 draft。

use crate::vault::models::{CaptureDraft, EntryKind};

/// 识别为连接 URL 的 scheme（小写）。
const CONNECTION_SCHEMES: &[&str] = &[
    "postgres://",
    "postgresql://",
    "mysql://",
    "mariadb://",
    "mongodb://",
    "mongodb+srv://",
    "redis://",
    "amqp://",
    "ftp://",
    "sftp://",
];

/// bookmark 候选 scheme。
const BOOKMARK_SCHEMES: &[&str] = &["http://", "https://", "ftp://", "file://"];

/// 多行 KV 解析中默认视为敏感的字段名（小写比较）。
const SENSITIVE_KEYS: &[&str] = &[
    "password",
    "passwd",
    "pwd",
    "secret",
    "token",
    "api_key",
    "apikey",
];

/// 公共入口：把任意粘贴文本转换成 `CaptureDraft`。
///
/// 仅在输入为空或纯空白时返回 Err；其它所有输入都会产生可保存的 draft。
pub fn parse_capture_local(raw_text: &str) -> Result<CaptureDraft, String> {
    let trimmed_input = raw_text.trim();
    if trimmed_input.is_empty() {
        return Err("capture input is empty".into());
    }

    let draft = if let Some(d) = try_connection_url(trimmed_input) {
        d
    } else if let Some(d) = try_ssh(trimmed_input) {
        d
    } else if let Some(d) = try_user_pass_host_port(trimmed_input) {
        d
    } else if let Some(d) = try_multiline_key_value(trimmed_input) {
        d
    } else if let Some(d) = try_single_url_bookmark(trimmed_input) {
        d
    } else {
        build_plain_note(trimmed_input)
    };

    sanitize_draft(draft)
}

/// 统一收尾：裁剪 title、丢空字段、限制字段数量。
fn sanitize_draft(mut draft: CaptureDraft) -> Result<CaptureDraft, String> {
    draft.title = draft.title.trim().chars().take(120).collect();
    if draft.title.is_empty() {
        return Err("capture title is empty".into());
    }
    draft
        .fields
        .retain(|f| !f.key.trim().is_empty() && !f.value.trim().is_empty());
    if draft.fields.len() > 32 {
        draft.fields.truncate(32);
        draft.warnings.push("too_many_fields".into());
    }
    Ok(draft)
}

// ---------------------------------------------------------------------------
// Helpers
// ---------------------------------------------------------------------------

/// 构造一个空的 `CaptureDraft`（除 title 外其它字段初始化为空）。
fn new_draft(kind: EntryKind, title: impl Into<String>) -> CaptureDraft {
    CaptureDraft {
        kind,
        title: title.into(),
        notes: None,
        fields: Vec::new(),
        manual_tags: Vec::new(),
        ai_tags: Vec::new(),
        ai_summary: None,
        search_aliases: Vec::new(),
        ai_provenance: None,
        warnings: Vec::new(),
    }
}

/// 给 draft 追加字段，并自动赋值 `capture-field-N`。
fn push_field(draft: &mut CaptureDraft, key: &str, value: impl Into<String>, sensitive: bool) {
    let idx = draft.fields.len();
    draft.fields.push(crate::vault::models::CaptureField {
        draft_id: format!("capture-field-{idx}"),
        key: key.to_string(),
        value: value.into(),
        is_sensitive: sensitive,
    });
}

/// 判断是否敏感字段名（大小写不敏感）。
fn is_sensitive_key(key: &str) -> bool {
    let lower = key.to_lowercase();
    SENSITIVE_KEYS.iter().any(|k| lower == *k)
}

/// 把 `host[:port]` 形式的字符串拆成 (host, Option<port>)。
///
/// 支持 IPv6 字面量 `[::1]:5432`。普通 host 中若 `:` 后跟纯数字则视为 port。
fn split_host_port(s: &str) -> (String, Option<String>) {
    // 支持 IPv6 字面量 [::1]:5432
    if let Some(rest) = s.strip_prefix('[') {
        if let Some(end) = rest.find(']') {
            let host = format!("[{}]", &rest[..end]);
            let after = &rest[end + 1..];
            if let Some(port) = after.strip_prefix(':') {
                return (host, Some(port.to_string()));
            }
            return (host, None);
        }
    }
    if let Some(idx) = s.rfind(':') {
        let (h, p) = (&s[..idx], &s[idx + 1..]);
        if !h.is_empty() && !p.is_empty() && p.chars().all(|c| c.is_ascii_digit()) {
            return (h.to_string(), Some(p.to_string()));
        }
    }
    (s.to_string(), None)
}

// ---------------------------------------------------------------------------
// 优先级 1：连接 URL
// ---------------------------------------------------------------------------

fn try_connection_url(text: &str) -> Option<CaptureDraft> {
    let first_line = text.lines().next().unwrap_or("").trim();
    let lower = first_line.to_lowercase();
    let scheme = CONNECTION_SCHEMES.iter().find(|s| lower.starts_with(*s))?;
    let after_scheme = &first_line[scheme.len()..];

    // 必须有 `@`（userinfo）才视为连接 URL；没有的话让 bookmark / note 兜底
    let at_idx = after_scheme.find('@')?;
    let userinfo = &after_scheme[..at_idx];
    let host_part = &after_scheme[at_idx + 1..];

    // userinfo: user[:password]
    let (user, password) = match userinfo.find(':') {
        Some(idx) => (
            userinfo[..idx].to_string(),
            Some(userinfo[idx + 1..].to_string()),
        ),
        None => (userinfo.to_string(), None),
    };
    if user.is_empty() && password.is_none() {
        return None;
    }

    // host_part: host[:port][/path?query#frag]
    // 先去掉 path / query / fragment
    let authority_end = host_part
        .find(|c: char| c == '/' || c == '?' || c == '#')
        .unwrap_or(host_part.len());
    let authority = &host_part[..authority_end];
    let path_remainder = &host_part[authority_end..];

    let (host, port) = split_host_port(authority);
    if host.is_empty() {
        return None;
    }

    let database = path_remainder
        .trim_start_matches('/')
        .split(|c: char| c == '?' || c == '#')
        .next()
        .filter(|s| !s.is_empty())
        .map(|s| s.to_string());

    let mut draft = new_draft(EntryKind::Credential, host.clone());
    if !user.is_empty() {
        push_field(&mut draft, "user", user, false);
    }
    if let Some(p) = password {
        push_field(&mut draft, "password", p, true);
    }
    push_field(&mut draft, "host", host, false);
    if let Some(p) = port {
        push_field(&mut draft, "port", p, false);
    }
    if let Some(db) = database {
        push_field(&mut draft, "database", db, false);
    }
    Some(draft)
}

// ---------------------------------------------------------------------------
// 优先级 2：SSH
// ---------------------------------------------------------------------------

fn try_ssh(text: &str) -> Option<CaptureDraft> {
    let first_line = text.lines().next().unwrap_or("").trim();
    let lower = first_line.to_lowercase();
    if !lower.starts_with("ssh ") {
        return None;
    }
    let rest = first_line[4..].trim_start();

    // 支持 `-p N` / `-pN` 形式的端口
    let mut port: Option<String> = None;
    let mut tokens: Vec<&str> = Vec::new();
    let mut iter = rest.split_whitespace().peekable();
    while let Some(tok) = iter.next() {
        if tok == "-p" {
            if let Some(p) = iter.next() {
                if !p.is_empty() && p.chars().all(|c| c.is_ascii_digit()) {
                    port = Some(p.to_string());
                }
            }
        } else if let Some(p) = tok.strip_prefix("-p") {
            // `-p2222` 形式
            if !p.is_empty() && p.chars().all(|c| c.is_ascii_digit()) {
                port = Some(p.to_string());
            }
        } else if tok.starts_with('-') {
            // 其它未知 flag，忽略
            continue;
        } else {
            tokens.push(tok);
        }
    }

    let user_at_host = tokens.into_iter().next()?;
    let at_idx = user_at_host.find('@')?;
    let user = &user_at_host[..at_idx];
    let host = &user_at_host[at_idx + 1..];
    if user.is_empty() || host.is_empty() {
        return None;
    }

    let mut draft = new_draft(EntryKind::Credential, host.to_string());
    push_field(&mut draft, "user", user, false);
    push_field(&mut draft, "host", host, false);
    if let Some(p) = port {
        push_field(&mut draft, "port", p, false);
    }
    Some(draft)
}

// ---------------------------------------------------------------------------
// 优先级 3：user:pass@host:port（无 scheme）
// ---------------------------------------------------------------------------

fn try_user_pass_host_port(text: &str) -> Option<CaptureDraft> {
    let first_line = text.lines().next().unwrap_or("").trim();
    // 整体不能是 URL（含 `://`）
    if first_line.contains("://") {
        return None;
    }
    let at_idx = first_line.find('@')?;
    let userinfo = &first_line[..at_idx];
    let host_part = &first_line[at_idx + 1..];

    // userinfo 必须有冒号分隔 password
    let colon_idx = userinfo.find(':')?;
    let user = &userinfo[..colon_idx];
    let password = &userinfo[colon_idx + 1..];
    if user.is_empty() || password.is_empty() {
        return None;
    }
    // user / password 不应含空白
    if user.chars().any(|c| c.is_whitespace())
        || password.chars().any(|c| c.is_whitespace())
    {
        return None;
    }

    // host_part 必须形如 host[:port]，不能再含 `@` 或空白
    if host_part.contains('@') || host_part.chars().any(|c| c.is_whitespace()) {
        return None;
    }
    let (host, port) = split_host_port(host_part);
    if host.is_empty() {
        return None;
    }
    // 要求 host 看起来像 host（含 `.`/IP/localhost），或者后接明确的数字 port。
    // 这样可避免普通 email（user@host.com 不会触发，因为没有 password 冒号）
    // 及自由文本误判。
    let looks_like_host = host.contains('.')
        || host == "localhost"
        || host.starts_with('[')
        || host.chars().all(|c| c.is_ascii_digit() || c == '.' || c == ':');
    if !looks_like_host && port.is_none() {
        return None;
    }

    let mut draft = new_draft(EntryKind::Credential, host.clone());
    push_field(&mut draft, "user", user, false);
    push_field(&mut draft, "password", password, true);
    push_field(&mut draft, "host", host, false);
    if let Some(p) = port {
        push_field(&mut draft, "port", p, false);
    }
    Some(draft)
}

// ---------------------------------------------------------------------------
// 优先级 4：多行 key-value
// ---------------------------------------------------------------------------

fn try_multiline_key_value(text: &str) -> Option<CaptureDraft> {
    let non_empty_count = text.lines().filter(|l| !l.trim().is_empty()).count();
    // 必须多于 1 行才考虑多行 KV
    if non_empty_count < 2 {
        return None;
    }

    let mut pairs: Vec<(String, String)> = Vec::new();
    for line in text.lines() {
        let line = line.trim();
        if line.is_empty() {
            continue;
        }
        // 识别 key ([:=]) value
        let sep_idx = line.find(|c: char| c == ':' || c == '=')?;
        let (k, v) = line.split_at(sep_idx);
        let key = k.trim();
        let value = v[1..].trim(); // 跳过分隔符
        if key.is_empty() || value.is_empty() {
            return None;
        }
        // key 不应含空格
        if key.chars().any(|c| c.is_whitespace()) {
            return None;
        }
        pairs.push((key.to_string(), value.to_string()));
    }

    if pairs.len() < 2 {
        return None;
    }

    // 推导 title：优先 host/hostname，其次 URL，再退回首字段 value
    let title = pairs
        .iter()
        .find(|(k, _)| {
            let lk = k.to_lowercase();
            lk == "host" || lk == "hostname"
        })
        .map(|(_, v)| v.clone())
        .or_else(|| {
            pairs
                .iter()
                .find(|(k, _)| k.to_lowercase() == "url")
                .map(|(_, v)| v.clone())
        })
        .unwrap_or_else(|| pairs[0].1.clone());

    let mut draft = new_draft(EntryKind::Credential, title);
    for (k, v) in pairs {
        push_field(&mut draft, &k, v, is_sensitive_key(&k));
    }
    Some(draft)
}

// ---------------------------------------------------------------------------
// 优先级 5：单 URL bookmark
// ---------------------------------------------------------------------------

fn try_single_url_bookmark(text: &str) -> Option<CaptureDraft> {
    let first_line = text.lines().next().unwrap_or("").trim();
    if first_line.is_empty() {
        return None;
    }
    let lower = first_line.to_lowercase();
    let scheme = BOOKMARK_SCHEMES.iter().find(|s| lower.starts_with(*s))?;
    // bookmark：不应有 userinfo（含 `@` 的情况会被 connection URL 或 user:pass 分支接住）
    let after_scheme = &first_line[scheme.len()..];

    // 推导 host：从 scheme 后到第一个 `/`、`?`、`#` 之间
    let authority_end = after_scheme
        .find(|c: char| c == '/' || c == '?' || c == '#')
        .unwrap_or(after_scheme.len());
    let authority = &after_scheme[..authority_end];
    let (host, _port) = split_host_port(authority);

    let title: String = if host.is_empty() {
        first_line.to_string()
    } else {
        host.clone()
    };

    let mut draft = new_draft(EntryKind::Bookmark, title);
    push_field(&mut draft, "url", first_line, false);
    Some(draft)
}

// ---------------------------------------------------------------------------
// 优先级 6：普通笔记（兜底）
// ---------------------------------------------------------------------------

fn build_plain_note(text: &str) -> CaptureDraft {
    let title: String = text
        .lines()
        .map(|l| l.trim())
        .find(|l| !l.is_empty())
        .unwrap_or("")
        .chars()
        .take(60)
        .collect();

    let mut draft = new_draft(EntryKind::Note, title);
    draft.notes = Some(text.to_string());
    draft
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;
    use crate::vault::models::{CaptureDraft, CaptureField, EntryKind};

    fn field<'a>(draft: &'a CaptureDraft, key: &str) -> &'a CaptureField {
        draft
            .fields
            .iter()
            .find(|field| field.key == key)
            .unwrap_or_else(|| panic!("field `{key}` not present in draft: {draft:?}"))
    }

    #[test]
    fn parses_connection_url_into_credential() {
        let draft = parse_capture_local("postgres://alice:hunter2@db.internal:5432/app").unwrap();
        assert_eq!(draft.kind, EntryKind::Credential);
        assert_eq!(field(&draft, "user").value, "alice");
        assert!(field(&draft, "password").is_sensitive);
        assert_eq!(field(&draft, "host").value, "db.internal");
        assert_eq!(field(&draft, "port").value, "5432");
    }

    #[test]
    fn unknown_text_is_always_a_saveable_note() {
        let draft = parse_capture_local("remember the staging rollout sequence").unwrap();
        assert_eq!(draft.kind, EntryKind::Note);
        assert!(!draft.title.trim().is_empty());
        assert_eq!(
            draft.notes.as_deref(),
            Some("remember the staging rollout sequence")
        );
    }

    #[test]
    fn parses_ssh_with_port_flag() {
        let draft = parse_capture_local("ssh user@host -p 2222").unwrap();
        assert_eq!(draft.kind, EntryKind::Credential);
        assert_eq!(field(&draft, "user").value, "user");
        assert_eq!(field(&draft, "host").value, "host");
        assert_eq!(field(&draft, "port").value, "2222");
        assert!(!field(&draft, "port").is_sensitive);
    }

    #[test]
    fn parses_ssh_without_port() {
        let draft = parse_capture_local("ssh deploy@web.example.com").unwrap();
        assert_eq!(draft.kind, EntryKind::Credential);
        assert_eq!(field(&draft, "user").value, "deploy");
        assert_eq!(field(&draft, "host").value, "web.example.com");
        assert_eq!(draft.title, "web.example.com");
    }

    #[test]
    fn parses_user_pass_host_port_without_scheme() {
        let draft = parse_capture_local("alice:hunter2@db.internal:5432").unwrap();
        assert_eq!(draft.kind, EntryKind::Credential);
        assert_eq!(field(&draft, "user").value, "alice");
        assert_eq!(field(&draft, "password").value, "hunter2");
        assert!(field(&draft, "password").is_sensitive);
        assert_eq!(field(&draft, "host").value, "db.internal");
        assert_eq!(field(&draft, "port").value, "5432");
    }

    #[test]
    fn parses_user_pass_host_port_mysql_style() {
        let draft = parse_capture_local("user:pass@host:3306").unwrap();
        assert_eq!(draft.kind, EntryKind::Credential);
        assert_eq!(field(&draft, "user").value, "user");
        assert_eq!(field(&draft, "password").value, "pass");
        assert_eq!(field(&draft, "host").value, "host");
        assert_eq!(field(&draft, "port").value, "3306");
    }

    #[test]
    fn single_url_becomes_bookmark() {
        let draft = parse_capture_local("https://example.com/path?query=1").unwrap();
        assert_eq!(draft.kind, EntryKind::Bookmark);
        assert_eq!(field(&draft, "url").value, "https://example.com/path?query=1");
        assert_eq!(draft.title, "example.com");
    }

    #[test]
    fn single_http_url_without_path() {
        let draft = parse_capture_local("http://example.com").unwrap();
        assert_eq!(draft.kind, EntryKind::Bookmark);
        assert_eq!(draft.title, "example.com");
    }

    #[test]
    fn multiline_key_value_becomes_credential() {
        let raw = "host = db.internal\nuser = alice\npassword = hunter2\nport = 5432";
        let draft = parse_capture_local(raw).unwrap();
        assert_eq!(draft.kind, EntryKind::Credential);
        assert_eq!(draft.title, "db.internal");
        assert_eq!(field(&draft, "host").value, "db.internal");
        assert_eq!(field(&draft, "user").value, "alice");
        assert_eq!(field(&draft, "password").value, "hunter2");
        assert!(field(&draft, "password").is_sensitive);
        assert_eq!(field(&draft, "port").value, "5432");
    }

    #[test]
    fn multiline_key_value_with_colon_separator() {
        let raw = "host: db.internal\nuser: alice\nsecret: topsecret";
        let draft = parse_capture_local(raw).unwrap();
        assert_eq!(draft.kind, EntryKind::Credential);
        assert_eq!(draft.title, "db.internal");
        assert!(field(&draft, "secret").is_sensitive);
    }

    #[test]
    fn multiline_ip_user_password_combo() {
        let raw = "host = 192.168.1.1\nuser = admin\npassword = admin123";
        let draft = parse_capture_local(raw).unwrap();
        assert_eq!(draft.kind, EntryKind::Credential);
        assert_eq!(draft.title, "192.168.1.1");
        assert_eq!(field(&draft, "host").value, "192.168.1.1");
        assert_eq!(field(&draft, "user").value, "admin");
        assert_eq!(field(&draft, "password").value, "admin123");
        assert!(field(&draft, "password").is_sensitive);
    }

    #[test]
    fn empty_input_is_rejected() {
        assert!(parse_capture_local("").is_err());
        assert!(parse_capture_local("   ").is_err());
        assert!(parse_capture_local("\n\n\t\n").is_err());
    }

    #[test]
    fn fields_get_sequential_capture_field_ids() {
        let draft = parse_capture_local("postgres://u:p@h:5432/db").unwrap();
        for (i, f) in draft.fields.iter().enumerate() {
            assert_eq!(f.draft_id, format!("capture-field-{i}"));
        }
    }

    #[test]
    fn plain_note_title_truncated_to_sixty_chars() {
        let long = "a".repeat(120);
        let draft = parse_capture_local(&long).unwrap();
        assert_eq!(draft.kind, EntryKind::Note);
        assert!(draft.title.chars().count() <= 60);
        assert_eq!(draft.title.chars().count(), 60);
    }

    #[test]
    fn multiline_key_value_single_line_falls_through_to_note() {
        // 只有 1 行 KV 不能算 KV，会落到普通笔记
        let draft = parse_capture_local("just one key=value").unwrap();
        assert_eq!(draft.kind, EntryKind::Note);
    }

    #[test]
    fn redis_url_with_only_password() {
        let draft = parse_capture_local("redis://:password@host:6379/0").unwrap();
        assert_eq!(draft.kind, EntryKind::Credential);
        // user 为空时不应产生 user 字段
        assert!(draft.fields.iter().all(|f| f.key != "user"));
        assert_eq!(field(&draft, "password").value, "password");
        assert!(field(&draft, "password").is_sensitive);
        assert_eq!(field(&draft, "host").value, "host");
        assert_eq!(field(&draft, "port").value, "6379");
        assert_eq!(field(&draft, "database").value, "0");
    }

    #[test]
    fn email_is_not_treated_as_credential() {
        // user@host 但无冒号 password：不应被 try_user_pass_host_port 误判
        let draft = parse_capture_local("alice@example.com").unwrap();
        assert_eq!(draft.kind, EntryKind::Note);
    }
}
