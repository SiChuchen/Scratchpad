// src-tauri/src/vault/llm/prompt.rs
use crate::vault::desensitize::DesensitizedEntry;
use crate::vault::llm::ChatMessage;

pub fn tag_prompt(entry: &DesensitizedEntry) -> Vec<ChatMessage> {
    let fields_block = entry
        .fields
        .iter()
        .map(|f| format!("- {}: {}", f.key, f.value))
        .collect::<Vec<_>>()
        .join("\n");

    vec![
        ChatMessage::system(
            "你是一个标签生成助手。为条目生成 3-5 个简短标签（2-4 字，中文优先）。
             只输出 JSON：{\"tags\": [\"tag1\", \"tag2\"]}。不要解释。"
        ),
        ChatMessage::user(format!(
            "类型: {}\n标题: {}\n字段:\n{}\n备注: {}",
            entry.kind.as_str(),
            entry.title,
            fields_block,
            entry.notes
        )),
    ]
}

pub fn search_prompt(query: &str, catalog: &[DesensitizedEntry]) -> Vec<ChatMessage> {
    let lines = catalog
        .iter()
        .map(|e| {
            let fields = e
                .fields
                .iter()
                .map(|f| format!("{}={}", f.key, f.value))
                .collect::<Vec<_>>()
                .join(",");
            format!(
                "id={}|title={}|kind={}|tags=[{}]|fields={}",
                e.id,
                e.title,
                e.kind.as_str(),
                e.tags.join(","),
                fields
            )
        })
        .collect::<Vec<_>>()
        .join("\n");

    vec![
        ChatMessage::system(
            "你是检索助手。从候选条目中找出最匹配用户查询的（最多 5 个）。
             只输出 JSON：{\"matches\": [\"id1\", \"id2\"]}。
             无匹配返回 {\"matches\": []}。"
        ),
        ChatMessage::user(format!("查询: {}\n候选:\n{}", query, lines)),
    ]
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::vault::desensitize::{DesensitizedEntry, DesensitizedField};
    use crate::vault::models::EntryKind;

    fn sample() -> DesensitizedEntry {
        DesensitizedEntry {
            id: "v1".into(),
            kind: EntryKind::Credential,
            title: "Prod DB".into(),
            notes: "mysql".into(),
            fields: vec![
                DesensitizedField {
                    key: "user".into(),
                    value: "admin".into(),
                    was_sensitive: false,
                },
                DesensitizedField {
                    key: "password".into(),
                    value: "[SECRET:abc]".into(),
                    was_sensitive: true,
                },
            ],
            tags: vec![],
        }
    }

    #[test]
    fn tag_prompt_user_message_contains_token_for_sensitive() {
        let msgs = tag_prompt(&sample());
        let user_msg = msgs.iter().find(|m| m.role == "user").unwrap();
        assert!(user_msg.content.contains("[SECRET:abc]"));
        assert!(user_msg.content.contains("Prod DB"));
        assert!(user_msg.content.contains("admin"));
    }

    #[test]
    fn tag_prompt_system_forces_json_output() {
        let msgs = tag_prompt(&sample());
        let sys = msgs.iter().find(|m| m.role == "system").unwrap();
        assert!(sys.content.contains("{\"tags\""));
    }

    #[test]
    fn search_prompt_includes_all_entries() {
        let mut e2 = sample();
        e2.id = "v2".into();
        e2.title = "Other".into();
        let catalog = vec![sample(), e2];
        let msgs = search_prompt("query", &catalog);
        let user_msg = msgs.iter().find(|m| m.role == "user").unwrap();
        assert!(user_msg.content.contains("id=v1"));
        assert!(user_msg.content.contains("id=v2"));
    }
}
