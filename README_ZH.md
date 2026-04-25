<div align="center">

# Soma Scratchpad

**一个为 AI 编程协作设计的 Windows 桌面中转站**

<img src="docs/assets/readme/hero.png" alt="Soma Scratchpad 主界面" width="520" />

[![GitHub Release](https://img.shields.io/github/v/release/SiChuchen/Scratchpad?style=flat-square&color=6e40c9)](https://github.com/SiChuchen/Scratchpad/releases/latest)
[![Windows](https://img.shields.io/badge/platform-Windows%2010+-0078D4?style=flat-square)](https://github.com/SiChuchen/Scratchpad/releases/latest)

[**下载最新版**](https://github.com/SiChuchen/Scratchpad/releases/latest) &nbsp;·&nbsp; [功能预览](#核心功能) &nbsp;·&nbsp; [使用方式](#使用方式) &nbsp;·&nbsp; [本地开发](#本地开发)

文字、截图、文件，`Ctrl+V` 即可临时收纳；需要时一键复制内容或路径，不再让临时文件污染桌面和 Git 仓库。

[English](README.md)

</div>

---

## 为什么需要它

用 Claude Code、Codex、Cursor、ChatGPT 这类 AI 编程助手时，你是否也遇到过这些问题：

- 报错截图随手放进项目目录，结果被 AI 一起提交到 Git，仓库里全是 `screenshot-xxx.png`
- 给 AI 的长文本提示词需要反复修改，但聊天框里文本太长只能缩略显示，只能复制出来改完再粘贴回去
- 临时代码片段、日志片段、链接、文件路径散落在桌面和剪贴板，找不到也理不清
- 想把图片或文件交给 AI 分析，但不想让它出现在工作目录里

Soma Scratchpad 就是为此设计的：一个**悬浮在桌面最上层的临时中转站**，随时可用，用完即走。

---

## 核心功能

### 文本收纳

粘贴文本后自动生成摘要标题，支持手动重命名，收起状态下一眼就知道每条内容是什么。展开后可直接编辑文本，改完一键复制，不需要在窗口之间来回切换。

### 图片收纳

截图后 `Ctrl+V` 直接导入，图片存储在中转站的独立目录中。支持两种复制模式：
- **复制图片内容** — 直接粘贴到聊天框、文档等任意位置
- **复制图片路径** — 将本地路径粘贴给 AI 读取

图片不会被放进你的项目目录，不会意外被 Git 提交。

### 文件收纳

在资源管理器中 `Ctrl+C` 复制文件，在窗口中 `Ctrl+V` 导入；也支持从资源管理器直接拖拽文件到窗口。导入后的文件同样支持复制内容或复制路径。

### 收纳 / 收藏 / 全部

| 页面 | 用途 |
|------|------|
| **收纳** | 临时暂存区，每次启动时自动清空未收藏的条目 |
| **收藏** | 点一下收藏按钮即可长期保留，关机重启也不会丢 |
| **全部** | 按时间浏览所有条目，支持按「文本 / 图片 / 文件」分类筛选 |

### 桌面常驻

- **置顶模式** — 点一下置顶按钮始终显示在最前面，再点一下取消，不挡视线
- **最小化** — 收起为桌面边缘的小图标（一只猫），需要时点一下即恢复
- **全局快捷键** — `Alt+Shift+V` 随时切换窗口显示
- **系统托盘** — 后台静默运行，通过托盘图标控制

### 个性化设置

- **三套主题** — 暗色玻璃 / 浅色磨砂 / 浅色冰砂，支持跟随系统自动切换
- **字体设置** — 中文字体和英文字体分别配置
- **中英文切换** — 界面语言即时切换，无需重启
- **代理更新** — 支持配置 HTTP / SOCKS5 代理检查和下载新版本
- **开机自启** — 可选开机自动启动，安静待命

---

## 截图预览

<p align="center">
  <img src="docs/assets/readme/text-edit.png" alt="文本编辑" width="240" />
  &nbsp;&nbsp;
  <img src="docs/assets/readme/image-file.png" alt="图片与文件" width="240" />
</p>

<p align="center">
  <em>文本编辑 &nbsp;&nbsp;|&nbsp;&nbsp; 图片与文件</em>
</p>

<p align="center">
  <img src="docs/assets/readme/categories.png" alt="分类筛选" width="240" />
  &nbsp;&nbsp;
  <img src="docs/assets/readme/settings.png" alt="设置面板" width="240" />
</p>

<p align="center">
  <em>全部 · 分类筛选 &nbsp;&nbsp;|&nbsp;&nbsp; 设置面板</em>
</p>

<p align="center">
  <img src="docs/assets/readme/minimized.png" alt="最小化 · 桌面猫咪" width="240" />
  &nbsp;&nbsp;
  <img src="public/app-icon-circle.png" alt="猫咪图标" width="80" />
</p>

<p align="center">
  <em>最小化状态 &nbsp;&nbsp;|&nbsp;&nbsp; 桌面猫咪</em>
</p>

---

## 下载与安装

前往 [GitHub Releases](https://github.com/SiChuchen/Scratchpad/releases/latest) 下载最新版本：

| 文件 | 说明 |
|------|------|
| `Soma Scratchpad_x.x.x_Windows.exe` | NSIS 安装包，推荐大多数用户使用 |
| `Soma Scratchpad_x.x.x_Windows.msi` | MSI 安装包 |
| `Soma Scratchpad_x.x.x_Windows_Portable.zip` | 免安装便携版，解压即用 |

应用安装后会自动检查更新。如果网络受限，可在设置中配置代理。

---

## 使用方式

1. **粘贴收集** — 复制文字、截图或文件后，在窗口中 `Ctrl+V` 创建条目
2. **编辑文本** — 展开文本条目，直接在编辑区修改内容
3. **重命名** — 点击条目标题即可修改，方便在收起状态下快速识别内容
4. **复制使用** — 点击复制按钮，文本复制内容，图片和文件支持复制内容或路径
5. **收藏保留** — 点击收藏按钮将条目转移到「收藏」页面长期保存
6. **最小化** — 点击最小化按钮收起到屏幕边缘，需要时点一下恢复

---

## 数据安全

- 所有数据存储在应用程序 exe 同级的 `data/` 目录中，不写入系统盘
- 数据库使用 SQLite 本地文件，不上传任何云端
- 图片和文件附件按日期存放在 `data/assets/YYYY-MM-DD/` 目录下
- 如需清理，直接删除 `data/` 目录即可

---

## 技术栈

| 层级 | 技术 |
|------|------|
| 框架 | [Tauri 2](https://v2.tauri.app/) |
| 后端 | Rust |
| 前端 | [Svelte 5](https://svelte.dev/) + TypeScript + Vite |
| 存储 | SQLite (rusqlite) |
| 平台 | Windows 10+ |

---

## 本地开发

```bash
# 安装前端依赖
pnpm install

# 启动开发模式（前端 + Rust 后端热重载）
pnpm tauri dev
```

前端类型检查：

```bash
pnpm check
```

Rust 测试：

```bash
cd src-tauri && cargo test
```

---

## 构建发布

```bash
pnpm tauri build
```

构建产物位于 `src-tauri/target/release/bundle/`。详细命令请以 `package.json` 中的 `scripts` 为准。

---

## Roadmap

- [ ]  更丰富的条目类型（链接预览、代码高亮）
- [ ]  键盘驱动的工作流优化
- [ ]  跨平台探索（macOS / Linux）

---

## License

[MIT](LICENSE)
