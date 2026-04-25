# Soma Scratchpad

一个轻量级的 Windows 桌面悬浮暂存坞，用于快速收集和管理文本片段、图片和文件。

## 功能

### 数据收集

- **文本粘贴** — 复制文字后在 dock 窗口中 Ctrl+V 直接创建条目
- **截图粘贴** — 截图后 Ctrl+V 自动保存为图片条目
- **文件粘贴** — 在资源管理器中 Ctrl+C 复制文件后 Ctrl+V 导入（doc、xlsx、pdf 等）
- **文件拖入** — 从资源管理器直接拖拽文件到 dock 窗口导入

### 内容管理

- **收纳** — 日常收集的所有条目，支持展开/折叠、重命名
- **全部** — 按时间排序浏览所有条目
- **收藏** — 收藏重要条目，独立管理
- **自动标题** — 文本条目自动生成摘要标题，自动检测代码内容
- **快速操作** — 复制内容、复制路径、收藏、删除

### 最小化标签

- **圆形标签** — 最小化后变为 48x48 圆形悬浮图标，贴近屏幕边缘
- **拖拽吸附** — 长按拖动到任意边缘松手自动吸附
- **完整露出** — 标签完整显示在屏幕工作区内，不会被遮挡
- **多显示器** — 基于当前显示器工作区计算位置

### 窗口控制

- **透明悬浮** — 无边框、始终置顶的半透明窗口
- **Ctrl+拖动** — 按住 Ctrl 键拖动窗口位置
- **全局快捷键** — Alt+Shift+V 快速切换窗口显示
- **系统托盘** — 后台运行，通过托盘图标控制显示

### 个性化设置

- **主题** — 内置 3 套主题预设，支持跟随系统
- **字体** — 中/英文字体分别设置
- **窗口** — 大小、位置、背景颜色和透明度
- **开机自启** — 支持开机自动启动
- **自动更新** — 支持检查和安装新版本

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

数据保存在应用程序 exe 同级的 `data/` 目录中，不写入系统盘。

| 路径 | 内容 |
|------|------|
| `<exe所在目录>/data/scratchpad.sqlite3` | 条目元数据（文本内容、折叠状态、视图归属等） |
| `<exe所在目录>/data/assets/YYYY-MM-DD/` | 图片和文件附件（按日期分目录） |

如需清理，直接删除 `data/` 目录即可。

## 项目结构

```
src/                          # Svelte 前端
  lib/api/dock.ts             # Tauri IPC 调用层
  lib/components/             # UI 组件
    entry/                    # 条目卡片（文本、图片、文件）
    views/                    # 视图（收纳、全部、收藏、设置）
  lib/state/                  # 前端状态管理
  lib/types/                  # TypeScript 类型定义
  App.svelte                  # 应用主入口，处理全局粘贴和拖放
  MinimizedApp.svelte         # 最小化标签入口

src-tauri/src/                # Rust 后端
  models/                     # 数据模型（entry, preferences）
  scratchpad/                 # 业务逻辑（storage, assets, preferences, clipboard）
  storage/                    # SQLite 连接和迁移
  system/                     # 系统功能（tab_controller, window, fonts）
  lib.rs                      # Tauri 命令注册
```
