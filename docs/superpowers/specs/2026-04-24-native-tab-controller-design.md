# 最小化圆形图标原生交互重构

日期：2026-04-24

## 根因

MinimizedApp.svelte 在 mousedown 时立即调用 `startDragging()`，把"点击恢复"和"拖动移动"绑死在同一条串行链路上。当 `startDragging()` 失败时，`restoreMain()` 也被一起短路。同时不存在长按阈值机制来区分短按和长按。圆形 region 的命中测试进一步加剧了不稳定。

修复方向：**将所有窗口交互下沉到 Rust/Win32 原生层，前端只负责展示。**

## 架构

### 双窗口保留

- `main`：主 dock 窗口
- `minimized-tab`：最小化圆形图标窗口

### 职责边界

| 层 | 职责 |
|----|------|
| 前端 | 图标展示、hover 视觉、最小化按钮触发 |
| Rust IPC | `ipc_dock_minimize_to_tab`（编排窗口操作） |
| Win32 子类 | 短按判定、长按判定、拖动、吸附、恢复 |

## 第 1 段：原生手势控制器

### 常量

```rust
const TAB_LONG_PRESS_TIMER_ID: usize = 1;
const LONG_PRESS_MS: u32 = 200;
const DEFAULT_HIDDEN_RATIO: f32 = 1.0 / 3.0;
const MAX_HIDDEN_RATIO: f32 = 1.0 / 2.0;
```

### 共享状态（AppState 内）

```rust
/// 主窗口最小化前的物理外框矩形
struct MainWindowGeometry {
    x: i32,
    y: i32,
    width: i32,
    height: i32,
}
```

在 `ipc_dock_minimize_to_tab` 时用 `GetWindowRect` 填充。恢复时直接用 `SetWindowPos` 回写。

### TabController

```rust
struct TabController {
    state: TabState,
    press_origin_screen: (i32, i32),  // 按下时鼠标屏幕坐标
    win_origin: (i32, i32),           // 按下时窗口左上角位置
    app: tauri::AppHandle,
}

enum TabState {
    Idle,
    Pressed,
    Dragging,
}
```

不长期存 anchor / hidden_ratio，拖动结束时即时计算吸附。

### HWND 目标

- 必须获取 `minimized-tab` 的**顶层宿主 HWND**
- 即 `app.get_webview_window("minimized-tab").hwnd()` 返回的值
- 不是 WebView2 的内部子窗口

### 子类安装

```rust
static SUBCLASS_INSTALLED: AtomicBool = AtomicBool::new(false);

pub fn install(app: &AppHandle, hwnd: HWND) {
    if SUBCLASS_INSTALLED.load(Ordering::SeqCst) {
        return; // 已安装，跳过
    }
    let controller = Box::new(TabController { ... });
    let ok = unsafe {
        SetWindowSubclass(hwnd, Some(subclass_proc), 0, Box::into_raw(controller) as usize)
    };
    if ok != 0 {
        // 安装成功，置位
        SUBCLASS_INSTALLED.store(true, Ordering::SeqCst);
    } else {
        // 安装失败，释放 controller，标记不变，下次可重试
        unsafe { drop(Box::from_raw(Box::into_raw(controller) as *mut TabController)); }
    }
}
```

关键：**先安装，仅在成功后置位**。失败时不锁死 AtomicBool，下次 minimize 仍可重试安装。

调用时机：`ipc_dock_minimize_to_tab` 里 show tab 之前。

### minimized-tab 窗口生命周期假设

- `minimized-tab` 在应用运行期间**不会被销毁重建**。它在 `tauri.conf.json` 中声明，Tauri 启动时创建，应用退出时销毁。
- 因此 `WM_NCDESTROY` 里的 `SUBCLASS_INSTALLED.store(false)` 只在应用退出时触发，不会在运行期被误触发。
- 如果未来改为动态创建/销毁 tab 窗口，`SUBCLASS_INSTALLED` 的 AtomicBool 方案需要改为按 HWND 跟踪。

### 消息处理（严格 match）

**WM_LBUTTONDOWN：**
- 记录 `press_origin_screen`（`GetCursorPos`）
- 记录 `win_origin`（`GetWindowRect`）
- `SetCapture(hwnd)`
- `SetTimer(hwnd, TAB_LONG_PRESS_TIMER_ID, LONG_PRESS_MS, None)`
- `state = Pressed`

**WM_TIMER：**
```
match state {
    Pressed => {
        KillTimer(hwnd, TAB_LONG_PRESS_TIMER_ID)
        state = Dragging
    }
    _ => {}
}
```
timer 只用于 Pressed → Dragging 的一次性切换。

**WM_MOUSEMOVE：**
```
match state {
    Dragging => {
        if GetAsyncKeyState(VK_LBUTTON) 最高位为 0 {
            // 左键已松开但没收到 WM_LBUTTONUP（异常路径）
            // 必须执行吸附，否则窗口停在拖动中的任意位置
            ReleaseCapture()
            state = Idle
            snap_pos = calc_snap_position(current_window_rect, work_rect, tab_size, computed_ratio)
            SetWindowPos(hwnd, snap_pos.0, snap_pos.1, SWP_NOSIZE | SWP_NOZORDER)
            return
        }
        pos = GetCursorPos()
        offset = (pos.x - press_origin.0, pos.y - press_origin.1)
        new_pos = (win_origin.0 + offset.0, win_origin.1 + offset.1)
        SetWindowPos(hwnd, new_pos.0, new_pos.1, SWP_NOSIZE | SWP_NOZORDER)
    }
    _ => DefSubclassProc(...)
}
```

异常退出路径也执行吸附，与 WM_LBUTTONUP 的 Dragging 分支保持一致。

**WM_LBUTTONUP：**
```
match state {
    Pressed => {
        KillTimer(hwnd, TAB_LONG_PRESS_TIMER_ID)
        ReleaseCapture()
        state = Idle
        restore_main_window(&app)
    }
    Dragging => {
        ReleaseCapture()
        state = Idle
        snap_pos = calc_snap_position(...)
        SetWindowPos(hwnd, snap_pos.0, snap_pos.1, SWP_NOSIZE | SWP_NOZORDER)
    }
    Idle => {}
}
```

**WM_CAPTURECHANGED：**
- `KillTimer`
- `state = Idle`

**WM_CANCELMODE：**
- `KillTimer`
- `ReleaseCapture`
- `state = Idle`

**WM_NCDESTROY：**
1. `KillTimer`
2. `ReleaseCapture`
3. `RemoveWindowSubclass`
4. `drop(Box::from_raw(data_ptr))`
5. `SUBCLASS_INSTALLED.store(false)`

## 第 2 段：最小化 / 恢复 / 吸附

### 真相源优先级

| 数据 | 主真相 | 兜底 |
|------|--------|------|
| 主窗口恢复 geometry | `AppState.main_geometry`（物理坐标，`GetWindowRect` 保存） | `DockPreferences`（逻辑坐标，需 × scale_factor） |
| 最小化时 geometry 同步 | `ipc_dock_minimize_to_tab` 同时写 `main_geometry` 和 `DockPreferences` | — |

所有恢复入口（子类 click、托盘菜单、快捷键）统一走 `restore_main_window()`，从 `main_geometry` 读。`DockPreferences` 只在 `main_geometry` 为 None 时兜底。

### 坐标系

**所有窗口控制统一用 Win32 物理像素坐标。** 不混用 Tauri 的 logical position/size。

- 读窗口位置：`GetWindowRect`
- 读显示器：`MonitorFromWindow` + `GetMonitorInfoW` → `rcWork`
- 移动窗口：`SetWindowPos`

### AppState 扩展

```rust
pub struct AppState {
    pub db: Mutex<Connection>,
    pub main_geometry: Mutex<Option<MainWindowGeometry>>,
}
```

### ipc_dock_minimize_to_tab

1. `GetWindowRect(main_hwnd)` → 存入 `main_geometry`（物理坐标，恢复主路径使用）
2. **同步到 DockPreferences**：将物理坐标转成逻辑坐标（`physical / scale_factor`），更新 `DockPreferences` 的 `dock_position_x/y/width/height` 并写入 DB。这确保兜底恢复路径和其他入口（托盘、快捷键）也能拿到最新的 geometry。
3. `MonitorFromWindow(main_hwnd, MONITOR_DEFAULTTONEAREST)` + `GetMonitorInfoW` → `rcWork`
4. 计算 main 中心到 `rcWork` 四边距离 → anchor
5. 读 tab 物理尺寸（`GetWindowRect(tab_hwnd)` 的 width/height）
6. 按 `DEFAULT_HIDDEN_RATIO` 计算吸附位置
7. 确保 tab 已安装 subclass（`SUBCLASS_INSTALLED` 防重复）
8. 确保 tab 已应用 circle region
9. `SetWindowPos` 定位 tab
10. `ShowWindow(tab_hwnd, SW_SHOWNOACTIVATE)`
11. `ShowWindow(main_hwnd, SW_HIDE)`

### 吸附算法

```rust
/// 纯函数，不依赖 Tauri 类型
fn calc_snap_position(
    window_rect: &RECT,
    work_rect: &RECT,
    tab_size: (i32, i32),
    hidden_ratio: f32,
) -> (i32, i32)
```

- 吸附目标是 `rcWork`（工作区），不是 `rcMonitor`
- 默认 `hidden_ratio = 1/3`
- 拖动结束时根据实际位置计算 ratio，clamp 到 `0..MAX_HIDDEN_RATIO`
- clamp 确保至少 `(1 - MAX_HIDDEN_RATIO)` 个图标可见

### restore_main_window(app)

1. 读 `main_geometry`
2. 有值 → `SetWindowPos` 恢复（主路径，物理坐标直接使用）
3. 无值 → 兜底路径：从 `DockPreferences` 读逻辑坐标，按以下规则转换：
   - DockPreferences 里的 `dock_position_x/y/width/height` 是 **Tauri 逻辑坐标**（前端 `outerPosition()` / `outerSize()` 写入）
   - 转换为物理坐标：`physical = logical × scale_factor`
   - scale_factor 取目标窗口的 DPI：`GetDpiForWindow(main_hwnd)` → `scale = dpi / 96.0`
   - 用 `SetWindowPos` 写入转换后的物理坐标
   - 如果 main_hwnd 也不可用（极端异常），不做恢复，打印错误日志
4. `ShowWindow(tab_hwnd, SW_HIDE)`
5. `ShowWindow(main_hwnd, SW_SHOWNORMAL)`
6. `SetForegroundWindow(main_hwnd)`

**主路径**：`main_geometry`（物理坐标，GetWindowRect 保存）
**兜底路径**：`DockPreferences`（逻辑坐标，需要 `× scale_factor` 转换）

注意：兜底路径只在应用启动后、尚未触发过最小化时才会走到。一旦用户触发过最小化，`main_geometry` 就会一直有值。

## 第 3 段：前端瘦身

### 硬原则

- MinimizedApp.svelte 只保留展示与 hover 视觉
- App.svelte 的 minimize() 只保留单个 IPC 调用
- 所有窗口几何、region、show/hide、恢复、吸附全部退出前端

### MinimizedApp.svelte 删除项

- `LONG_PRESS_THRESHOLD_MS` 常量
- `isPressed`、`isDragging`、`pressTimer` 状态变量
- `handleMouseDown`、`handleMouseUp`、`restoreMain` 函数
- `invoke` import
- `onMount` 里的 `invoke('ipc_window_apply_circle_region')`（region 由 Rust 统一负责）
- `onmousedown`、`onmouseup` 绑定
- 不再需要的 a11y svelte-ignore 注释

### MinimizedApp.svelte 保留项

- 透明样式初始化（`#app`、`body`、`html`）
- `handleMouseEnter` / `handleMouseLeave` / `scheduleAutoHide`
- `mouseenter` / `mouseleave` 绑定
- 图标 HTML + CSS（hover scale/brightness）

### App.svelte minimize() 简化

```typescript
async function minimize() {
  try {
    await invoke('ipc_dock_minimize_to_tab')
  } catch (e) {
    showToast(`最小化失败: ${formatError(e)}`, 'error')
  }
}
```

不再 import `getAllWindows`、`currentMonitor`、`LogicalPosition`。不再手动计算几何。

### 不改的文件

- `src/main.ts`
- `src/lib/state/window.ts`（后续可清理，本次不动）

## 第 4 段：Rust 依赖与模块结构

### Cargo.toml

```toml
windows-sys = { version = "0.59", features = [
    "Win32_Foundation",
    "Win32_Graphics_Gdi",
    "Win32_UI_WindowsAndMessaging",
    "Win32_UI_Shell",
    "Win32_UI_Input_KeyboardAndMouse",
] }
```

| feature | API |
|---------|-----|
| `Win32_Foundation` | `HWND`, `LRESULT`, `RECT`, `POINT` 等基础类型 |
| `Win32_Graphics_Gdi` | `MonitorFromWindow`, `GetMonitorInfoW`, `CreateEllipticRgn`, `SetWindowRgn`, `GetWindowRect` |
| `Win32_UI_WindowsAndMessaging` | `SetWindowPos`, `ShowWindow`, `SetForegroundWindow`, `SetTimer`, `KillTimer`, `GetCursorPos`, `WM_*` 常量, `SWP_*` / `SW_*` 常量 |
| `Win32_UI_Shell` | `SetWindowSubclass`, `DefSubclassProc`, `RemoveWindowSubclass` |
| `Win32_UI_Input_KeyboardAndMouse` | `SetCapture`, `ReleaseCapture`, `GetAsyncKeyState`, `VK_LBUTTON` |

### 模块职责

| 模块 | 职责 |
|------|------|
| `system/tab_controller.rs` | 状态机、窗口子类回调、WM_* 消息分发、拖动过程、`calc_snap_position()` 纯函数 |
| `system/window.rs` | `apply_circle_region`、`restore_main_window()`（show/hide/position/focus） |
| `lib.rs` | `AppState` 定义、IPC 命令注册、`ipc_dock_minimize_to_tab` 编排逻辑 |

### 文件变更清单

| 操作 | 文件 |
|------|------|
| 新增 | `src-tauri/src/system/tab_controller.rs` |
| 改 | `src-tauri/Cargo.toml` |
| 改 | `src-tauri/src/system/mod.rs` |
| 改 | `src-tauri/src/system/window.rs` |
| 改 | `src-tauri/src/lib.rs` |
| 改 | `src/MinimizedApp.svelte` |
| 改 | `src/App.svelte` |

## 验收标准

1. 点击最小化图标，主窗口能恢复
2. 长按最小化图标，能拖动
3. 拖动后松手，不会误恢复主窗口
4. 短按不会误触发拖动
5. 拖动结束后能正确吸附到最近边缘
6. 默认隐藏 1/3 生效
7. 手动往里推时最多隐藏 1/2
8. 多显示器下位置计算正确
9. 圆形窗口轮廓和命中区域一致
10. 没有新的控制台错误
11. `pnpm check` 通过
