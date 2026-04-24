# 设计规格：主题与 Token 系统

日期：2026-04-24（v3 — 二次审查修订版）

## 目标

将 Soma Scratchpad 前端从硬编码 `rgba()` 值的散落式样式，改造为基于 CSS token 的主题系统。同时提供 3 套预设主题、系统深浅色跟随、以及用户自定义能力。

## 约束

- 产品是桌面悬浮工具，始终置顶，需要保持一定透明度不遮挡下层窗口
- `surface-0`（dock 背景）推荐不透明度范围 **60%-95%**，预设默认值在 78-85%，用户可通过设置页自由调节
- 所有主题必须保留 `backdrop-filter: blur(24px)` 玻璃效果
- UI 文案全部为简体中文
- 字体使用系统字体（Microsoft YaHei / Segoe UI），不依赖网络字体
- 产品处于初期阶段，无需考虑旧数据迁移
- `:root` 的 `font-size` 固定为 `15px` 不变（间距和控件尺寸依赖 rem 基准），具体文本字号通过 token 控制

---

## 1. Token 体系架构

### 三层结构

1. **语义层** — 定义在主题预设中，随主题切换而变化
2. **组件层** — 引用语义 token，组件实际使用的变量
3. **硬编码引用** — 组件 `<style>` 中只写 `var(--xxx)`，不出现 `rgba(...)` 原始值

### Token 分类与数量

| 类别 | 数量 | 专家模式可编辑 | 说明 |
|---|---|---|---|
| 颜色 token | 17 | 是 | 定义在主题预设中，所有颜色均可能含 alpha |
| 效果 token | 1 | 是 | `--shadow-default`，独立于颜色，校验规则不同 |
| 布局 token | 6 | 是（通过覆盖） | 间距 3 + 圆角 3，默认通过档位切换，专家模式可逐项覆盖 |
| 衍生字号 token | 6 | 否 | 由两个基准字号 + 比例系数派生，用户调滑块 |
| **合计** | **30** | **24 可编辑** | |

### 颜色 Token（17 个）

所有颜色 token 均可能包含 alpha（包括 `--text-primary` 在浅色主题中是 `rgba(..., 0.88)`）。编辑器统一提供色块 + alpha 滑块。

| Token | 用途 |
|---|---|
| `--color-primary` | 主强调色 |
| `--color-primary-light` | 主强调色浅色变体 |
| `--color-primary-faint` | 主强调色极浅背景 |
| `--color-accent` | 次强调色（收藏/Note） |
| `--color-danger` | 危险色（删除/错误） |
| `--color-success` | 成功色 |
| `--color-info` | 信息色（图片 badge） |
| `--color-file` | 文件 badge |
| `--surface-0` | 容器底色（dock 背景） |
| `--surface-1` | 卡片/表面层 |
| `--surface-2` | 凹陷表面（输入框、代码块） |
| `--text-primary` | 主文字 |
| `--text-muted` | 次要文字（时间、预览） |
| `--text-faint` | 极淡文字（placeholder） |
| `--border-default` | 默认边框 |
| `--border-subtle` | 极淡边框（分割线） |
| `--border-emphasis` | 强调边框（hover、active） |

### 效果 Token（1 个）

独立于颜色 token，有自己的校验规则和编辑器。

| Token | 用途 |
|---|---|
| `--shadow-default` | 基础 box-shadow 值 |

### 布局 Token（6 个）

**间距（3 个）：**

| Token | 标准档默认值 | 影响范围 |
|---|---|---|
| `--space-sm` | 0.2rem | badge 内边距、按钮间隙 |
| `--space-md` | 0.35rem | header gap、卡片 gap |
| `--space-lg` | 0.55rem | section padding |

用户可选「紧凑/标准/宽松」三档。

**圆角（3 个）：**

| Token | 微圆档默认值 | 影响范围 |
|---|---|---|
| `--radius-sm` | 0.25rem | badge、小按钮 |
| `--radius-md` | 0.35rem | 卡片、输入框 |
| `--radius-lg` | 0.5rem | 大容器、弹窗 |

用户可选「锐利/微圆/圆润」三档。

### 衍生字号 Token（6 个）

`:root` 的 `font-size` 固定 `15px` 不变。字号通过 CSS 变量 token 控制，不依赖 rem 基准变化。

界面字号 token（由 `ui_text_size_px` 基准派生）：

| Token | 比例 | 默认(12px基准) | 用途 |
|---|---|---|---|
| `--font-xs` | ui × 0.78 | 9.4px | meta 标签、时间戳 |
| `--font-sm` | ui × 0.875 | 10.5px | badge、条目预览 |
| `--font-md` | ui × 1.0 | 12px | 导航按钮、设置标签、操作按钮 |
| `--font-lg` | ui × 1.1 | 13.2px | 区块标题 |

内容字号 token（由 `content_text_size_px` 基准派生）：

| Token | 比例 | 默认(14px基准) | 用途 |
|---|---|---|---|
| `--font-body` | content × 0.85 | 11.9px | 文本条目内容 |
| `--font-mono` | content × 0.85 | 11.9px | 代码/等宽内容 |

设置页提供两个独立滑块：
- 界面字号：范围 10-16px，默认 12px
- 内容字号：范围 12-20px，默认 14px

### 完整组件 Token 映射表

每个组件中所有硬编码颜色值都必须映射到 token。以下为完整清单：

**通用：**

| 组件变量 | 映射 |
|---|---|
| `--card-bg` | `var(--surface-1)` |
| `--card-border` | `var(--border-default)` |
| `--card-hover-border` | `var(--border-emphasis)` |
| `--input-bg` | `var(--surface-2)` |
| `--input-border` | `var(--border-default)` |
| `--input-focus-border` | `var(--color-primary-light)` |
| `--btn-bg` | `color-mix(in srgb, var(--text-primary) 8%, transparent)` |
| `--btn-hover-bg` | `color-mix(in srgb, var(--text-primary) 18%, transparent)` |
| `--btn-border` | `color-mix(in srgb, var(--text-primary) 15%, transparent)` |
| `--scrollbar-thumb` | `color-mix(in srgb, var(--text-muted) 30%, transparent)` |
| `--divider-color` | `var(--border-subtle)` |

**EntryCard：**

| 原始硬编码 | 组件变量 | 映射 |
|---|---|---|
| badge 文本蓝底 | `--badge-text-bg` | `var(--color-primary-faint)` |
| badge 文本色 | `--badge-text-color` | `var(--color-primary)` |
| badge 图片紫底 | `--badge-image-bg` | `color-mix(in srgb, var(--color-info) 10%, transparent)` |
| badge 图片色 | `--badge-image-color` | `var(--color-info)` |
| badge 文件绿底 | `--badge-file-bg` | `color-mix(in srgb, var(--color-file) 10%, transparent)` |
| badge 文件色 | `--badge-file-color` | `var(--color-file)` |
| 收藏按钮黄 | `--accent-color` | `var(--color-accent)` |
| 删除按钮红 | `--danger-color` | `var(--color-danger)` |
| 折叠复制蓝 | `--copy-action-bg` | `color-mix(in srgb, var(--color-primary) 10%, transparent)` |
| 折叠复制蓝边框 | `--copy-action-border` | `color-mix(in srgb, var(--color-primary) 25%, transparent)` |
| 折叠复制蓝字 | `--copy-action-color` | `var(--color-primary)` |
| 预览文字 | `--preview-color` | `var(--text-muted)` |
| 时间戳 | `--time-color` | `color-mix(in srgb, var(--text-muted) 80%, transparent)` |

**TopBar：**

| 原始 | 组件变量 | 映射 |
|---|---|---|
| 底部分割线 | `--topbar-border` | `var(--border-subtle)` |
| nav-btn active 背景 | `--nav-active-bg` | `color-mix(in srgb, var(--text-primary) 15%, transparent)` |
| nav-btn active 边框 | `--nav-active-border` | `color-mix(in srgb, var(--text-primary) 25%, transparent)` |
| pin 按钮蓝 | `--pin-color` | `var(--color-primary)` |

**HomeView / NoteView：**

| 原始 | 组件变量 | 映射 |
|---|---|---|
| 搜索框底 | `--search-bg` | `var(--surface-2)` |
| 搜索框边框 | `--search-border` | `var(--border-default)` |
| 新建文本框边框 | `--form-border` | `color-mix(in srgb, var(--color-primary) 20%, transparent)` |
| 新建文本框 focus | `--form-focus-border` | `color-mix(in srgb, var(--color-primary) 40%, transparent)` |
| 提交按钮 | `--submit-bg` | `color-mix(in srgb, var(--color-primary) 15%, transparent)` |
| 提交按钮边框 | `--submit-border` | `color-mix(in srgb, var(--color-primary) 30%, transparent)` |
| 提交按钮字 | `--submit-color` | `var(--color-primary)` |
| Note 视图金色强调 | `--note-accent` | `var(--color-accent)` |
| 拖拽排序线 | `--drag-indicator` | `var(--color-primary)` |
| 空状态文字 | `--empty-color` | `var(--text-muted)` |
| 空状态提示 | `--empty-hint-color` | `color-mix(in srgb, var(--text-muted) 75%, transparent)` |

**Toast：**

| 原始 | 组件变量 | 映射 |
|---|---|---|
| toast 背景 | `--toast-bg` | `var(--surface-2)` |
| toast 边框 | `--toast-border` | `color-mix(in srgb, var(--color-primary) 30%, transparent)` |
| toast 文字 | `--toast-color` | `var(--color-primary)` |
| toast 撤销按钮 | `--toast-undo-color` | `var(--color-primary)` |
| toast 错误边框 | `--toast-error-border` | `color-mix(in srgb, var(--color-danger) 30%, transparent)` |
| toast 错误文字 | `--toast-error-color` | `var(--color-danger)` |

**SettingsView：**

| 原始 | 组件变量 | 映射 |
|---|---|---|
| 字体下拉背景 | `--dropdown-bg` | `var(--surface-2)` |
| 字体下拉高亮 | `--dropdown-hover-bg` | `color-mix(in srgb, var(--color-primary) 10%, transparent)` |
| 更新状态绿 | `--update-ok-color` | `var(--color-success)` |
| 更新状态蓝 | `--update-available-color` | `var(--color-primary)` |
| 更新状态红 | `--update-error-color` | `var(--color-danger)` |
| 重置按钮红 | `--reset-color` | `var(--color-danger)` |
| range accent | `--accent-slider` | `var(--color-primary)` |
| checkbox accent | `--accent-check` | `var(--color-primary)` |

**ImageEntryBody / FileEntryBody：**

| 原始 | 组件变量 | 映射 |
|---|---|---|
| 图片占位底 | `--placeholder-bg` | `var(--surface-2)` |
| meta-tag 底 | `--meta-bg` | `color-mix(in srgb, var(--text-primary) 5%, transparent)` |

---

## 2. 三套预设主题

### 深色玻璃（dark-glass）— 默认深色

强调色色相：天蓝

| Token | 值 |
|---|---|
| `--color-primary` | rgba(125, 211, 252, 0.9) |
| `--color-primary-light` | rgba(125, 211, 252, 0.6) |
| `--color-primary-faint` | rgba(125, 211, 252, 0.12) |
| `--color-accent` | rgba(251, 191, 36, 0.85) |
| `--color-danger` | rgba(248, 113, 113, 0.9) |
| `--color-success` | rgba(74, 222, 128, 0.8) |
| `--color-info` | rgba(192, 132, 252, 0.85) |
| `--color-file` | rgba(74, 222, 128, 0.85) |
| `--surface-0` | rgba(42, 53, 72, 0.85) |
| `--surface-1` | rgba(255, 255, 255, 0.04) |
| `--surface-2` | rgba(15, 23, 42, 0.5) |
| `--text-primary` | #e8edf5 |
| `--text-muted` | rgba(148, 163, 184, 0.5) |
| `--text-faint` | rgba(148, 163, 184, 0.35) |
| `--border-default` | rgba(148, 163, 184, 0.1) |
| `--border-subtle` | rgba(148, 163, 184, 0.08) |
| `--border-emphasis` | rgba(148, 163, 184, 0.25) |
| `--shadow-default` | 0 8px 32px rgba(0, 0, 0, 0.45) |

### 磨砂白底（light-matte）— 浅色方案 A

强调色色相：靛蓝。不透明度 78%，保留 blur。

| Token | 值 |
|---|---|
| `--color-primary` | rgba(37, 99, 235, 0.85) |
| `--color-primary-light` | rgba(37, 99, 235, 0.6) |
| `--color-primary-faint` | rgba(37, 99, 235, 0.08) |
| `--color-accent` | rgba(217, 119, 6, 0.85) |
| `--color-danger` | rgba(220, 38, 38, 0.85) |
| `--color-success` | rgba(22, 163, 74, 0.8) |
| `--color-info` | rgba(109, 40, 217, 0.75) |
| `--color-file` | rgba(22, 163, 74, 0.75) |
| `--surface-0` | rgba(250, 250, 252, 0.78) |
| `--surface-1` | rgba(255, 255, 255, 0.85) |
| `--surface-2` | rgba(0, 0, 0, 0.03) |
| `--text-primary` | rgba(30, 30, 30, 0.88) |
| `--text-muted` | rgba(60, 60, 60, 0.45) |
| `--text-faint` | rgba(60, 60, 60, 0.35) |
| `--border-default` | rgba(0, 0, 0, 0.07) |
| `--border-subtle` | rgba(0, 0, 0, 0.05) |
| `--border-emphasis` | rgba(0, 0, 0, 0.15) |
| `--shadow-default` | 0 4px 20px rgba(0, 0, 0, 0.12) |

### 半透明奶油（light-frosted）— 默认浅色

强调色色相：暖棕。不透明度 78%，保留 blur。

| Token | 值 |
|---|---|
| `--color-primary` | rgba(161, 98, 7, 0.8) |
| `--color-primary-light` | rgba(161, 98, 7, 0.6) |
| `--color-primary-faint` | rgba(161, 98, 7, 0.08) |
| `--color-accent` | rgba(180, 83, 9, 0.85) |
| `--color-danger` | rgba(185, 28, 28, 0.85) |
| `--color-success` | rgba(21, 128, 61, 0.8) |
| `--color-info` | rgba(147, 51, 234, 0.7) |
| `--color-file` | rgba(21, 128, 61, 0.7) |
| `--surface-0` | rgba(245, 243, 238, 0.78) |
| `--surface-1` | rgba(255, 255, 255, 0.55) |
| `--surface-2` | rgba(0, 0, 0, 0.04) |
| `--text-primary` | rgba(40, 35, 30, 0.88) |
| `--text-muted` | rgba(80, 70, 60, 0.45) |
| `--text-faint` | rgba(80, 70, 60, 0.35) |
| `--border-default` | rgba(0, 0, 0, 0.06) |
| `--border-subtle` | rgba(0, 0, 0, 0.05) |
| `--border-emphasis` | rgba(0, 0, 0, 0.12) |
| `--shadow-default` | 0 4px 24px rgba(0, 0, 0, 0.1) |

### 间距档位

| 档位 | --space-sm | --space-md | --space-lg |
|---|---|---|---|
| 紧凑 | 0.15rem | 0.25rem | 0.4rem |
| 标准 | 0.2rem | 0.35rem | 0.55rem |
| 宽松 | 0.25rem | 0.45rem | 0.7rem |

### 圆角档位

| 档位 | --radius-sm | --radius-md | --radius-lg |
|---|---|---|---|
| 锐利 | 0.125rem | 0.2rem | 0.3rem |
| 微圆 | 0.25rem | 0.35rem | 0.5rem |
| 圆润 | 0.375rem | 0.5rem | 0.75rem |

---

## 3. 主题状态机

### 数据模型

Rust 端 `DockPreferences` 主题相关字段：

```rust
// 主题模式（三态，互斥）
pub theme_mode: String,              // "system" | "preset" | "custom"

// 预设主题 ID（theme_mode = "preset" 或 "system" 时有效）
pub theme_preset_id: String,         // "dark-glass" | "light-matte" | "light-frosted"

// 自定义主题的基础预设（theme_mode = "custom" 时有效）
// 用户基于哪个预设修改的，用于"恢复此主题"功能
pub custom_base_preset_id: String,

// 用户对 token 的覆盖值
// 可包含：颜色 token、效果 token、布局 token 的任意组合
// key 为 token 名（如 "--space-sm"），value 为 CSS 值字符串
// 专家模式编辑的布局 token 也通过此字段持久化
pub theme_overrides: HashMap<String, String>,

// 布局设置（提供档位快速切换，作为基础值；被 theme_overrides 中的同 key 覆盖）
pub ui_text_size_px: u32,            // 默认 12
pub content_text_size_px: u32,       // 默认 14
pub spacing_preset: String,          // "compact" | "normal" | "spacious"
pub radius_preset: String,           // "sharp" | "normal" | "round"
```

**Token 合并优先级（高 → 低）：**

1. `theme_overrides` 中的逐项覆盖（用户通过专家模式或微调滑块设置）
2. `spacing_preset` / `radius_preset` 档位值（布局 token 的基础值）
3. `THEME_PRESETS[theme_preset_id].tokens`（颜色 token 的基础值）
4. 系统默认值（兜底）

移除的旧字段（由主题 token 替代）：
- `dock_bg_color` → `--surface-0`
- `dock_bg_opacity` → `--surface-0` alpha
- `text_color` → `--text-primary`
- `text_size_px` → `ui_text_size_px` + `content_text_size_px`
- `entry_surface_opacity` → `--surface-1` alpha

### 状态转换规则

```
初始状态：theme_mode = "system"
  ↓
系统深色 → 自动选择 dark-glass
系统浅色 → 自动选择 light-frosted

用户点击预设卡片 → theme_mode = "preset", theme_preset_id = 选定值
用户拖动任何微调滑块 → theme_mode = "custom", custom_base_preset_id = 当前预设 ID
用户打开"跟随系统" → theme_mode = "system"，保留 theme_overrides 不清空

system → preset:  用户手动选主题，覆盖系统跟随
system → custom:  不允许直接跳转，必须先 preset 再 custom
preset → custom:  用户修改任何微调项，overrides 开始累积
custom → preset:  用户重新选择预设，清空 overrides
preset → system:  用户打开跟随系统，overrides 不清空（但处于 system 模式下 overrides 不生效）
custom → system:  用户打开跟随系统，保留 overrides（切回 custom 时可恢复）
```

**overrides 保留规则：** 切换到 system 模式时不清空 `theme_overrides`，这样用户在 system → custom → system → custom 来回切换时不会丢失自定义配置。system 模式下 overrides 被忽略（只用预设 token），但数据保留在存储中。

### 系统主题跟随生命周期管理

```typescript
onMount(() => {
  const mq = window.matchMedia('(prefers-color-scheme: dark)')

  function onSystemThemeChange(e: MediaQueryListEvent) {
    if (preferences.theme_mode === 'system') {
      applyPreset(e.matches ? 'dark-glass' : 'light-frosted')
    }
  }

  mq.addEventListener('change', onSystemThemeChange)

  // 启动时根据系统状态初始化
  if (preferences.theme_mode === 'system') {
    applyPreset(mq.matches ? 'dark-glass' : 'light-frosted')
  }

  return () => mq.removeEventListener('change', onSystemThemeChange)
})
```

---

## 4. 前端主题引擎

### 预设主题定义

新建 `src/lib/themes/presets.ts`：

```typescript
export interface ThemePreset {
  id: string
  name: string
  tokens: Record<string, string>  // 完整 18 个颜色 token
}

export const THEME_PRESETS: Record<string, ThemePreset> = { ... }
```

新建 `src/lib/themes/layout.ts`：

```typescript
export const SPACING_PRESETS = { compact: {...}, normal: {...}, spacious: {...} }
export const RADIUS_PRESETS = { sharp: {...}, normal: {...}, round: {...} }
```

### Token 合并函数

新建 `src/lib/themes/engine.ts`：

```typescript
export function computeThemeTokens(prefs: DockPreferences, systemDark: boolean): Record<string, string> {
  // 1. 根据 theme_mode 确定基础预设
  // 2. 合并 spacing/radius/font token
  // 3. 合并 theme_overrides（custom 模式）
  // 4. 返回完整 token map
}
```

此函数应编写纯函数单元测试（见第 8 节）。

### 主题应用管线

```
启动
  → Rust: ipc_get_preferences → DockPreferences
  → 前端: computeThemeTokens(prefs, systemDark) → 完整 token map
  → $effect(): 遍历 token map → root.style.setProperty(k, v)
  → CSS: 组件引用 var(--card-bg) → 自动响应
```

### 颜色选择器 + Alpha 处理

原生 `<input type="color">` 只支持 hex（无 alpha）。方案：

- **所有 17 个颜色 token** — 统一使用 `<input type="color">` + 透明度滑块
  - 即便深色主题的 `--text-primary` 是 `#e8edf5`（无 alpha），浅色主题中 `--text-primary` 可能是 `rgba(30, 30, 30, 0.88)`
  - 统一编辑器避免跨主题切换时丢失 alpha 信息
- **内部存储** — 组合为 `rgba(r, g, b, alpha)` 字符串存入 `theme_overrides`
- **`--shadow-default`** — 单独的文本输入框（box-shadow 格式），不使用颜色选择器

---

## 5. 设置页布局

窗口默认 360×640，设置页按折叠分组处理，避免过长。

### 分组结构（可折叠区块）

**主题（默认展开）：**
- 「跟随系统」开关 → 开启时隐藏预设卡片，关闭时显示
- 3 张主题预览卡片（色块缩略图 + 名称），点击切换
- 选中卡片有 `--color-primary` 边框高亮

**外观（默认展开）：**
- 背景透明度滑块（60%-95%）
- 卡片背景色选择器（color + alpha 分离）
- 强调色选择器
- 文字颜色选择器
- 间距三档：紧凑 / 标准 / 宽松
- 圆角三档：锐利 / 微圆 / 圆润
- 界面字号滑块（10-16px）
- 内容字号滑块（12-20px）

**字体（默认折叠）：**
- 中文字体选择
- 英文字体选择

**更新（默认折叠）：**
- 代理 IP 输入框 + 端口输入框（保存/清除按钮）
- 检查更新按钮 + 版本信息

**高级（默认折叠）：**
- 开机自启开关
- 专家模式开关 → 展开后显示完整 24 个可编辑 token 列表
  - 17 个颜色 token：色块预览 + hex 输入 + 透明度滑块（统一格式）
  - 1 个效果 token（`--shadow-default`）：文本输入框
  - 6 个布局 token（`--space-sm/md/lg`、`--radius-sm/md/lg`）：数值输入框
  - 每个 token 旁有「恢复此项默认」按钮（移除 theme_overrides 中对应 key，回到档位/预设默认值）
  - 每个 token 旁有「恢复此项默认」按钮
  - 非法值校验（见第 6 节）

### 重置按钮（三档）

位于高级区底部：
- 「重置当前主题」— 回到当前预设的默认 token（custom → 清空 overrides 回到 base preset）
- 「重置所有外观」— 重置主题 + 字号 + 间距 + 圆角到默认值
- 「重置全部设置」— 重置外观 + 字体 + 开机自启 + 更新代理（等同于恢复出厂）

---

## 6. 保存策略

### 防抖机制

- 所有外观微调滑块和 token 输入的变更：**即时预览**（CSS 变量立即更新），**300ms 防抖写库**
- 系统副作用字段（`launchOnStartup`、更新代理）：**立即写库**，不走防抖
- 主题预设切换：**立即写库**（非高频操作）

**关键约束：** Rust 端 `set_preferences` 是全量覆盖写入（DELETE + INSERT），所以定时器不能闭包捕获旧的 `next` 快照——否则会覆盖在防抖窗口内发生的其他字段变更。

实现方式：定时器始终 flush **当前最新的 `preferences` 状态**，而不是闭包中的快照。

```typescript
let saveTimer: ReturnType<typeof setTimeout> | null = null

// 外观字段变更：即时预览 + 防抖写库
function scheduleAppearanceSave(partial: Partial<DockPreferences>) {
  const next = { ...preferences, ...partial }
  preferences = next  // 即时预览
  if (saveTimer) clearTimeout(saveTimer)
  saveTimer = setTimeout(async () => {
    await dockApi.setPreferences(preferences)  // flush 当前最新状态
  }, 300)
}

// 系统副作用字段：立即写库
async function saveNow(partial: Partial<DockPreferences>) {
  const next = { ...preferences, ...partial }
  preferences = next
  // 如果有 pending 的防抖保存，先 flush 掉，避免后续覆盖
  if (saveTimer) { clearTimeout(saveTimer); saveTimer = null }
  await dockApi.setPreferences(next)
}
```

### Token 校验与兜底

专家模式输入需要校验：

```typescript
const TOKEN_SCHEMA: Record<string, { type: 'color' | 'length' | 'shadow' | 'number'; min?: number; max?: number }> = {
  '--color-primary':   { type: 'color' },
  '--surface-0':       { type: 'color' },
  '--space-sm':        { type: 'length', min: 0.05, max: 1.0 },
  '--radius-md':       { type: 'length', min: 0, max: 2.0 },
  '--shadow-default':  { type: 'shadow' },
  // ...
}
```

校验规则：
- `color` 类型：尝试解析为 CSS 颜色，失败则拒绝并回滚到旧值
- `length` 类型：解析数值，检查 min/max 范围
- `shadow` 类型：正则校验 box-shadow 格式
- 任何非法值：输入框标红，不应用，显示提示文字

---

## 7. 浅色主题对比度验证清单

浅色主题上线前必须逐项验证以下状态：

| 检查项 | 深色/浅色均需验证 |
|---|---|
| 普通文字 vs 卡片背景 | WCAG AA (4.5:1) |
| 弱文字 vs 卡片背景 | 至少 3:1 |
| placeholder vs 输入框背景 | 至少 2.5:1 |
| badge 文字 vs badge 背景 | 至少 3:1 |
| hover 态 vs 正常态 | 视觉可区分 |
| active/选中态 vs hover 态 | 视觉可区分 |
| disabled 态 | 明显弱化 |
| toast 成功/错误 | 在两种 surface 上均可读 |
| 图片无 preview 时的占位区边界 | 可见 |
| 拖拽排序指示线 | 在 surface-0 和 surface-1 上均可见 |
| minimized 模式图标 | 无需 token，但需确认不受影响 |
| 滚动条 | 在两种背景下可拖拽 |

---

## 8. 验证策略

### 单元测试

测试框架：**vitest**（Vite 原生集成，零配置支持 TypeScript）

安装：`pnpm add -D vitest`

测试文件：`src/lib/themes/__tests__/engine.test.ts`

- `computeThemeTokens(prefs, systemDark)` 纯函数测试：
  - `theme_mode = "system"` + 系统深色 → 返回 dark-glass token
  - `theme_mode = "system"` + 系统浅色 → 返回 light-frosted token
  - `theme_mode = "preset"` → 返回指定预设 token，忽略系统状态
  - `theme_mode = "custom"` → 基础 preset + overrides 合并正确
  - overrides 中的非法值被忽略，使用基础值
  - layout token（spacing/radius）在 theme_overrides 中的值优先于档位值
  - spacing/radius/font 派生 token 计算正确

### Rust 端测试

- `DockPreferences` roundtrip：写入 → 读取 → 字段一致
- `theme_overrides` HashMap 序列化/反序列化正确

### 前端静态检查

- `pnpm check` 通过（TypeScript + Svelte 类型检查）

### 手动测试

- 三套预设主题切换：视觉无残留硬编码色
- 浅色/深色下拖动所有滑块：UI 即时响应，无闪烁
- 设置页滑块拖动后检查 Rust 端日志：300ms 内仅一次写库
- 系统深浅切换：自动跟随模式正确切换，手动锁定模式不受影响
- 专家模式输入非法值：输入框标红，不应用
- 重置功能三级：分别验证范围正确

---

## 9. 清理项

- 删除 `src/main.js` 和 `src/main.js.map`（残留编译产物）
- 在 `tsconfig.json` 中确认 outDir 不输出到 `src/`

---

## 10. 后续优化 Backlog

以下问题在 token 化完成后作为独立任务处理：

1. 搜索清除按钮 "x" → `×` SVG 图标
2. 操作按钮点击区域从 1.6rem 放大到 1.8rem
3. 卡片 hover 增加 background 变化或微位移
4. 视图切换加入 fade/slide 过渡动画
5. 空状态增加淡灰 SVG 插图
6. 滚动条宽度从 3px 优化到 4-5px 或 hover 展开
