# WinSuiteX · D 玻璃拟态无边框窗口 — 技术笔记

> 用途：把「GDI+ 玻璃拟态 + 单 exe 零依赖 + 自建标题拖拽 + RDP 可见性」整套可复用经验固化下来，
> 供后续做新 skill 前先 review。本文档本身**不是最终 skill**，只是素材。
> 主交付物：`glass_showcase.cpp`（源码）、`WinSuiteX_glass4.exe`（产物）、`verify_*.py`（验证脚本）。

---

## 1. 一句话结论
- 无边框走 **`WS_POPUP | WS_VISIBLE` + `WS_EX_APPWINDOW` + 圆角 `SetWindowRgn`**（已验证 RDP 可见）。
- 之前 `WS_OVERLAPPEDWINDOW + WM_NCCALCSIZE` 会出现**双层标题栏/裁剪**，已弃。
- 拖拽必须**自建**（系统默认 caption 拖拽循环依赖前台窗口，RDP 下失灵），且坐标**一律用消息 lParam**。

## 2. 编译命令（MinGW-w64，单文件零依赖）
```bash
g++ -O2 -Wall -mwindows -finput-charset=UTF-8 -DUNICODE -D_UNICODE \
    glass_showcase.cpp -o WinSuiteX_glass4.exe -lgdiplus -static -s
```
- `-mwindows`：自动链 user32/gdi32/kernel32 + 子系统 windows。
- `-lgdiplus`：唯一额外系统 DLL（gdiplus.dll 系统自带，无需附带）。
- `-static -s`：静态 libgcc/libstdc++、剥离符号。

## 3. 无边框方案
| 项 | 做法 |
|---|---|
| 窗口样式 | `WS_POPUP \| WS_VISIBLE`，扩展 `WS_EX_APPWINDOW`（保证任务栏出现） |
| 圆角 | `CreateRoundRectRgn(0,0,WIN_W,WIN_H,16,16)` + `SetWindowRgn(hWnd,hr,TRUE)`（8px 圆角 → 16 直径） |
| 硬指标 | 窗口 600×718、客户区 600×718、style 无 `WS_CAPTION` |

> ⚠️ `WS_POPUP` 无 NC 区，`WM_NCCALCSIZE` / `WM_NCPAINT` 不会被发送，属死代码；保留仅作"切回标准窗口"备注，实际可删。

## 4. 防闪烁四联修（用户原话："控件上鼠标移动时闪烁"）
1. `wc.hbrBackground = NULL` + `WM_ERASEBKGND return 1`：不擦背景，避免两遍重绘。
2. **双缓冲 back buffer 复用**：`g_memDC/g_memBmp/g_oldBmp` 文件级静态，仅在 `WM_PAINT` 首次创建，`WM_DESTROY` 调 `ReleaseBackBuffer()`（先 `SelectObject` 恢复旧对象再 `DeleteObject`/`DeleteDC`）。
3. **阴影位图缓存** `g_shadow[4][2]`：`WM_CREATE` 用 `BuildShadows()` 预生成（盒式模糊近似高斯，~670×170），Render 只切换不重建，消除每帧模糊卡顿。
4. `WM_SETCURSOR`：锁定 `SetCursor(LoadCursor(NULL,IDC_ARROW))` 并 `return TRUE`，杜绝指针在标题/控件间穿行时的光标抖动。

## 5. 自建标题拖拽
- `WM_NCHITTEST`：标题区（y<TITLE_H）命中 close/min 按钮 → `HTCLIENT`（交 `WM_LBUTTONDOWN` 自处理）；其余标题空白 → `HTCAPTION`。
- `WM_NCLBUTTONDOWN(wp==HTCAPTION)`：从 `lParam` 取**屏幕坐标**记 `g_dragStartX/Y`，记当前窗口左上角 `g_dragWinX/Y`，`SetCapture(hWnd)`，`g_drag=true`。
- `WM_MOUSEMOVE(g_drag)`：从 `lParam` 取**客户坐标**，加当前窗口左上角还原屏幕坐标，按 `(当前屏幕 - 起始屏幕)` 位移 `SetWindowPos(...,SWP_NOZORDER|SWP_NOSIZE|SWP_NOACTIVATE)`。
- `WM_LBUTTONUP` / `WM_NCLBUTTONUP`：`g_drag=false; ReleaseCapture();`
- **`WM_CAPTURECHANGED`：`g_drag=false; return 0;`（勿再 `ReleaseCapture`，系统已释放）** —— 防捕获被 Alt-Tab/Win+M 夺走后窗口黏鼠标软锁死。

## 6. RDP / 自动化环境三大坑（最重要）
1. **`mouse_event` 注入在 RDP 下被前台门控拦截**：目标窗口（`SetForegroundWindow` 被拦）收不到 `WM_NCLBUTTONDOWN` → 拖拽恒 (0,0)。**验证改用 `SendMessage` 直发 `hwnd`**（不受前台门控限制，且走同一代码路径）。
2. **`GetCursorPos` 在 RDP 下坐标对不上**：`SetCursorPos`/`GetCursorPos` 对不上，依赖真实光标的拖拽算法非确定性失灵。**坐标一律从消息 `lParam` 取**。
3. **`SetCapture` 返回值不能门控 `g_drag`**：跨进程 `SendMessage` 触发的窗口过程里 `SetCapture` 会返回 `NULL`（真实前台场景不会），据此置 `false` 会假阴性卡死拖拽。**做法：无条件 `g_drag=true`，安全网交给 `WM_CAPTURECHANGED`**。

## 7. 确定性验证三件套（headless 必过）
- `verify_glass.py`：结构验证 — 读 `GWL_STYLE` / window rect / client rect，判 `client==600x718 且无 WS_CAPTION`。
- `verify_drag.py`：`SendMessage` 直发 `WM_NCLBUTTONDOWN(lParam=屏幕坐标) → 6×WM_MOUSEMOVE(lParam=客户坐标) → WM_LBUTTONUP`；每步把"期望屏幕坐标"减当前窗口左上角换算成客户坐标。用 `SetProp(hWnd,"GlassDragMoves",计数)` 暴露移动次数，断言 `g_dragMoves>0` 证明**自绘分支**在执行（而非系统默认 caption 拖拽兜底）。
- `verify_buttons.py`：`SendMessage(WM_LBUTTONDOWN, 客户坐标)` 到 min(536,28)/close(576,28) 中心，判 `IsIconic` / `IsWindow==0`。
- 三者均 `try/finally` 保证终止子进程（避免孤儿窗口干扰后续测试）。

## 8. 交付前对抗审查清单（多子 agent）
从三个独立角度各派一个只读审查 agent：① 拖拽逻辑正确性（坐标符号/竞态/多屏/DPI/按钮冲突）；② 全文件回归与闪烁源（泄漏/无边框正确性/命中对齐）；③ 验证脚本严谨性（假绿/坐标换算/覆盖盲区）。本轮真缺陷：`WM_CAPTURECHANGED` 缺失、`SetCapture` 误门控、`WM_DESTROY` 未释放阴影/字体。修复后三件套复验全绿。

## 9. 待确认 / 备选产物
- **需 Z.W. 本机确认**：鼠标在控件上移动**不再闪烁**（RDP 截图采不到，靠肉眼）。
- 备选产物（上一段验证用，保留作对照，未删）：`glass_showcase_ovl.cpp`（WS_OVERLAPPED+NCALCSIZE 冻存版）、`WinSuiteX_glass5.exe`（其重叠窗口路线经验证客户区也=600×718，仅残留无害 `WS_CAPTION` 位）。
## 10. 内存优化（阴影去重）
- **现象**：headless 实测 PrivateUsage 9.05MB / WorkingSet 20.61MB，体感"偏高"。
- **根因**：`g_shadow[4][2]` 为每张卡各存 rest/hover 两张阴影位图，共 8 张（~570KB/张，合计 ~4.5MB）。但 4 张卡几何完全相同（仅 Y 不同），模板可共用。
- **改法**：`g_shadow[4][2]` → `g_shadow[2]`（0=常态/1=hover）。`BuildShadows` 仅用 `CardRect(0)` 生成两套；`Render` 按各卡 Y 偏移 `DrawImage`；`WM_DESTROY` 对应改释放。外观/行为零变化。
- **实测**：PrivateUsage 9.05→5.50MB（−39%），WorkingSet 20.61→17.15MB（−17%）；三条验证仍全绿。
- **余量**：剩余 ~5.5MB 主要是 1.7MB back buffer（双缓冲必需）+ GDI+/C++ 运行时。再压需放弃 `-static`（牺牲便携性）或改渲染架构，性价比低。
- 新增 `measure_mem.py`（PSAPI 读 WorkingSet/PrivateUsage，结束必终止子进程）作为内存回归基准。

## 11. skill 化
待本文档 review 后再定（候选名 `djjz-gdi-glass-borderless`，含编译命令 + 三线验证 + 对抗审查清单 + RDP 三坑 + 内存去重）。
