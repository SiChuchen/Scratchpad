# Soma Scratchpad

Soma Scratchpad 是一个 Windows 桌面工作区，用于收集临时资料，并将有价值的信息整理为可搜索的本地资料库。它面向 AI 辅助工作流，避免把截图、复制的文件、笔记、凭据或路径直接堆进项目仓库。

[下载最新版本](https://github.com/SiChuchen/Scratchpad/releases/latest) · [English README](README.md) · [MIT License](LICENSE)

## 当前功能

### 统一内容工作区

- 将文字、截图和从资源管理器复制的文件粘贴到桌面暂存区。
- 支持直接拖入文件，也可以创建和编辑文本条目。
- 在一个工作区中浏览**收纳**（临时）、**收藏**（长期）和**全部**内容。
- 统一搜索文本、图片、文件、凭据、书签和笔记，并按类型筛选。
- 支持重命名、编辑文本与结构化笔记、在可排序页面调整顺序，以及撤销受支持的删除操作。
- 可以打开图片或文件所在目录；图片和文件既可复制内容，也可复制路径，便于粘贴到终端或 AI 工具。
- 在设置中将全部收纳与资料库内容一键导出为 **Excel (.xlsx)**、**CSV**、**Markdown** 或 **JSON**，便于备份与迁移；敏感字段默认以 `******` 脱敏，可按需包含原文。

### 不污染项目仓库的临时中转

- 新粘贴或拖入的内容默认进入临时**收纳**。
- 收藏后长期保留；不再需要时可恢复为临时内容。
- 可配置临时内容的自动清理策略。
- 图片和导入文件会复制到应用自己的数据目录，而不是工作项目目录。

### 全局快捷入口

- 通过可配置的全局快捷键打开独立的录入与搜索窗口，默认是 `Alt+Shift+Space`。
- **记录模式**先立即进行本地解析，再按设置选择是否由 AI 增强整理。
- 录入内容会形成可编辑的**凭据**、**书签**或**笔记**草稿，保存前可修改所有字段。
- **搜索模式**检索统一工作区，预览选中内容，复制字段或路径，并可跳转主窗口继续管理。
- `Ctrl+Tab` 在记录和搜索间切换；`Ctrl+Enter` 保存录入；`Escape` 隐藏快捷入口。

### 本地结构化资料库

资料库可用于保存运维笔记、Runbook、服务入口、连接信息、书签和凭据。

- 每个字段都有明确的敏感/非敏感标记。
- 敏感值会按需要在界面中隐藏；可配置复制后自动清空剪贴板；敏感字段不会进入可搜索投影。
- 本地解析支持连接 URL、SSH 命令、`user:password@host`、单一 URL、多行键值对，以及中英文混合的部署清单。
- 即使 AI 不可用，本地草稿仍然可以编辑和保存。

### 可选的 AI 整理与搜索

AI 是可选能力。在设置中配置服务商和模型即可使用；内置预设包含 DeepSeek、OpenAI、Kimi、智谱、通义、OpenRouter，以及自定义 OpenAI 兼容端点。

- 自动整理会生成标题、类型、字段、标签、摘要和搜索别名。
- 结构化提取会保留文本中明确出现的 URL、路径、IP、端口、版本、邮箱和敏感占位符，而不是把它们压缩成一段说明文字。
- 自动混合搜索可由 AI 扩展查询计划，本地搜索始终是可用的降级路径。
- DeepSeek 的思考模式有独立开关，默认**关闭**，以获得稳定的结构化输出；复杂推理任务可按需打开。
- 输出截断时会以更高预算自动重试一次；认证、限流、超时、网络和截断错误会明确提示，同时不影响本地录入。

#### AI 数据处理

- API Key 保存在应用本地配置中，不会返回给前端；但目前不提供静态加密。
- 发送录入内容到 AI 前，已识别的敏感值会替换为请求级 `[SECRET:...]` 占位符。
- 应用会拒绝未知占位符，并阻止敏感值进入 AI 生成的标签、摘要和别名。
- 开启 AI 后，脱敏后的录入文本或搜索查询会发送给所配置的外部服务商。任何绝不能离开设备的数据都不应开启 AI 处理。

### 桌面行为与个性化设置

- 支持置顶、收起到屏幕边缘、系统托盘和开机自启。
- 主窗口和快捷入口分别拥有可配置的全局快捷键。
- 提供暗色玻璃、浅色磨砂、浅色冰砂和跟随系统主题。
- 支持分别设置中文/英文字体、即时切换界面语言、配置更新代理、切换数据目录和自动更新。

## 下载与安装

从 [GitHub Releases](https://github.com/SiChuchen/Scratchpad/releases/latest) 下载：

| 文件 | 用途 |
| --- | --- |
| `Soma_Scratchpad_x.y.z_Windows.exe` | NSIS 安装包，推荐大多数用户使用 |
| `Soma_Scratchpad_x.y.z_Windows.msi` | MSI 安装包 |
| `Soma_Scratchpad_x.y.z_Windows_Portable.zip` | 便携版，解压后直接运行 |

运行环境为 Windows 10 或更高版本。应用更新器会校验来自发布频道的签名更新元数据。

## 数据位置与备份

默认情况下，应用数据位于可执行文件同级的 `data/` 目录：

```text
<应用目录>/
  soma-scratchpad.exe
  data/
    scratchpad.sqlite3
    assets/
```

备份整个 `data/` 目录即可保留条目和附件。修改数据目录不会自动迁移已有数据。

## 本地开发

前置条件：Node.js、pnpm、Rust stable，以及 Tauri 2 在 Windows 上所需的开发环境。

```bash
pnpm install
pnpm tauri dev
```

常用校验命令：

```bash
pnpm check
pnpm test:unit
pnpm build

cd src-tauri
cargo test
cargo fmt --check
cargo clippy
```

构建 Windows 安装包和更新产物：

```bash
pnpm tauri build
```

构建产物位于 `src-tauri/target/release/bundle/`。

## 发布流程

推送 `v*` 标签后会触发发布工作流：构建带签名的 Windows 安装包、便携版和 `latest.json`，再发布到 GitHub Release。

发布前请先执行上述校验。AI 整理回归还可以额外运行：

```powershell
.\scripts\Invoke-AiOrganizationEvaluation.ps1 -DatabasePath <scratchpad.sqlite3 路径>
```

评测脚本只在运行时读取本机已保存的配置；脚本本身不包含 API Key，也不会将 API Key 上传到仓库。

## 技术栈

| 层级 | 技术 |
| --- | --- |
| 桌面运行时 | Tauri 2 |
| 后端 | Rust |
| 前端 | Svelte 5、TypeScript、Vite |
| 本地存储 | SQLite |
| 平台 | Windows 10+ |

## License

[MIT](LICENSE)
