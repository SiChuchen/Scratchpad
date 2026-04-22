# Soma Scratchpad

一个轻量级的 Windows 桌面悬浮暂存坞，用于快速收集和管理文本片段、图片和文件。

## 功能

### 数据收集

- **文本粘贴** — 复制文字后在 dock 窗口中 Ctrl+V 直接创建条目
- **截图粘贴** — 截图后 Ctrl+V 自动保存为图片条目
- **文件粘贴** — 在资源管理器中 Ctrl+C 复制文件后 Ctrl+V 导入（doc、xlsx、pdf 等）
- **文件拖入** — 从资源管理器直接拖拽文件到 dock 窗口导入
- **原生拖放** — 支持 Tauri 原生 drag-drop，获取真实文件路径

### 内容管理

- **Home 视图** — 日常收集的所有条目，支持展开/折叠
- **Note 视图** — 收藏重要条目，独立管理
- **Categories 视图** — 按 MIME 类型分类浏览所有条目
- **快速操作** — 复制内容、复制路径、收藏、删除（含撤销）

### 窗口控制

- **透明悬浮** — 无边框、始终置顶的半透明窗口
- **Ctrl+拖动** — 按住 Ctrl 键拖动窗口位置
- **最小化** — 收缩为屏幕边缘的小标签，自动半透明隐藏
- **系统托盘** — 后台运行，通过托盘图标控制显示

### 个性化设置

- 窗口大小、位置、背景颜色和透明度
- 文本字体（中/英文字体分别设置）和字号
- 开机自启动

## 技术栈

- **前端**: Svelte 5 + TypeScript + Vite
- **后端**: Tauri 2 (Rust)，SQLite (rusqlite)
- **平台**: Windows 10+

## 开发

```bash
pnpm install
pnpm tauri dev
```

## 构建

```bash
pnpm tauri build
```

## 数据存储

| 路径 | 内容 |
|------|------|
| `%LOCALAPPDATA%\Soma\scratchpad\scratchpad.sqlite3` | 条目元数据（文本内容、折叠状态、视图归属等） |
| `%LOCALAPPDATA%\Soma\scratch\images\YYYY-MM-DD\` | 图片和文件附件（按日期分目录） |

## 项目结构

```
src/                          # Svelte 前端
  lib/api/dock.ts             # Tauri IPC 调用层
  lib/components/             # UI 组件
    entry/                    # 条目卡片（文本、图片、文件）
    views/                    # 视图（Home、Note、Categories、Settings）
  lib/state/                  # 前端状态管理
  lib/types/                  # TypeScript 类型定义
  App.svelte                  # 应用主入口，处理全局粘贴和拖放

src-tauri/src/                # Rust 后端
  models/                     # 数据模型（entry, preferences）
  scratchpad/                 # 业务逻辑（storage, assets, preferences）
  storage/                    # SQLite 连接和迁移
  system/                     # 系统功能（字体列表）
  lib.rs                      # Tauri 命令注册
```
