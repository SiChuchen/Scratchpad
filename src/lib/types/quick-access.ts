// src/lib/types/quick-access.ts
//
// Quick-access 面板的状态类型定义。把类型放在 types/ 目录以便 Svelte
// 组件以 `import type` 方式引用——避免与状态模块中的运行时导出共存的类型在
// svelte-check 中触发 runes 模式检测的边缘问题。

export type QuickAccessMode = 'record' | 'search'

export interface QuickAccessState {
  mode: QuickAccessMode
  /** Record 模式下的录入草稿文本。 */
  draft: string
  /** Search 模式下的查询字符串。 */
  query: string
  /** Search 模式下当前选中的条目 ID（用于上下文）。 */
  selectedId: string | null
}
