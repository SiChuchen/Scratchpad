# Vault UX 重构 + 速记工作流

> **状态：已被取代，请勿按本文实施。**
>
> 2026-07-17 的产品评审确认不再新增顶级“速记”Tab，并将 Vault 的用户定位调整为“资料库”。新的正式设计见：
> `docs/superpowers/specs/2026-07-17-library-quick-access-design.md`

- 日期：2026-07-16
- 分支：`feature/vault-credentials`
- 关联文档：`docs/superpowers/plans/2026-07-15-vault-credentials.md`（初始实现）

## 1. 背景

当前保险箱（Vault）UI 存在信息架构问题：

- Vault header 一行内塞了过多交互元素：filter tabs（全部/凭据/书签/笔记）+ 智能搜索 filter tab + 内联搜索框 + 3 个新建按钮 + 导入按钮，语义层级混乱。
- FTS5 关键词搜索与 LLM 自然语言搜索混杂在同一个 header，用户心智不清。
- SmartImportDialog（智能导入）藏在弹窗里，入口不突出。
- LLM 打标用覆盖语义（`set_tags`），会冲掉用户手动加的标签。

经讨论重新梳理用户心智，确定按「职责拆分顶级 tab」重构。

## 2. 用户心智模型

### 2.1 三个顶级 tab

| Tab | 职责 | 存储 |
|-----|------|------|
| 收纳（HomeView） | 日常 dock 条目管理 | 现有 dock schema |
| 速记（QuickCapture，**新**） | 粘贴 → 脱敏 → LLM 智能解析 → 确认保存 | `vault_entries` |
| 保险箱（VaultView，**精简**） | 浏览 + 手动表单新建 + FTS5 搜索 | `vault_entries` |

速记与保险箱操作同一张 `vault_entries` 表，只是录入形态不同：

- 速记 = 粘贴式录入（LLM 辅助分类提取字段+标签）
- 保险箱 = 表单式录入（手动填写）+ 浏览/搜索

### 2.2 标签哲学

**LLM 是标签的主生产者**：

- 创建条目时 LLM 自动打标（主路径）。
- 用户手动编辑标签是兜底微调，不作为卖点突出。
- retag（重新分析）时，LLM 结果与现有标签**并集去重**，不覆盖用户手动加的标签。

### 2.3 搜索行为

保险箱搜索框 = FTS5 关键词**隐式混搜**，匹配 `title + notes + 字段值 + tags`（现状已满足，`build_searchable` 把 tags 拼进 searchable 字段）。

- 不加 `tag:` 显式语法（保持简单）。
- 去掉 LLM 智能搜索 filter tab。
- 后端 `ipc_vault_llm_search` 端点保留备用，前端不再暴露。

## 3. 设计方案

### 3.1 速记 tab 工作流

```
用户粘贴原始文本
       │
       ▼
  本地脱敏（正则识别 secret）
  规则：API key / 密码 / token / PEM / JWT / Bearer / URL creds /
       long base64(≥56) / AWS AK / CC
       │
       ▼
  secret → token_map（SHA-256 + 会话盐 + 双向映射，仅内存）
       │
       ▼
  脱敏文本 + 指令 → LLM（OpenAI 兼容，JSON mode，temp 0.0）
       │
       ▼
  LLM 返回 { kind, title, fields:[{key,value}], tags:[...] }
  （field value 中的 secret 用占位符 [TOKEN_xxx] 引用）
       │
       ▼
  前端预览
  （secret 字段从 token_map 反向还原真实值，仅本地可见）
       │
       ▼
  用户确认 / 编辑（可改类型、字段、标签）
       │
       ▼
  保存到 vault_entries
  （占位符 → 真实值回填后写入；触发 suggest_tags 合并去重 + fts5_upsert）
```

#### 脱敏：自由文本入口

现有 `desensitize_entry` 接收的是结构化 `VaultEntry`，速记场景输入是**自由文本**。需要新增：

```rust
// src-tauri/src/vault/desensitize.rs
pub fn desensitize_raw_text(text: &str, map: &mut TokenMap) -> String
```

- 复用现有正则规则集（`build_regex_set`，7 条规则）。
- 扫描自由文本，匹配到的 secret 入 token_map，替换为 `[TOKEN_<short>]` 占位符。
- 单测覆盖每条规则在自由文本中的命中与映射正确性。

#### LLM Prompt

新增 `capture_prompt`：

```rust
// src-tauri/src/vault/llm/prompt.rs
pub fn capture_prompt(desensitized_text: &str) -> Vec<ChatMessage>
```

系统指令大意：

> 你是信息分类助手。用户粘贴的文本可能包含凭据/书签/笔记。
> 文本中的 `[TOKEN_xxx]` 是敏感信息占位符，请在对应字段值里原样保留。
> 只输出 JSON：`{ "kind": "credential"|"bookmark"|"note", "title": "...", "fields": [{"key":"...","value":"..."}], "tags": ["..."] }`。不要解释。

参数：`json_mode: true, temperature: 0.0, max_tokens: Some(512)`。

#### 占位符回填

LLM 返回的 fields 里 value 可能是 `[TOKEN_abc123]`。保存到 `vault_entries` 前：

- 遍历 fields，通过 token_map 反向查找替换为真实值。
- 未知占位符保持原样（容错，不阻断保存）。
- 被回填的字段标记 `is_sensitive = true`（来自 token_map 的必为敏感）。

### 3.2 标签合并去重

修改 `suggest_tags_for_entry`（`src-tauri/src/vault/ipc.rs:76`）：

```rust
// 当前（覆盖）：
let _ = vstore::set_tags(&mut conn, &entry_id, &parsed.tags);

// 改为（合并去重）：
let existing = vstore::list_tags(&conn, &entry_id).unwrap_or_default();
let mut merged = existing.clone();
for t in &parsed.tags {
    if !merged.iter().any(|e| e.eq_ignore_ascii_case(t)) {
        merged.push(t.clone());
    }
}
let _ = vstore::set_tags(&mut conn, &entry_id, &merged);
// 事件推送合并后的最终结果
Ok(merged)
```

### 3.3 保险箱 header 精简

当前 `VaultView.svelte` header：

```
[全部][凭据][书签][笔记][🤖 智能搜索]  [搜索框......]  [+凭据][+书签][+笔记][📥]
```

精简后：

```
[全部][凭据][书签][笔记]  [搜索框......]  [+凭据][+书签][+笔记]
```

具体改动：

- `Filter` 类型去掉 `'search'` 变体：`type Filter = EntryKind | 'all'`。
- 删除 `🤖 智能搜索` filter 按钮及对应分支。
- 删除 `📥` 导入按钮（已提升为速记 tab）。
- 删除 `LlmSearchPanel` 引用、`showImport` / `SmartImportDialog` 引用。
- `SearchBar` 保持现状（已是纯 FTS5、窄框、对齐 HomeView 样式）。
- 搜索结果仍由 `searchResults` 驱动内联展示，逻辑不变。

### 3.4 导航改动

- 在导航组件（`App.svelte` 或专用 nav 组件）里，于「收纳」与「保险箱」之间插入「速记」tab。
- `src/lib/i18n/locales/zh-CN.ts` 加 `nav.capture = '速记'`（或类似文案）。
- 速记 tab 图标暂定 `📋` 或 `✏️`。

### 3.5 速记 tab 组件

新增 `src/lib/components/views/QuickCaptureView.svelte`：

- 顶部：大文本域（≥4 行）用于粘贴，占位提示示例。
- 粘贴后（或点击「解析」按钮）→ 调 `vaultApi.parseCapture(text)`。
- loading / error 状态展示。
- 解析成功后展示**预览卡片**：
  - 类型选择（credential/bookmark/note，可改）
  - title 输入框（可改）
  - 字段列表（key/value 可编辑；敏感字段默认掩码，点击切换显示）
  - 标签列表（可增删）
- 底部「保存」按钮 → 调 `vaultApi.createEntry(...)` → 成功后清空 + toast 提示。
- LLM 未配置时：错误提示引导去设置页。

## 4. 后端端点清单

### 4.1 新增

```rust
#[derive(Debug, Serialize, Clone)]
#[serde(rename_all = "camelCase")]
pub struct CaptureField {
    pub key: String,
    pub value: String,       // 已回填真实值
    pub is_sensitive: bool,  // 来自 token_map 则为 true
}

#[derive(Debug, Serialize, Clone)]
#[serde(rename_all = "camelCase")]
pub struct ParsedCapture {
    pub kind: EntryKind,
    pub title: String,
    pub fields: Vec<CaptureField>,
    pub tags: Vec<String>,
}

#[tauri::command]
pub async fn ipc_vault_parse_capture(
    state: State<'_, crate::AppState>,
    app: AppHandle,
    raw_text: String,
) -> Result<ParsedCapture, String>
```

流程：

1. `token_map.lock()` → `desensitize_raw_text(&raw_text, &mut map)` → drop guard。
2. 取 LLM config；无配置返回错误（前端引导去设置）。
3. `capture_prompt(&desensitized)` → `adapter.complete(req)` （**不持任何锁**）。
4. 解析 JSON，占位符通过 token_map 反向还原 field value。
5. 返回 `ParsedCapture`（真实值仅本地，不出进程）。

锁纪律：db 先 token_map 后；MutexGuard 不跨 `.await`（与现有 `ipc_vault_llm_search` 一致）。

### 4.2 修改

- `suggest_tags_for_entry`（ipc.rs:76）：`set_tags` 覆盖 → 合并去重，返回合并后结果。
- `lib.rs`：注册 `ipc_vault_parse_capture`。

### 4.3 保留备用

- `ipc_vault_llm_search`：前端不再调用，端点保留（未来可能复用）。
- `LlmSearchPanel.svelte` / `SmartImportDialog.svelte`：移除引用，文件可留可删（建议删，避免混淆）。

## 5. 前端 API 层

`src/lib/api/vault.ts` 新增：

```ts
parseCapture: (rawText: string) =>
  invoke<ParsedCapture>('ipc_vault_parse_capture', { rawText }),
```

`src/lib/types/vault.ts` 新增 `ParsedCapture` / `CaptureField` 类型。

## 6. 任务拆解

### 阶段 1：后端
1. `desensitize.rs`：新增 `desensitize_raw_text` + 单测
2. `llm/prompt.rs`：新增 `capture_prompt` + 单测
3. `ipc.rs`：新增 `ParsedCapture` / `CaptureField` 结构 + `ipc_vault_parse_capture` 端点
4. `ipc.rs`：改 `suggest_tags_for_entry` 合并去重 + 单测
5. `lib.rs`：注册新端点
6. `cargo test --offline` 全绿

### 阶段 2：前端 API
7. `types/vault.ts`：加 `ParsedCapture` / `CaptureField`
8. `api/vault.ts`：加 `parseCapture`

### 阶段 3：前端 UI
9. 新增 `QuickCaptureView.svelte`（粘贴 + 解析 + 预览 + 保存）
10. 精简 `VaultView.svelte`（去智能搜索 tab、去导入按钮、Filter 类型收窄）
11. 导航加「速记」tab
12. `i18n/locales/zh-CN.ts` 加文案

### 阶段 4：收尾
13. 删除 `SmartImportDialog.svelte`、`LlmSearchPanel.svelte`（如未被其他地方引用）
14. `pnpm check` 通过
15. `pnpm tauri dev` 手动验证完整流程

## 7. 测试要点

### Rust 单测

- `desensitize_raw_text`：每条正则规则在自由文本中的命中；多 secret 不互相干扰；token_map 双向映射正确。
- `capture_prompt`：system prompt 含占位符保留指令 + JSON schema 说明。
- `suggest_tags_for_entry` 合并去重：`existing=["a","B"]` ∪ `llm=["b","c"]` → `["a","B","c"]`（大小写不敏感去重）。
- 占位符回填：已知占位符还原真实值；未知占位符保持原样。

### 前端手动验证

- 速记：粘贴 `ghp_xxxxxxxxxxxx` + 一段说明 → LLM 返回 credential + 字段 → 预览正确显示（secret 掩码可切换）→ 保存 → 在保险箱列表出现。
- 速记：粘贴一个 URL → 分类为 bookmark。
- 速记：粘贴一段纯文字 → 分类为 note。
- 速记：LLM 未配置时显示明确错误，引导去设置。
- 速记：网络/超时失败时静默降级提示。
- 保险箱：搜索框输入 tag 名能命中对应条目。
- 保险箱：header 不再有智能搜索 tab 和导入按钮。
- 标签：手动加 `重要` 标签 → 点 retag → `重要` 仍在，LLM 新标签已并入。

## 8. 不在本次范围内

- 向量 embedding 语义搜索。
- `tag:` 显式过滤语法。
- 标签全局管理（重命名 / 合并 / 全局列表）。
- 多轮对话 / Agent 工具调用。
- 速记 tab 的历史记录（粘贴内容不单独存档，保存后即为普通 vault entry）。
- 应层加密（按用户明确选择，API key 明文存 preferences 表）。

## 9. 安全约束（不变）

- 原始密码 / 机密绝不能离开本地进程。
- LLM 调用前必须脱敏（正则 + token_map 占位符替换）。
- LLM 仅走 OpenAI 兼容协议。
- LLM 调用 30 秒超时，无重试，失败静默降级。
- token_map 仅内存，进程退出即失效。

## 10. 风险与备选

| 风险 | 影响 | 备选 |
|------|------|------|
| LLM 分类不准（如把凭据误判为 note） | 条目进错 filter | 预览页可手动改 kind；可接受 |
| 脱敏正则漏判某种 secret 格式 | 敏感信息泄露给 LLM | 正则规则集可后续扩充；先覆盖已知高频格式 |
| LLM 返回的占位符与 token_map 对不上 | secret 字段保存为字面占位符 | 回填容错：未知占位符保持原样，用户在预览页可手动改 |
| 速记 tab 与保险箱手动新建功能重叠 | 用户困惑两个入口的关系 | 速记面向「懒人快速录」，保险箱面向「精细填写」，tab 文案与提示区分 |
