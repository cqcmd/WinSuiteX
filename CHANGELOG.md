# Changelog

## Rust 重写线（2026-09 起 · 当前主推）

> **两条产品线并存，版本号独立编排**：
> - **C++ 版**（`dist/客户机/winsuitex.exe`，1.7.x 线）自 2026-09-26 起**冻结**，只作只读参照，不再接受新需求；其变更历史见下方 v1.x 各节。
> - **Rust 线**（本表）是唯一活跃线，产物为 `dist/winsuitex-gui-gdi.exe`（面板，0.4.x 线）+ `dist/winsuitex.exe`（命令行，0.1.0.x 线），与 C++ 的 1.7.x 无对应关系。
>
> 版本号四段式 `主.次.修订.编译日期戳(YYYYMMDD)`；技术形态为 Rust workspace（core / platform / report / gui-gdi / cli 五 crate），GUI 是纯 Rust + GDI 自绘单 exe（零 dll、零 GPU、≈830KB），面向网吧/电竞酒店单 exe 分发场景。

## GUI 0.5.2 — 2026-10-22（滚动错位修复 + 启动提速，分支 `gdi-aa-radius`）

Z.W. 两则真机反馈：「滚动竖条后内容界面有问题」（截图暗条盖字、SMART 徽标串到内存区）+「运行的时候，界面要等一阵才弹得出来」。

- **滚动错位根因**：`AaSurface::new/finish` 把 `GetViewportOrgEx` 的视口偏移加进 StretchBlt 坐标（误以为它吃设备坐标），但 StretchBlt 吃的是**逻辑坐标**、GDI 自己按视口映射——滚动量被算了两遍，AA 元素（玻璃卡/SMART 徽标/分组横条）整体上移一个 `scroll`，与 DrawTextW 的文字错位。scroll=0 时两路径等价，所以只有滚起来才暴露。修复：去掉补偿、直接传内容坐标（`aa_round` 本就正确，历史一致口径已写进注释）。
- **像素级回归** `aa_draws_at_logical_coords_under_viewport_offset`：视口偏移 60px 下在内容 y=100 处画白色圆角块，断言设备 y=60 亮 / 设备 y=5 保持黑底。A/B 对照——注入旧补偿后测试红（实测 BGR=(0,0,0)）、恢复后绿。
- **启动提速根因**：`collect()`（WMI/SMART 读取，秒级）同步卡在 `CreateWindowExW` 之前，双击 exe 到出窗全程白等。修复：窗口先显示加载页（`draw_loading_page`：「正在采集硬件信息…」），采集移后台线程（`collect_all` 仍单线程串行，COM 纪律不变），完成后 `PostMessageW(WM_APP_SNAPSHOT_READY)` 切正式内容。
- **实测**（`tools/scroll_align_probe.py`）：出窗耗时 **0.04s**（旧版秒级）；`WSX_FAKE_AUDIO=30` 撑出滚动后中段帧——SMART 徽标紧贴硬盘行、暗条消失、分组带与文字对齐，与 bug 截图直接对照。
- 门禁：fmt / clippy -D（默认+all-features）/ 364 测试全绿；verify_gui_resource PASS；部署 `dist/winsuitex-gui-gdi.exe`（0.5.2.20261022）。

## GUI 0.5.1 — 2026-10-22（磁盘页窗高震荡修复：ULW psize 与 SetWindowPos 互拽，分支 `gdi-aa-radius`）

Z.W.「现在是磁盘页面的时候，一直在短窗口和长窗口反复绘画」——0.5.0 引入分层窗口后暴露的**反馈环**，本次 A/B 实测复现并修复：

- **根因**：`on_paint` 的磁盘页自适应高度 `SetWindowPos` 发生在建 Backbuffer **之后**，而 `present_layered` 的 `UpdateLayeredWindow` 的 `psize` 语义是**窗口新尺寸**（传的是 bb 旧高）——每帧末尾 ULW 都把刚调高的窗口**拽回旧高**，触发 WM_SIZE → InvalidateRect → 重绘 → 再 SetWindowPos……短/高两尺寸无限往返（探针实测高度在 492↔950 间翻转、dump 每秒重写 7+ 次）。
- **修复（双保险）**：
  1. **顺序不变式**：`on_paint` 重排为「量高 → SetWindowPos 定窗高 → **重读客户区** → 用最终 w/h 建 Backbuffer → 绘制 → 呈现」，bb/窗口/ULW 三方尺寸同源，resize 后下一帧 `desired==h` 零动作天然收敛；
  2. **ULW 几何中立**：`present_layered` 的 `psize` 改传**当前窗口尺寸**而非 DIB 尺寸——ULW 只更新内容、永不改窗口几何，即使顺序不变式被打破也不会与 SetWindowPos 打架（尺寸不一致时留 wlog 痕迹）。
- **收敛根基钉死**：目标高度抽纯函数 `desired_win_height(total, tab, cap)`（只吃内容量、绝不读当前窗高），回归测试钉死 clamp 区间/非磁盘页恒 cap/幂等/对 total 单调——任何引入 `cur_h` 依赖的改动都会让测试外的集成路径重新震荡。
- **A/B 物证**（`tools/osc_probe.py`，SendMessageW 切磁盘页 + 紧循环采样 GetWindowRect + dump 重写计数）：
  - 修复前（HEAD 5cbb7c8）：基线 950 稳定 → 切页后 `{492: 378k, 950: 856k}` 双值翻转，1s 内 dump 重写 7 次 = **OSCILLATING**；
  - 修复后：切页后 `{492: 1254k}` 单值（收敛到内容适配高度），1s 内 dump 重写 0 = **STABLE**。
- 回归测试：`desired_win_height_is_content_only_and_clamped`；门禁：fmt / clippy -D（默认+all-features）/ 362 测试全绿；verify_gui_resource PASS；部署 `dist/winsuitex-gui-gdi.exe`（0.5.1.20261022）。

## GUI 0.5.0 — 2026-10-22（窗口圆角真羽化：分层窗口 per-pixel alpha，分支 `gdi-aa-radius`）

Z.W.「窗口圆角在深色背景下面看不太出来羽化，但在浅色背景下还是很明显」——`SetWindowRgn` 的 1-bit 形状掩码是天花板（深色壁纸把阶梯压住、浅色壁纸高对比炸出来），真 AA 只有分层窗口一条路能钉住 WIN_RADIUS=12（DWM 圆角半径固定 ~8px 且仅 Win11，已排除）：

- **呈现模型切换**：窗口创建加 `WS_EX_LAYERED`，`Backbuffer::present` 的 BitBlt 呈现改为 `present_layered`（`UpdateLayeredWindow` + `ULW_ALPHA` 预乘 ARGB）；`SetWindowRgn` 废除（1-bit region 会把 AA 角重新裁成锯齿，两者不能共存）。
- **SDF 羽化**（`draw::apply_round_alpha`）：iq 圆角矩形 SDF 覆盖度采样，边界两侧 1px 过渡带（只做覆盖度 AA 不做宽羽化——宽羽化在浅色壁纸下会发虚）；直边像素对齐全不透明，锯齿只出现在四角弧线。内缩 r+1 内接矩形走快路径（不进开方），每帧开销可忽略。最大化时方形满不透明（Win11 惯例）。
- **物证**（`WSX_DUMP_LAYERED` 裸转储 → Python 合成，`tools/layered_on_{white,black,lightgray}.png` + 角部 8x 放大）：弧线 alpha 呈 0→38→122→187→230→252→255 连续 SDF 梯度（角尖全透明、直边全不透明、过渡带占比 0.013%）；羽化像素反推直色 =(68,66,104)=`C_WIN_BORDER`，预乘精确。白底合成窗口边角无阶梯。
- **鲁棒性**：ULW 失败（个别精简系统/驱动）自动降级——去掉 `WS_EX_LAYERED` + 恢复 `SetWindowRgn` 1-bit 圆角，宁可回到锯齿也不能"窗口全透明看不见程序"；降级后工具镜像 blit 即真实呈现。
- **诊断钩子** `WSX_DUMP_LAYERED=<前缀>`：把羽化后的预乘 ARGB 裸转储（.bgra+.meta，写失败落 .err 不静默）——本 VM 屏幕读回（`GetDC(0)`/ImageGrab）与分层窗口 DC 读回均不可用，这份缓冲就是 ULW 提交的字节本身，离线按 `out = premul + bg*(1-a/255)` 可精确复现任意壁纸观感。
- 回归测试：`round_rect_sd_sign_and_magnitude`（SDF 符号/弧上过零）、`sd_coverage_transition_band`（1px 过渡带）、`apply_round_alpha_writes_premultiplied_feather`（角尖清零/直边不动/过渡带预乘/最大化全 255）。
- 门禁：fmt / clippy -D（默认+all-features）/ 361 测试全绿；verify_gui_resource PASS；部署 `dist/winsuitex-gui-gdi.exe`（0.5.0.20261022）。**待 Z.W. 真机浅色壁纸目检后合回 main**。

## GUI 0.4.9 — 2026-10-22（圆角超采样抗锯齿，分支 `gdi-aa-radius`）

Z.W.「主线的『圆角』还有一招没试过，试一下」→ 纯 GDI 的 3x 超采样 AA（环境无关，VM/RDP 照常）：

- **draw.rs 新增 `AaSurface`**：目标区当前背景先放大抠入 3x 离屏（渐变低频无可见损失）→ 前景以 3x 尺寸画圆角 → `StretchBlt(HALFTONE)` 缩回时区域平均自动生成边缘灰度过渡。`fill_round` / `stroke_round` / `fill_glass_round` 三函数内部升级，**调用点零改动**，全部卡片/按钮/玻璃卡自动受益。
- **准入与降级**：圆角 <3px 或边 <16px 过渡带不足 1px，走原路径（`*_raw`）；离屏像素超 24MB 降 2x、仍超或 `StretchBlt` 失败（个别远程会话禁屏幕读取）整体降级——任何环境不劣化。视口滚动偏移经 `GetViewportOrgEx` 自适应，调用方无需传 scroll。
- **scope 界定**：`SetWindowRgn` 窗口形状圆角是二值掩码，超采样无解（真 AA 只有 DWM/分层窗口，已评估在案）；本版治的是**绘制出来的**圆角。
- **量化对比**（`tools/aa_edge_compare.py`，三页 A/B）：微过渡（软）像素占比 menu 8.6%→26.5%、hw 7.2%→16.4%、disk 11.5%→33.0%；唯一色数 1.8~2.2x；硬跳变占比不变（文字边缘未受扰）。
- 回归测试：`aa_worthwhile_thresholds`（准入阈值）+ `aa_paths_never_panic_on_tiny_dc`（AA/降级/退化三路不崩）。
- 门禁：fmt / clippy -D / 全量测试通过；部署 `dist/winsuitex-gui-gdi.exe`（0.4.9.20261022）。



Z.W. 目检疑点「透明度50是否都统一调整的？右键增强里好像不太一样」：

- **全量审计**所有玻璃调用点，确认内容卡（深 navy `tint_color()` @ `glass_alpha()`）早已统一，白色蒙层（图标底/活动页签/section 头带/hover 行，`C_WHITE`@0.08~0.22）是有意的浅色提亮不属 50% 玻璃；**唯一漏网之鱼**是右键增强页 `feature_row` 卡单用 `C_GLASS_DARK`(64,60,96) @ 0.45（更浅更紫）。
- 修复：该处收敛到 `tint_color() + glass_alpha()`（白字对比度反而更高：6.7~7.5:1 vs 浅紫约 3.5:1）。
- 验证：VM 内真实截图，扫两页卡片内区 navy 像素直方图同族同档（(64~67,55~56,145)），与混合数学精确吻合（navy(20,24,52)@0.5 叠渐变 ≈ (66,55,145)）。
- 门禁：fmt / clippy -D / 355 测试全过；verify_gui_resource PASS。

## 羽化双修（窗口圆角描边 + 开关控件质感，两版通用）

- 旧窗口描边是亮薰衣草 (170,160,245)，勾在 `SetWindowRgn` 的 1-bit 锯齿上炸眼；且描边仅 1px、外缘主导色是亮渐变本身（实测外缘亮度 146，A 方案仅 101）。修：`C_WIN_BORDER`→(70,66,104) 暗靛 + 描边改画全窗矩形宽 scale(4)（GDI 画笔以路径为中心，外半边被 RGN/AA 离屏裁掉、内半边≈2px 才是可见暗环；描边矩形**不能内缩**——内缩到 x=0 的半笔会被 AA 离屏整个切掉）。修复后外缘 1px/2px 亮度 = 72/70，与 A 的 101/72 持平。
- 开关轨道/旋钮原本走 `aa_round`（3x 超采样），小尺寸高对比控件上 1px 灰度过渡读作"发虚"。新增 `draw::fill_round_crisp`（硬边 RoundRect），轨道+旋钮改硬边，阴影保留 AA 软影做深度。连带修出 `fill_round_raw` 潜伏 bug：未选 NULL_PEN，RoundRect 用 DC 残留画笔描出黑边——已补 NULL_PEN。
- 钉死值 `smoke_tests::theme_constants_regression` 同步 (70,66,104)；物证 `_shot_tmp/cmp_fix_corner.png` / `cmp_fix_switch.png` / `fix_*.png`。

## 双主题运行时一键切换（深空玻璃 ⇄ 流畅灰紫，本分支收尾动作）

Z.W. 对两版都满意、难取舍 → 不做二选一，把两套主题都编进同一 exe，面板底栏点一下即时换肤，选择持久化到 `%APPDATA%/WinSuiteX/theme.txt`，重启仍生效。

- `theme.rs` 从「编译期 `const` 颜色」重构为「`Theme` 结构体 + `DEEP_SPACE`/`FLUENT` 两个 `const` 实例 + 全局 `AtomicU8`(`CURRENT_THEME`) + `theme()` 访问器」；几何常量（窗口尺寸/圆角/行高）两套相同，保持 `const` 不变，`CARD_RADIUS` 纳入 `Theme`（深空 8 / 流畅 10）。
- 124 处颜色/`CARD_RADIUS` 常量调用点经脚本按名称降序替换为 `theme().XXX`（pages 106 / draw 15 / icons 1 / main 2），调用点零语义改动；`grad_at`/`tint_color`/`glass_alpha` 改读 `theme()`。
- 底栏新增切换按钮（`draw_footer` 画 "主题：XXX ▸"）→ `pages::theme_btn_rect` 命中区 → `main::on_click` 命中后 `set_theme(1-id)+save_theme+InvalidateRect`；`theme_constants_regression` 重写为双主题校验（DEEP_SPACE 12 色 + FLUENT 6 色基线 + 两套 GLASS_ALPHA==0.50 + 默认 `tint_color()==DEEP_SPACE.C_GLASS_TINT`）。
- 验证：VM 内 `tools/gdi_interact.py` 发 `SendMessageW` 点击 footer 按钮坐标触发切换，生成深空/流畅两套三页截图（`_shot_tmp/sw_deep_*.png` / `sw_fluent_*.png`）与并排对比图（`_shot_tmp/sw_cmp_*.png`）；像素证据确认两版均为 AA 圆角 + 各自色板，差异集中在背景梯度与卡片色调。
- 门禁：fmt / clippy -D / 全量测试通过；verify_gui_resource PASS；dist 已部署运行（0.4.9.20261022）。

## 主题按钮移到标题栏（Z.W. 反馈「放下面后两排字错位」）

- 底栏按钮与版权行挤在一行导致 hint/版权两排字错位 → 切换按钮**移到标题栏右上角**（最小化键左侧，`theme_btn_rect` 重定位，与窗口按钮同层同几何）；hover 走 `hover_tb()==2`（`hit_test` 标题栏分支只判 x 范围、由 `y<TITLE_H` 把关，滚动偏移不再影响 hover）。
- 底栏还原为两行满宽居中（提示行 + 版权行），错位消除。
- `draw_titlebar` 增加 `f_tiny` 形参（两处调用点 + smoke_tests 同步）；VM 重截双主题三页（`_shot_tmp/tb_deep_*` / `tb_fluent_*` / `tb_cmp_*.png`）确认按钮位置与底栏两行对齐。

## 主题按钮移到页签行 + 真实点击修复（Z.W. 反馈「放右上角后点击不生效、点最小化关闭反而是切主题」）

Z.W. 二次反馈标题栏版有两个问题：①按钮"点击不生效"，反而点最小化/关闭像切主题；②按钮配色"比较突兀"。定位到**根因是 NC 命中测试吞点击**：

- **点击不生效根因**：标题栏按钮落在 `nc_hit_test` 的 `HTCAPTION`（拖动）区——`nc_hit_test` 只对 `x >= right - bw*2`（最小化/关闭两键）返回 `HTCLIENT`，主题按钮在更左、被当标题栏空白 → 走拖动循环，`WM_LBUTTONUP` 根本到不了 `on_click`。此前 `SendMessageW` 脚本"验证通过"是**假象**：`SendMessage` 跳过 `WM_NCHITTEST`，绕过了这层 bug（真实鼠标走 `SetCursorPos+mouse_event` 在本 VM 又不路由，连最小化都触发不了——故复现只能源自代码推理）。
- **修：按钮移到页签行「磁盘信息」之后**（`theme_btn_rect` 重定位到 tab row，右缘贴 `content_right-MARGIN`、与三页签同一 y 带）。页签行在标题栏下方，`nc_hit_test` 自然返回 `HTCLIENT`，点击送达 `on_click`；另加**防御性** `HTCLIENT` 分支覆盖主题按钮区，杜绝未来布局变动误入拖动/缩放带。
- **误切主题根因**：标题栏版 `hit_test` 把主题按钮命中塞在 `y<TITLE_H` 的窗口按钮分支里，与最小化/关闭同一 y 带易串扰。现主题按钮 y 带 `[50,82]`（页签行）与窗口按钮 `[0,44]`（标题栏）**物理互斥**，`hit_theme_btn`/`hit_test` 都判 y 带，最小化/关闭不可能再触发主题切换。
- **配色收敛（不突兀）**：弃标题栏实色按钮（`C_BTN` 亮块），改**玻璃药丸**——`fill_glass_round(C_WHITE@0.06, hover 0.16)` + `stroke_round(on-grad 暗淡细描边)` + 文字 `C_ON_GRAD_DIM`(hover 白)，与页签同质感、低对比融入渐变。
- `hit_test` 形参语义改**屏幕坐标** `screen_y`（主题/窗口按钮都在固定层；`on_mouse_move`/`on_click` 两处调用点同步），避免滚动把固定层按钮判到负 y；`draw_titlebar` 去 `f_tiny`、`draw_tabbar` 加 `f_tiny`（两处调用点 + smoke_tests 同步）。
- 验证：`SendMessageW` 点新按钮坐标 (505,66) 成功 deep→fluent→deep 切换；`_shot_tmp/f2_cmp_*.png` 双主题三页并排图 + `btn_cmp.png` 按钮区放大确认位置与配色；像素断言 deep bg(139,84,254)/footer(27,54,122)、fluent (48,44,75)/(25,25,40)。
- 门禁：fmt / clippy -D / 全量测试通过；verify_gui_resource PASS（0.4.9.20261022）；dist 已部署。

## UI 打磨三则（ZSUI 开关重写 + 磁盘列距 + 选中行告警色，Z.W. 2026-10-22）

以「颜值/UI 设计师」视角处理 Z.W. 圈的三处：①开关太丑 ②「可用/已用%」挨太紧 ③选第一行红色 93% 显不出来。参考 `qiu7824/zsui` 的令牌化控件方法（4px 网格 / 满药丸 radius / 语义色 / 扁平无脏投影）。

- **开关重写（`draw_switch`）**：丑的根因是旋钮下 `aa_round`「软阴影」在小控件上渲染成一块**脏月牙**（实测截图旋钮旁 (32,30,43) 暗点）。改为 Fluent 药丸——轨道满圆角(radius=h/2)+1px 描边（双层硬边填充，不用 AA stroke 防发虚）、旋钮**纯白无阴影无描边环**。**开=绿 `C_SUCCESS`**（与「已启用」文字、卡片描边 `C_CARD_EDGE_ON` 统一"启用=绿"语义，消除旧版"绿描边/绿字 + 靛轨道"的语义割裂）、**关=叠白玻璃灰紫**，两态异色相一眼分清（旧开靛/关灰紫同为紫调显闷）。
- **磁盘「可用/已用%」列距**：右对齐相邻列的视觉间隙 = 已用%列宽 − 其文本宽，故加宽末列——权重 `[8,5,9,9,8,10,13,14,6]`→`[8,5,9,9,8,10,13,14,9]`（已用% 列宽 38→55px，间隙 13→30px）；总容量/可用 不动保 2048.0 GiB 不被裁。
- **选中行红 93% 显不出来**：根因是选中行把所有文字统一换深字 `C_SEL_ROW_TEXT`，把已用% 的红/橙告警一起吞掉；且亮橙(250,173,20)在浅绿选中底对比仅 ~1.7:1 隐身。改 `used_color(pct, on_light)`——选中浅底走**深红(176,32,38)/深橙(150,84,8)**（对比 5.7:1 / 4.9:1），深底走亮红/亮橙；已用% 列无条件走告警色（含选中）。新增回归 `used_color_selected_row_keeps_alert_visible` 钉死（含"选中行≥90% 不得回落深字"）。
- 验证：VM 重截开关放大（绿药丸/灰紫药丸干净无月牙）+ 磁盘表放大（可用/已用% 拉开）+ `fix_usedcolor_proof.png` 四状态色板含 WCAG 对比度；门禁 fmt/clippy -D/353 测试全绿（含新回归）；verify_gui_resource PASS；dist 部署（0.4.9.20261022）。

## GUI 0.4.7 — 2026-10-19（硬件页内存段「插槽」标签去重）

- 内存段三个字段名都叫「插槽」（汇总行 + 每根明细行），明细行改顺序编号「内存 #N」（与 硬盘#N/网卡#N/显卡#N 一致），汇总行仍叫「插槽」。
- 顺带在真机数据上排除一处磁盘 PD 映射疑点（E:→PD0 映射正确，此前是截图误读，采集层无需改）。

## GUI 0.4.6 — 2026-10-19（磁盘表头挤碰 + 温度 0℃ 假数据）

- **表头挤碰**（用户红框项）：磁盘表列权重要贴合每列真实最小宽，「分区格式」4 字中文表头权重 6→9，`[8,7,10,6,8,12,13,12,6]` → `[8,5,9,9,8,10,13,14,6]`；修后实测间隙 0→11px。教训：`DrawTextW` 硬裁下，权重 < max(表头中文宽, 最长数据宽) 就会零间隙相贴。
- **温度 0℃ 假数据**：WMI `Temperature=0` 是未上报哨兵（0°C 物理不可能），此前被 `calibrate_temp(0)=0` 收成 `Some(0)`。采集层抽 `wmi_temp_c()` 滤 0 + GUI 详情卡 `.filter(|t| *t > 0)` 双保险；新增单测 `wmi_temp_zero_is_not_zero_celsius`。

## GUI 0.4.5 — 2026-10-19（删除调试页 + 玻璃透明度固化 50%）

- 删除「调试」页（玻璃透明度/叠色深浅滑杆 + 输入框 + 重置，约 517 行）：`theme.rs` 的 `GLASS_ALPHA`/`TINT_DEPTH` 原子量与 setter 全删，改 `pub const GLASS_ALPHA: f32 = 0.50`；页签 4→3；`smoke_tests.rs` 的 `theme_constants_regression` 钉死 0.50 与 `tint_color()==C_GLASS_TINT`。

## GUI 0.4.4 — 2026-10-14（磁盘详情页表格加「分区格式」列 GPT/MBR）

- 磁盘表新增「分区格式」列（per-disk `partition_style`：GPT/MBR，经 `v.disk_index` 关联，读不到留 "-"），与 per-volume 文件系统列并列区分。

## GUI 0.4.3 — 2026-10-14（GPT/MBR 分区表类型 + 内存只展示真实条收敛）

- 硬盘行加分区格式（文件系统）显示的上游：分区表类型识别；内存幻影槽（固件虚报）收敛为只统计真实生效条数，与 SPD 直读工具（CPU-Z/HWiNFO）口径一致。

## GUI 0.4.2 — 2026-10-14（硬件页硬盘行加分区格式/文件系统显示）

## GUI 0.4.1 — 2026-10-14（幽灵槽三段提示合并为一条「提示」行）

## GUI 0.4.0 — 2026-10-14（皮肤化：单主题 A 深空玻璃）

- 整窗渐变 `(135,81,238)`→`(38,98,235)`、底栏实色 `(27,54,122)`；frameless 自绘标题栏（仅「最小化 | 关闭」两键，还原走双击标题栏）、圆角 `SetWindowRgn`、窗口描边中深灰。
- **卡片压暗不提亮**：内容卡 `C_GLASS_TINT=(20,24,52)` 深 navy 玻璃，白字对比度 6.7~7.5:1（提亮仅 3.5:1，是「字看不清」的根因）；图标全 GDI 自绘线段、柔和粉彩 `C_ACCENT_SOFT`，禁 emoji。
- 磁盘页 9 列（盘符/磁盘/文件系统/分区格式/簇大小/类型/总容量/可用/已用%），列宽按权重、数值列右对齐；硬件页固定 `WIN_H=948` + 滚动条（窗口尺寸受工作区钳制，小屏自适应）。

## CLI 0.1.0 — 2026-10-19（补全 exe 资源：图标 + 版本 + 版权）

- `winsuitex.exe`（命令行）此前文件属性页全空白（FileVersion/公司名/版权皆无）——CLI crate 的 build.rs 只注入了 `--version` 用的构建时间戳，没链 app.rc，违背「编译产物必须带图标 + 版权信息」。
- 修复：CLI crate 补 `app.rc`（图标 #101 + VERSIONINFO + 应用清单）+ `app.manifest` + `app.ico` + `version.ini`，build.rs 调 MSVC rc.exe 编译链接（沿用 GUI 侧已验证的资源链与五坑规避），并加 version.ini 与 Cargo.toml 语义号 parity 检查（漂移即 warning）。
- 验证：`tools/verify_gui_resource.py` 对 CLI exe PASS（版本 0.1.0.20261019 / 图标 #101 / 清单 #1），`--version` 输出与文件属性版本一致。

## Rust 0.3.x 及更早（2026-09-19 ~ 10-13，摘要）

内存幻影槽显式化（面板/报告只统计真实生效内存、幽灵槽口径小字说明、容量口径告警入 core 单一真源）、芯片组识别增强（AMD 家族 token + 主板型号兜底）、Win11 误报 Win10 修复、显示器品牌回归 core 单一数据源、`center_window` 窄工作区 clamp 越界崩溃修复、厂商中文名收敛单一数据源、SAS/RAID 盘 WMI 通电时间兜底、磁盘 SMART 健康徽标与排查脚本等。GUI 面板自 2026-09-23 首版（egui 即时模式）起，2026-10-14 起为 GDI 自绘版（0.4.0 皮肤化），egui/coreui 废弃版已删。

---

## v1.3.5 — 2026-09-11（内存容量去小数 + 选中分区显示卷标）

两条用户需求：
1. "内存总计还有小数点，感觉怪怪的" → 内存容量改为整数显示。
2. "磁盘信息功能选中分区详情中再加入一个卷标显示，未分区的你自己看如何处理" → 选中详情框新增卷标行。

- **内存容量去小数**：
  - 新增 `FormatBytesInt()`（整数、四舍五入，无小数）：用于内存容量——内存条都是整 GB，".0" 显得累赘。
  - SMBIOS 上报的 `memTotalMB` 通常略小于真实容量（如 16364 MB），直接整除会掉到 "15 GB"；故采用 `(bytes + u/2)/u` 四舍五入，保证显示 "16 GB"。
  - 首页"总计"、每插槽容量（`(capacityMB+512)/1024` 四舍五入）、HTML 报告的"总计"与每插槽三处统一改为整数 GB，全程序内存显示不再有小数点。
  - 磁盘容量仍用 `FormatBytesStr`（保留一位小数，如 223.6 GB 更直观），不受影响。
- **选中分区显示卷标**：
  - `DiskInfo` 新增 `WCHAR volLabel[64]`，分区盘在 `GetVolumeInformationW` 采集时一并取卷标；未分区盘无卷标（已清零）。
  - 选中详情框加第 6 行 `卷标 X`（`DISK_DETAIL_H` 128→148 以容纳），仅可访问分区显示；未分区/无法访问不显示该行。
  - 一键复制文本的"选中详情"可访问分支补 `卷标 = X`。
  - HTML 报告"分区与簇大小"表新增"卷标"列，分区盘带卷标（缺则 `-`）、未分区盘显示 `—`。
  - 仍遵循"四端一致"原则（详情框/复制文本/HTML 报告/采集同步）。
- 版本戳 → `1.3.5.20260911`。

> **验证**：`--selftest-nogui` **161/0** 全绿；编译通过（仅 hardware.cpp 既有 `RSMB` 多字符常量告警，无害）。

## v1.3.4 — 2026-09-11（磁盘详情框显示硬盘序列号）

用户需求"磁盘信息选择框中，显示所选硬盘的序列号"：

- **新增 `serial` 字段**：`DiskInfo` 增加 `WCHAR serial[64]`，四端（表格之外的详情框/复制文本/HTML 报告/采集）统一承载。
- **采集**：
  - 新增 `QueryDiskSerial(pd, out, len)`：以 0 访问权限打开 `\\.\PhysicalDriveN`（仅查询、不锁定，兼容已挂载/在用的盘），查询 `STORAGE_DEVICE_DESCRIPTOR` 的 `SerialNumberOffset` 多字节字段转宽字符并去首尾空格。
  - 未分区盘：在原有描述符读取里顺带取序列号。
  - 分区盘（有 `diskIndex`）：采集末尾统一补查一次（同一物理盘多卷会重复查询，刷新期数量小、可接受）。
- **展示**：
  - 选中详情框加第 5 行 `序列号 X`（`DISK_DETAIL_H` 108→128 以容纳）；无序列号则不显示该行、不占位。
  - 一键复制文本的"选中详情"块三分支（未分区/可访问/不可访问）均补 `序列号 = X`。
  - HTML 报告"分区与簇大小"表新增"序列号"列，分区盘与未分区盘都带序列号（缺则 `-`）。
- 版本戳 → `1.3.4.20260911`。

> **验证**：`--selftest-nogui` **161/0** 全绿；编译通过（仅 hardware.cpp 既有 `RSMB` 多字符常量告警，无害）。

## v1.3.3 — 2026-09-11（清理未分区磁盘型号字符串）

用户实测截图反馈：磁盘页里未分区盘的"文件系统"列显示 `INTEL SSDS -`、`INTEL SSC -`、`INTEL SSDF -` 等带尾部孤立 `-` 的型号，看起来不规整。

- **根因**：`EnumDisks()` 对未分区/未初始化物理盘通过 `IOCTL_STORAGE_QUERY_PROPERTY` 读取 `STORAGE_DEVICE_DESCRIPTOR` 的 `VendorId`/`ProductId` 拼接型号。部分 SSD 固件在 `ProductId` 尾部留空格和 `-`（或字段截断），直接拼接并只去尾空格，会留下 `INTEL SSDS -` 这类尾巴。
- **修复**：新增 `NormalizeDiskModel()` 辅助函数：
  - 去除首尾空格；
  - 合并连续空格为单个；
  - 去除末尾孤立 ` -`（如 `INTEL SSDS -` → `INTEL SSDS`）或单独 `-`。
  - 在未分区盘采集流程中，型号写入 `d->model` 前调用一次，**一处清洗、四处受益**：磁盘表格列、选中详情框、一键复制文本、HTML 报告全部自动生效。
- 版本戳 → `1.3.3.20260911`。

> **验证**：`--selftest-nogui` **161/0** 全绿；编译通过（仅 hardware.cpp 既有 `RSMB` 多字符常量告警，无害）。

## v1.3.2 — 2026-09-11（修复 Win10 下"经典菜单"误显示"未启用"）

实测反馈"win10 下，经典菜单显示未启用"：

- **根因**：`{86ca1aa0-34aa-4e8b-a509-50c905bae2a2}` 注册表键是 **Windows 11 专属 hack**——存在=强制经典（旧版）菜单，不存在=Win11 新菜单。v1.3.1 为修复"勾选后状态不跟实际变化"，把 `IsClassicMenuEnabled()` 改为**只读 HKCU**，但 Win10 及更早版本右键菜单**本就是经典样式**，该键既不写也不生效，于是函数读不到键就返回 FALSE，UI 误显示"未启用"。
- **修复（按操作系统语义区分）**：
  - `IsWindows11()`（复用 common.h `GetOSVersion`，经 RtlGetVersion，不受 manifest 兼容性标记影响，build≥22000 判定为 Win11）。
  - `IsClassicMenuEnabled()`：非 Win11 直接返回 TRUE（经典菜单是系统默认，恒为"已启用"）；Win11 才读 `HKCU` 的 InprocServer32 空值键（与切换写/删的键严格对应，避免 HKLM 残留导致复选框"点完不变"——这正是 v1.3.1 已修过的坑，本版不复用 HKLM 兜底）。
  - **状态文案**：Win10/更早显示"系统默认"而非"已启用"，更准确；Win11 才显示"已启用/未启用"。
  - **点击处理**：新增 `IsWindows11()` 守卫——Win10/更早点击复选框只弹"当前系统右键菜单本就是经典样式，无需切换，此选项仅适用于 Windows 11"提示，**不写不删任何注册表**（避免 v1.3.1 逻辑下在 Win10 误删一个本就不存在的键、还弹出"已切回 Windows 11 新样式菜单"的误导文案）；仅 Win11 才真正写/删键并刷新外壳。
- 版本戳 → `1.3.2.20260911`。

> **验证**：`--selftest-nogui` **161/0** 全绿；编译通过（仅 hardware.cpp 既有 `RSMB` 多字符常量告警，无害）。

## v1.3.1 — 2026-09-11（经典菜单开关位置与状态响应优化）

v1.3.0 提交后实测微调：

- **把"使用经典右键菜单"开关移到标题区右侧空白处**：原放在功能卡片下方，卡片多时挤占列表且接近窗体底部。现改为标题右侧紧凑卡片（200×50 基准），不占卡片区。
- **压缩文案与字号**：标签"使用经典右键菜单（Windows 11 新菜单切换）"缩为"经典菜单"；状态描述"已启用：右键将显示旧版（经典）菜单（重启资源管理器后完全生效）"缩为"已启用/未启用"；字号从 16/13px 降到 13/11px，复选框从 22px 降到 18px，避免超出窗体。
- **统一页脚提示**：把 TAB_MENU 页脚"右键菜单项启用后即时生效，无需重启"改为"右键项即时生效，经典菜单切换需重启资源管理器"，与弹窗/描述一致。
- **修复勾选后状态不跟实际变化**：`IsClassicMenuEnabled()` 原本同时读 HKCU 与 HKCR 作兜底；删除 HKCU 键后若 HKLM 仍有残留键，HKCR 合并视图会让函数继续返回 TRUE，导致复选框点完仍显示勾选。改为**只读 HKCU**（切换操作写/删的也是 HKCU），勾选状态即时准确反映用户操作。
- 版本戳 → `1.3.1.20260911`。

> **验证**：`--selftest-nogui` **161/0** 全绿；编译通过（仅 hardware.cpp 既有 `RSMB` 多字符常量告警，无害）。

## v1.3.0 — 2026-09-11（右键经典菜单开关 + PowerShell 右键项 + 磁盘页补全未分区盘）

三处增强，全部数据驱动、与现有右键功能/磁盘逻辑同构：

- **右键增强页新增"使用经典右键菜单"开关（Windows 11 新菜单一键切换）**：在功能卡片下方加一行自绘复选框，实时读取 `HKCU\Software\Classes\CLSID\{86ca1aa0-34aa-4e8b-a509-50c905bae2a2}\InprocServer32`（HKLM 同键作兜底）。勾选 = 写入该键空值（强制经典菜单）、取消 = 删除该键（恢复 Win11 新菜单），操作后 `SHChangeNotify` 刷新资源管理器（部分系统即时刷新，普遍需重启资源管理器后完全生效）。复选框为纯 GDI 自绘（无系统控件），hover/选中态与现有卡片一致。
- **右键功能新增 PowerShell（"在此处打开 PowerShell"）**：沿用 `g_features[]` 数据驱动框架新增一项，菜单文字"PowerShell"，命令 `powershell.exe -NoExit -Command "Set-Location -LiteralPath '%V'"`（`%V` 由 Explorer 替换为当前文件夹，不依赖工作目录）。为兼顾文件夹内、文件夹空白处、桌面空白处三场景，挂到 `Directory\shell` + `Directory\Background\shell` + `DesktopBackground\shell` 三处父路径；并配 Win11 新菜单直显 CLSID `{0F486944-1B88-4059-ADFD-2ACF2469D7F2}`，注册/注销/图标/HTML 报告全自动适配，无需改其它代码。
- **磁盘页补全"未分区/未初始化"磁盘**：原 `EnumDisks()` 只枚举逻辑盘符（`GetLogicalDriveStringsW`），无盘符的物理盘（空盘/未初始化/整盘未分区）完全不可见。本次在枚举完逻辑分区后，探测 `PhysicalDrive0..31`，仅把"没有任何已挂载卷"的物理盘追加为独立行（沿用 `CollectDisk()` 的 IOCTL 模式读型号 + 总线 + 物理总容量），避免与已分区盘重复。表格里该行盘符显示"—"、类型"未分区"、文件系统列改显型号、簇大小/可用显"—"；选中详情框与一键复制文本、HTML 报告同步补型号/总线/物理容量说明。
- 版本戳 → `1.3.0.20260911`；exe 体积随新 CLSID/复选框资源小幅上浮。

> **验证**：（构建/自检/双 agent 对抗审查结果待补）

按需求"内存容量总计单位需要处理"，把原本裸写 "X MB" 的内存容量改为自动换算到合适单位（GB/TB），并顺手把整段内存的单位都统一了：

- **主页"总计"行**：原 `memTotalMB × 1024 × 1024` 直接拼成 "16384 MB"，现过 `FormatBytesStr` 自动换算，真实 16GB 机器显示 "16.0 GB"，不再出现一长串 MB。
- **HTML 报告"总计"行**：同步改为 `FormatBytesStr` 换算（与主页一致）。
- **一键复制文本"总计"行**：改用 `FmtSizeW`（整数 GB/MB，如 "16 GB"）换算，与主页/HTML 同源但整数风格。
- **每条内存明细统一**：发现 HTML 报告与一键文本里每条内存仍裸写 "%u MB"，而主页每条明细早已是 "%.1f GB"，三处单位不齐。本次把 HTML 报告（`FormatBytesStr`）与一键文本（`FmtSizeW`）的每条明细也换算成 GB，整段内存单位彻底统一。
- 容量字段 `capacityMB` 经 `(ULONGLONG)×1024×1024` 或 `<<20` 提升为字节再换算，无溢出；`memTotal`/`mt`/`cap` 缓冲 32 字节足够（最坏 "16.0 GB" 级别）。
- 版本戳 → `1.2.9.20260911`；exe 690KB（与 v1.2.8 持平，无新增资源）。
- **验证**：`--selftest-nogui` 142/0 全绿；双 agent 对抗审查（内存安全 + 功能一致性）**0 Critical 0 Major**——三处总计与每条明细数值一致、单位统一，缓冲边界安全，无布局回归。

## v1.2.8 — 2026-09-11（补 DISP/DISK 循环 HW_MAX_* 封顶，与 CPU/GPU 一致）

v1.2.7 对抗审查记下的既有 Minor：HWSEC_DISP / HWSEC_DISK 的循环此前未加 `HW_MAX_*` 截断（CPU/GPU 段已封顶）。采集端 `HW_Collect` 本就封顶、且两端循环一致故不引发布局错位，但属潜在越界隐患（采集端若异常给出大于上限的 count 即 OOB 读）。本次补齐：

- `HwSecRows` HWSEC_DISK 改为 `if (diskCount==0) return 1; { int n=min(diskCount,HW_MAX_DISK); return n; }`；HWSEC_DISP 循环加 `&& i < HW_MAX_DISPLAY`。
- `DrawHardwarePage` HWSEC_DISP / HWSEC_DISK 循环同步加 `&& i < HW_MAX_DISPLAY` / `&& i < HW_MAX_DISK`。
- 计划行数与绘制行数仍严格相等（两端同用 `min(count, HW_MAX_*)` 上限、同 name/vendor 行数判定）；0 时两端均 1 行。
- 纯防御性重构，不改变任何实际展示（正常采集 count ≤ HW_MAX_* 时行为完全不变）；exe 体积持平 689KB。
- **验证**：`--selftest-nogui` 142/0 全绿；双 agent 对抗审查（内存安全 + 功能一致性）**0 Critical 0 Major**——DISK/DISP 计划行数与绘制行数完全一致，新增 `&& i < HW_MAX_*` 封顶两端对称、且数组上限同值，不存在一端封顶另一端漏封的脱节，亦无越界。

## v1.2.7 — 2026-09-11（主页多 CPU/显卡全列 + 移除 CPU 代号行）

按需求两点调整硬件首页展示：

- **主页不再显示 CPU 微架构代号**：v1.2.5 加的"代号"行从硬件首页（DrawHardwarePage HWSEC_CPU）移除，避免和普通用户无关的微架构术语挤占首页。代号仍**保留在 HTML 报告（每颗 CPU 行）与一键复制文本**里——面向维护工程师的深度信息不丢，只是不上首页。
- **主页支持多 CPU 全列**：HWSEC_CPU 由"只画 cpus[0]"改为遍历 `g_hw.cpus[0..cpuCount-1]`；单颗时标签"型号"，多颗时每颗独立成块、标签 `CPU#1 / CPU#2 …`。主页"处理器"段行数随之变为 `cpuCount × 2`。
- **主页支持多显卡全列**：HWSEC_GPU 同样由"只画 gpus[0]"改为遍历 `g_hw.gpus[0..gpuCount-1]`；多卡时每颗独立成块、标签 `显卡#1 / 显卡#2 …`，并**逐卡**判断 `GpuHasTelemetry` 决定是否画状态行（之前只判断主卡）。HwSecRows 同步改为逐卡计行。
- 显示器段（HWSEC_DISP）此前已遍历所有显示器，本次未改，保持多显示器全列。
- **核心不变量守住**：HwSecRows 计划行数 == DrawHardwarePage 绘制行数，对 cpuCount/gpuCount == 0 / 1 / >1 各分支逐条推演一致；循环上限统一用 `i < count && i < HW_MAX_*`，与 HwSecRows 的 `n = min(count, HW_MAX_*)` 对齐，杜绝计划/绘制脱节（该项目 v1.1.10、v1.2.5 都修过此类 bug）。
- 版本戳 → `1.2.7.20260911`；exe 689KB（与 v1.2.6 持平，本次无新增资源）。
- **验证**：`--selftest-nogui` 142/0 全绿；双 agent 对抗审查（内存安全 + 功能一致性）**0 Critical 0 Major**——三段落计划/绘制行数完全一致，多设备循环边界与 HW_MAX_* 封顶对齐。审查另记一处**既有** Minor：HWSEC_DISP/HWSEC_DISK 循环未加 HW_MAX_* 截断（CPU/GPU 有），因两端一致且 HW_Collect 已封顶，不引发布局错位，本次不改。

> **版本号修正说明（2026-09-10）**：v1.0.10 ~ v1.0.13 这段误用了 `--set` 手动指定版本号，破坏了全局「每次编译 修订+1、满10进1」的规则（修订段不应超过 9，满 10 应进 1 到次段）。已按规则重算并修正为 **v1.1.4.20260910**。正确序列：`1.0.8 → 1.0.9 → 1.1.0 → 1.1.1 → 1.1.2 → 1.1.3 → 1.1.4`。下文中 v1.0.10~v1.0.13 对应修正后的 v1.1.0~v1.1.4，仅为保留原始提交命名。

## v1.2.6 — 2026-09-11（未装显卡驱动也能识别真实型号：PCI 硬件 ID 查表）

补上"刚装完系统还没打驱动"场景的最后一块拼图——无驱动时显卡不再显示微软占位名：

- **根因**：无驱动时 Windows 加载微软基本显示驱动，GDI/DXGI 只报 "Microsoft Basic Display Adapter"（Win10）/"Standard VGA Graphics Adapter"（Win7），真实型号在 GDI 层拿不到。但 **PCI 设备节点与驱动无关**，`SPDRP_HARDWAREID` 恒有 `PCI\VEN_XXXX&DEV_XXXX`。
- **内置 PCI 映射表**（`thirdparty/pci_ids/gpu_pci_table.h`，自动生成自 pciutils pci.ids，勿手改）：NVIDIA/AMD/Intel 三家 GPU 关键词过滤 + 剔除 HDMI Audio 误命中，**2223 条**精确 device 名（如 GTX 1060 6GB / RX 470-590 / UHD 630），二分查找。
- **两层还原**（`CollectGpuRealNames`）：① GDI 适配器 `DeviceID` 直接解析 VEN/DEV 修正占位名；② GDI 拿不到时 SetupAPI Display 类硬件 ID 兜底，多出的设备（无驱动双卡同名被去重丢失）自动追加补齐；未命中表退到 vendor 级（含 Matrox/S3/SiS/VIA/3dfx/ASPEED 等 16 家，"NVIDIA 显卡 (PCI 10DE:1C82)"）。已装驱动零开销跳过。
- **修 DXGI 占位卡重复追加**：无驱动时 GDI 报 Basic Display Adapter、DXGI 报 Basic Render Driver，互不匹配会多出一条假显卡 → 追加分支加占位名过滤。
- 显存/遥测行为：无驱动时显存保持 0（DXGI 基本显示驱动的"显存"是共享内存，不写不准数）；NVML/ADL 本就随驱动分发，无驱动自动降级。
- selftest 新增 6 项：占位名残留扫描 + PCI 查表单测（GTX 1060 6GB / RX 470 / UHD 630 精确命中、未知名 NULL、占位名检测边界）。
- **验证**：双 agent 对抗审查 0 Critical；Major（老 vendor 覆盖 + 双卡同名丢失）与 consumed 死代码均已修复重写；142/0 自检全绿。真机（尤其无盘镜像未装驱动的机器）待实测。
- 版本戳 → `1.2.6.20260911`；exe 609→689KB（+80KB，主要是 PCI 表）。

## v1.2.5 — 2026-09-11（CPU 微架构代号：libcpuid 静态编译，零运行时依赖）

给 CPU 补上 SMBIOS/CPUID 品牌串拿不到的**微架构代号**（如 "Ivy Bridge" / "Zen 3"），维护时一眼看懂平台代际：

- **libcpuid 0.4.1 静态编译**（源码取自 gitee 镜像 xealous/libcpuid，快照在 `thirdparty/libcpuid/`）：仅取 `cpu_identify()` → `cpu_codename` 最小文件集（5 个 .c），纯用户态 C 代码随项目编译，**零驱动、零 DLL、零运行时依赖**；识别失败安静降级为不显示。
- `HwCpu` 新增 `codename[48]`；`HW_Collect` 在 SMBIOS/CPUID 填完 name/vendor 后调用 `CollectCpuCodename()`（型号级属性，仅填第一颗 CPU，不覆盖已有字段）。
- **三处展示**（仅识别到代号才显示）：主页面规格行后加"代号"行；HTML 报告每颗 CPU 追加"代号"行；一键复制文本 socket 行后追加"代号: ..."。
- 构建：libcpuid 5 个 .c 用 **gcc**（`-std=gnu11 -DHAVE_CONFIG_H`，enum→int 隐式转换是 C 宽松语义，g++ 编不过）编译成 .o 再与主程序 g++ 链接（`libcpuid.h` 自带 extern "C"）；手写 `config.h` 仅定义 `VERSION`；build.bat 新增 [3.5/5] 步骤。
- 顺带修复（对抗审查发现）：`HwSecRows(HWSEC_CPU)` 在 cpuCount==0 时计划 2 行 vs 绘制 1 行的 v1.1.10 遗留脱节 → 对齐为 1 行。
- 版本戳 → `1.2.5.20260911`；exe 461→609KB（+148KB，主要是代号识别表）。
- **验证**：双 agent 对抗审查（内存安全 + 功能一致性）0 Critical 0 Major；`--selftest-nogui` 全绿，沙箱 Xeon 实测识别出 "Broadwell-E (Xeon)"。

## v1.2.4 — 2026-09-11（AMD GPU 遥测：ADL 动态加载，补齐 A 卡）

延续 v1.2.3 的 NVIDIA NVML 遥测，给 A 卡补上同等能力：

- **动态加载 `atiadlxx.dll`（失败回退 `atiadlxy.dll`）**，零额外驱动；拿到即走 ADL，拿不到（非 A 卡 / 未装 AMD 驱动）安静退出、字段全 0 降级。
- 优先 **ADL2_* 系列**（带 context handle 的新 API），复用 `HwGpu` 已有的 7 个遥测字段，语义与 NVML 一致，三处展示（主页面状态行 / HTML / 一键复制）自动生效。
- 采集项：**温度**（OD6 `ADL2_Overdrive6_Temperature_Get` 优先，回退 ODN `ADL2_OverdriveN_Temperature_Get`，温度以 m°C 返回需 ÷1000）、**显存已用**（`ADL2_Adapter_VRAMUsage_Get`）；跳过非活跃适配器（`ADL2_Adapter_Active_Get`）。风扇/功耗/时钟 ADL 不通过公共 OD 接口稳定暴露，保持 0，不做伪报。
- 所有 ADL 函数指针独立判空，缺失的 API 那项保持 0，绝不野指针。
- 版本戳 → `1.2.4.20260911`。
- **验证**：`--selftest-nogui` 全绿（沙箱无 A 卡走降级分支）；A 卡真机需实测温度/显存数值。

## v1.2.3 — 2026-09-11（NVIDIA GPU 遥测：NVML 动态加载，零额外驱动）

按"可引入更优原生 DLL"的新架构方向，给 N 卡补上温度/风扇/功耗/利用率/显存用/核心时钟（之前只有型号+显存，无遥测）：

- **动态加载 `nvml.dll`，零额外驱动**：`nvml.dll` 随 NVIDIA 显卡驱动自带，不引入任何 SYS；`LoadLibraryW` 拿不到（非 N 卡 / 没装 N 驱动）即安静退出，所有遥测字段保持 0，界面走"未识别"降级，**核心功能完全不受影响**（符合可选增强、缺失即降级的设计原则）。
- **各遥测函数独立判空**：`nvmlDeviceGetClock` 等旧符号在部分新版驱动里可能缺失，逐项 `GetProcAddress` 守卫，缺哪项那项就保持 0，绝不野指针崩溃。
- **按名字匹配落到已有 `gpus[]` 条目**（大小写无关子串），匹配不到就跳过，不硬塞；`gpuCount` 不变。NVML 的 `total` 显存还能回填补 DXGI 没拿到的 `vramMB`。
- **三处口径一致**：主页面显卡段（新增"状态"行，仅有遥测时画）/ HTML 报告 / 一键复制文本，全走同一 `GpuHasTelemetry()` 判断，无遥测时不产生空行、不追加状态段。
- `HwGpu` 新增 7 个遥测字段（`tempC/fanPct/powerW/utilPct/memUsedMB/memFreeMB/clockMHz`），`ZeroMemory` 保证缺省为 0。
- 自检新增 NVML 渲染两分支断言（无遥测不追加 / 有遥测含各字段）。
- **验证**：`--selftest-nogui` 全绿（无 N 卡的沙箱上自动走"无遥测"降级分支，核心仍通过）；N 卡真机需实测遥测数值。

## v1.2.2 — 2026-09-10（WIM 真机：显示器厂商行去冗余）

WIM 真机（AOC 显示器）反馈：v1.2.1 把厂商并入型号行（如 `AOC 2769`）后，下方仍单独列一行 `厂商：AOC`，造成重复。本版去掉冗余：

- **显示器厂商行仅在型号行未含厂商时显示**：新增 `DispVendorRedundant()` 辅助判断——当 `name` 以 `vendor` 开头（大小写无关，如 `AOC 2769` 以 `AOC` 开头）时，视为型号行已含厂商，隐藏单独的厂商行；否则（如通用名 `Generic PnP Monitor` 未替换、或 name 为空仅识别到厂商）照常显示厂商行。
- 三处口径一致：主页面 `HwSecRows`（行数计算）/ `DrawHardwarePage`（绘制）/ HTML 报告（`厂商：%ls`）均走同一判断，布局不会因隐藏而空行。
- **验证**：主工程自检 130/0（exit 0）。

## v1.2.0 — 2026-09-10（处理器显示改版：去混淆 + 修超边界）

WIM 真机目测 v1.1.9 后用户反馈：处理器行既超边界又看不懂"硬件 X"是什么。本版重做处理器显示：

- **处理器改为 2 行布局**：`型号` 行（完整型号名）+ `规格` 行（`%u 核 / %u 线程 · %u MHz`）。多颗 CPU 再加 `总计` 行。
- **只显示 CPUID 真实硬件能力，去掉"BIOS 启用的当前核数"和 `(硬件 X)` 标注**：维护盘点要的是真实规格（如 BIOS 单核节能时启用 1 核却真实是 4 核），并排显示既冗长又让人误以为"8 核"。真机 E3-1230 V2 现在显示 `4 核 / 8 线程`，与竞品一致。
- **修超边界**：原行长字符串（型号+启用值+硬件值+频率）被 `DT_END_ELLIPSIS` 裁得难看；改短后规格行稳定可见，不再溢出。
- 同步改 HTML 报告处理器段（规格行直接显示真实能力，不再并排"启用值"），三处口径一致。
- **验证**：主工程自检 129/0（exit 0）。

## v1.2.1 — 2026-09-10（WIM 真机：CPU/网卡/显示器三项识别修复）

按 WIM 真机（i5-3570 / MSI B75MA-P45 / Realtek PCIe GBE + Realtek 8188GU / AOC 显示器）新截图继续修 3 处：

- **修：CPU 规格虚报翻倍（i5-3570 显示 8 核 / 16 线程）**
  - 根因：原实现仅依赖 `CPUID.0xB` 推导物理核/线程。在 VM 或部分旧平台 BIOS 中，CPUID 会暴露宿主拓扑而非 Guest 实际可用核数，导致 i5-3570（真实 4 核 / 4 线程）被错报为 8 核 / 16 线程。
  - 修复：新增 `GetProcessorTopologyFromOS()`，优先使用 `GetLogicalProcessorInformationEx(RelationProcessorCore)` 获取操作系统当前真实激活的处理器拓扑（与 Windows 任务管理器一致），再回退到 CPUID。OS 不可用时保留原 CPUID 兜底链。
- **修：网卡"总计"把虚拟适配器也算进去**
  - 现象：设备管理器只有 2 块真实网卡（Realtek 8188GU Wireless、Realtek PCIe GBE），但 WinSuiteX 显示"共 4 块网卡"——把 WAN Miniport 等虚拟适配器计入。
  - 修复：新增 `IsVirtualNic()`，在 `CollectNic()` 中过滤描述含 `WAN Miniport`、`Microsoft 6to4/ISATAP/Kernel Debug/Teredo`、`RAS Async`、Npcap/WinPcap/TAP-Windows 等虚拟网卡；UI 主页面扩展为展示前 2 块真实网卡，超过 2 块再显示"总计 N 块网卡"。
- **修：显示器型号仍显示 "Generic PnP Monitor (2769)"**
  - 现象：厂商已正确识别为 AOC，型号后缀 (2769) 也已提取，但型号行仍显示 `Generic PnP Monitor (2769)`，不够直观。
  - 修复：在 `AppendMonitorModelFromDeviceId()` 中，当原 EDID 名为 `Generic PnP Monitor`、`Generic Non-PnP Monitor` 或中文"通用即插即用监视器"，且成功解析出厂商/型号时，直接用 `厂商 型号`（如 `AOC 2769`）替换原名字。
- **验证**：主工程自检 130/0（exit 0）；HTML 报告无头导出验证通过。

## v1.1.9 — 2026-09-10（WIM 真机目测 bug 修复）

v1.1.8 在 WIM 真机（Xeon E3-1230 V2 / 华硕 P8H61-M / GTX 650 / AOC 32G2WG3）目测发现 2 个真 bug，本版修复：

- **处理器硬件最大核数显示错误（8 而非 4）**：原 `CPUID.0xB` 解析误用 `EAX[4:0]`（x2APIC 移位位数）当计数，且把 levelType==2 的"整颗 CPU 逻辑处理器总数"直接当物理核。修正为用 `EBX[15:0]`（Intel SDM 规范计数），并 `物理核 = 总逻辑 / 每核线程数`（E3-1230 V2：8÷2=4 核）。
- **内存类型显示"LPDDR"而非"DDR3"**：根因不是偏移（Type 17 的 Memory Type 字段确实在 0x12，偏移正确），而是字节→字符串枚举值整体错移了 3（把 DDR3 写成 0x15、LPDDR 写成 0x18…）。机器真实报告 `0x18`，被误映射成 LPDDR。已按 DMTF SMBIOS DSP0134 修正枚举（0x18=DDR3 / 0x1A=DDR4 / 0x1B=LPDDR …）。
- 为实现真回归测试，把内存类型枚举抽成 `HwMemTypeToStr()` 独立函数，selftest 新增 9 条字节→字符串映射断言（含 0x18→DDR3 等关键用例），杜绝"注入式渲染测试漏测枚举"的假成功。

## v1.1.8 — 2026-09-10（竞品对比增强：硬件信息更详细）
对标竞品软件截图，补齐 5 项硬件细节（用户决策：5 项全做；尺寸按"能同行就同行"追加）：

- **处理器**：新增"硬件最大核/线程"（CPUID 真实能力，不受 BIOS 单核节能限制），与 SMBIOS 报告的"启用核/线程"并列；仅当两者不同才追加 `(硬件 X)` 标注。
- **主板**：新增"芯片组"行（SetupAPI 枚举主机桥，精确匹配"主机桥/Host Bridge/芯片组/Chipset"，避免误命中 PCIe 下游桥）。
- **内存**：每条内存追加"类型"（DDR3/DDR4/DDR5…，SMBIOS Type 17 Memory Type）；识别不到留空，不写"未知"（与 HTML/文本口径一致）。
- **显卡**：每条 GPU 追加"显存 (X GB)"（DXGI `IDXGIAdapter1::GetDesc1` DedicatedVideoMemory，跳过软件适配器；按名称匹配写入，避免多 GPU 顺序错位）。
- **显示器**：分辨率同行追加"尺寸 (X 英寸)"（SetupAPI `DEVPKEY_Monitor_PhysicalSize`，勾股换算；注意 pid=3）。
- **对抗审查（2 轮只读）修 5 处**：① 显示器尺寸 DEVPROPKEY 漏写 `pid=3` → 尺寸功能静默失效（被注入测试掩盖，审查才抓出）；② HTML 主板段 BIOS `<td>` 未闭合就把芯片组 `<tr>` 嵌进去 → 嵌套非法；③ 显示器厂商名用 `%s` 接 `WCHAR*`（应为 `%ls`）→ 乱码；④ 芯片组关键字裸 `"Bridge"` 误命中 PCIe 桥；⑤ CPU 最大核/线程回退守卫被 0x0B 循环污染 `regs` → 缓存 `maxLeaf` 修复。
- **验证**：主工程自检 119/0（新增 13 项 v1.1.8 断言：CPUID 硬断言 + 纯文本/HTML 注入式渲染分支断言）。

## v1.1.7 — 2026-09-10（显示器厂商识别）
- **显示器型号补全厂商型号**：在 v1.1.5 已取 EDID 真实型号基础上，按厂商代码映射表（AOC/SAM/DEL/ACR/GSM/HWP 等）补全前缀，使型号更易读（如 AOC 32G2WG3）。

## v1.1.6 — 2026-09-10（WIM-20250421TVP 真机目测 hotfix）
按 WIM 机器（Win10 Pro for Workstations 22H2）真机截图 + 导出 HTML 报告反馈修 2 处：

- **修：默认音频设备识别失败（跨本地化匹配）**
  - 现象：系统声音设置明确显示"扬声器 (High Definition Audio Device)"是默认输出，但工具显示"未识别默认设备"。
  - 根因：`CollectDefaultAudioByRole` 用 COM `PKEY_Device_FriendlyName`（英文 "Device"）拿默认端点名，与 SetupAPI 采集到的同名设备（中文 "设备"）做 `wcsstr` 双向匹配，因**中英文 "Device/设备" 差异**整段失配。
  - 修复：新增 `NormalizeAudioKey`——把两端名字各自**转小写、去空格与括号、剥掉 `device`/`设备`** 后再做大小写无关子串匹配；跨中英文本地化也能命中（扬声器↔High Definition Audio 设备、NVIDIA/Realtek/USB 等场景均验证通过）。
- **修：显示器型号不精确（缺真实型号）**
  - 现象：设备管理器显示 "Generic Monitor (32G2WG3)"，32G2WG3 是 AOC 真实型号，但工具只显示 "Generic PnP Monitor"。
  - 根因：EDID 没给具体型号时 Windows 把名字 fallback 成 "Generic PnP Monitor"，**真实型号藏在 `mdd.DeviceID` 硬件 ID 里**（形如 `MONITOR\GSM32G2WG3\...\0000`）。
  - 修复：新增 `AppendMonitorModelFromDeviceId`——解析 DeviceID 第二段硬件 ID，剥掉 3 字母厂商前缀（GSM/ACR/SAM/DEL/HWP…），把型号拼到名字后 → `Generic PnP Monitor (32G2WG3)`；同步进主页面与 HTML 报告（均读 `d->name`）。占位 ID（`Default_Monitor`/含下划线）与名字已含型号的情况自动跳过，不重复不误报。
- **验证**：算法级独立单测 10 场景全通过（音频 5 + 显示器 5）；主工程自检 106/0。

## v1.1.5 — 2026-09-10（v1.1.4 真机目测 hotfix）
按 v1.1.4 真机目测截图继续修 4 处（横向 Tooltip 仍按计划留后续独立版本做）：

- **修：主板段缺 BIOS 信息**
  - 之前"主板"段只显示型号（厂商+产品），BIOS 厂商/版本/日期被丢掉了（原 v1.0.8.1 的合并段在后续改动中丢失）。
  - 修复：段标题改为"主板 / BIOS"，在型号行下补一行 `BIOS / 厂商 版本 日期`。
  - `HwSecRows HWSEC_BOARD` 从 `1` 改为 `2`。
- **修：内存行"插槽名"与"容量"黏在一起**
  - 用户纠正：不是两条内存之间缺分隔，而是单条内"插槽名"溢出 96px 标签列、顶到右边的"容量"字段。
  - 修复：内存行 label 固定为"插槽"二字（永不溢出），value 改为 `插槽名 · 容量 · 频率 · 厂商`，插槽与大小天然被列间距 + `·` 分隔开。
- **修：显示器型号显示成显卡名（数据源错误）**
  - 根因：`CollectGpu` 把 `EnumDisplayDevicesW(NULL, i, &dd)` 第一层枚举出的**显卡适配器名**（如 "NVIDIA GeForce GTX 650"）直接当成了显示器型号。
  - 修复：对挂桌面的适配器（`DISPLAY_DEVICE_ACTIVE`）再枚举一层 monitor，`mdd.DeviceString` 才是 EDID 里的**显示器真实型号**；非活跃适配器（RDP/镜像重复条目）跳过，避免产生十几条同名重复显示器。
  - 本机（远程桌面环境）实测：修复前显示 "Microsoft Remote Display Adapter"，修复后显示 "Generic Non-PnP Monitor 1920×1080@32Hz"。
- **修：音频段只显示设备名，缺"总计"**
  - 用户要求："音频设备显示总计+目前使用中的设备名"。
  - 修复：段内改为 3 行——`总计 / N 设备` + `播放 / 默认播放 · 设备名` + `录音 / 默认录音 · 设备名`；默认设备识别不到时显示"未识别默认设备"。
  - `HwSecRows HWSEC_AUDIO` 从 `2` 改为 `3`。

## v1.1.4 — 2026-09-10（原 v1.0.13，v1.0.12 真机目测 hotfix）
按真机目测反馈修 3 个展示问题（问题 3 横向 Tooltip 留 v1.1.5 单独做）：

- **修：内存明细行间距过紧**
  - `ChannelA-DIMM/8.0 GB · 1600 MHz · Kingston` 紧贴上一行 "总计"——locator 字段用满 96px 标签区，与 value 字段无视觉分隔。
  - 修复：locator 字段后加 ` · ` 视觉分隔符；空槽 fallback "插槽" 也加。
- **修：显示器段缺型号**
  - 之前只显示"分辨率 @ 刷新率 · 显卡名"，型号信息被吞在 value 字段里不易识别。
  - 修复：每显示器拆为 2 行——`显示器#1 / 1920 × 1080 @ 144 Hz` + `型号 / Dell U2719D`；多显示器全列。
  - `HwSecRows HWSEC_DISP` 从 `1` 改为 `1 + (displayCount - 缺型号数)`。
- **修：默认音频只显示播放，没显示录音**
  - 用户反馈"不是只显示默认设备，主要默认使用的设备你没识别对"——指默认播放之外的默认录音也应展示。
  - 修复：`HwAudio.role` 字段（bit 0=默认播放 / bit 1=默认录音 / 0=未识别为默认）。
  - `CollectDefaultAudio` 拆为 `CollectDefaultAudioByRole(eRender, 1)` + `CollectDefaultAudioByRole(eCapture, 2)`；同一条音频可同时被标为播放+录音（如 USB 耳麦）。
  - 主页音频段改为 2 行：默认播放 / 默认录音；某 role 识别不到则少一行；都识别不到时 fallback 显示第一个真实音频设备。
  - `HwSecRows HWSEC_AUDIO` 从 `1` 改为 `2`（最坏情况）。
- **HTML 报告不变**（显示器和音频仍全列全部；BIOS/内存等明细也全列）
- **问题 3 横向 Tooltip 暂未做**：按 v1.0.12 商量的"分两阶段"计划，Tooltip 留到 v1.0.14 单独 commit + 多轮对抗式 review

## v1.0.12 — 2026-09-10
按真机目测反馈做的"信息深化 + 默认化"三项增强：

- **磁盘选中详情框加 NT 设备名**（与 HTML 报告分区表段保持一致）：
  - 选中分区时在"总容量 / 可用 / 已用 %"下方多一行 `NT 设备名  \\.\PhysicalDriveN`（如 `\\.\PhysicalDrive0`、`\\.\PhysicalDrive1+2`）。
  - 失败 / RAM / 网络盘显示 `-`。
  - 详情区高度 88 → 108px（多 1 行）。
- **主页内存段展开已插内存条**：
  - 之前只有 1 行汇总（"X MB · 已插 N / M 插槽"），现在叠加每条已插的明细行：
    - 槽位 | 容量（GB / MB）· 频率（MHz）· 厂商
    - 例：`DIMM_A1 · 16.0 GB · 3200 MHz · Kingston`
  - 空槽折叠为 1 行 `空槽 N 条`。
  - 行数从 1 → 1 + 已插数（最多 HW_MAX_MEM=16）+ 1（空槽行）。
  - 料号 / Bank 等进 HTML 报告（主页宽度有限，料号太挤）。
  - HiDPI 自适应：行数过多时仍按 USB < 音频 < 显示器 < 网卡 < **内存** 优先级裁次要段。
- **主页音频只显示当前默认播放设备**：
  - 之前显示"全部真实识别到的音频设备"或"共 N 个 · XXX"，现在改为"默认播放设备：XXX"。
  - 数据来源：`IMMDeviceEnumerator::GetDefaultAudioEndpoint(eRender, eConsole)` → `IPropertyStore::GetValue(PKEY_Device_FriendlyName)` → 模糊匹配到 SetupAPI 采集的某条 audio。
  - COM 用 STA + `COINIT_DISABLE_OLE1DDE`，每步失败都 fallback 不抛异常。
  - 识别不到默认设备时 fallback 显示第一个真实音频设备（避免整段空白）。
  - HTML 报告不变（仍列全部音频，便于多声卡环境排查）。
- **自检 105 → 106**：新增 `memPopulated` 统计正确 + 默认音频标记断言（在沙箱有音频时生效）。
- **代码增量**：+~120 行（main.cpp 三处 / hardware.cpp +CollectDefaultAudio / hardware.h +isDefault 字段 / selftest.cpp +断言）。

## v1.0.11 — 2026-09-10
按真机目测反馈做的"磁盘号可追溯"增强：

- **磁盘信息页加「磁盘」列**：
  - 主页面表格从 6 列升级为 7 列，在"盘符"之后插入"磁盘"列，显示各分区对应的 PhysicalDrive 编号（如 `PD0`、`PD1`）。
  - 跨盘卷（条带 / 镜像卷会跨多个物理盘）显示 `PD0+1`、`PD1+2` 等，余下 extents 用 `+N` 拼接。
  - 失败 / RAM 盘 / 网络盘无法取到 extents 时显示 `-`。
- **HTML 报告分区表段加「NT 设备名 / 磁盘」列**：
  - 导出报告的分区表新增一列 `NT 设备名 / 磁盘`，输出标准 NT 设备路径 `\\.\PhysicalDriveN`（如 `\\.\PhysicalDrive0`、`\\.\PhysicalDrive1+2`），可直接贴出来定位或用 diskpart / 磁盘管理跳转。
- **实现要点**：
  - 新增 `GetVolumeDiskIndex()`：构造 `\\.\X:` 卷路径 → `CreateFileW`（0 权限 / 共享读写）→ `IOCTL_VOLUME_GET_VOLUME_DISK_EXTENTS` → `DISK_EXTENT` 数组首个 `DiskNumber` 作为主号，余下拼接 `+N`。
  - `DiskInfo` 结构加 `diskIndex`（-1=无）/`diskExtents[16]`（余下 extents 串）。
  - `EnumDisks()` 改为非 static（selftest 也需调用填充），并在采集每个卷时填充上述两字段。
- **版本策略注**：本应 bump 到 `1.0.11`，但 `version_stamp.py` 默认 `--bump` 把 1.0.10 进位到 `1.1.0`，故产物版本字符串显示为 `1.1.0.20260910`（前三段语义版本）。下次手动用 `--set 1.1.1` 即可。

### v1.0.11.1 — 2026-09-10 hotfix（磁盘表头截断）
- **现象**：真机目测发现磁盘信息页表头"文件系统"被截成"文件系"，"簇大小"列也偏挤。
- **根因**：`colW[7] = {50, 96, 54, 56, 80, 100, 100}` —— "磁盘"列 96px 远超"PD0+1"6 字符所需（~40px），"文件系统"列 54px 放不下 4 个汉字（4×14px ≈ 56-60px + padding 超列宽）。
- **修复**：压缩"磁盘"列 96→64，省下的 32px 分配给"文件系统"54→80 和"簇大小"56→62；总宽 536 保持不变，无连锁回归。
- **验证**：静态计算"文件系统"列写字区 = 80 − 8 padding = 72px，4 字 56-60px 完全放下；`--selftest-nogui` 105/0。沙箱无法渲染窗口，**仍需真机目测**。

## v1.0.10 — 2026-09-10（hotfix）
- **修：v1.0.9 双缓冲顺序错导致界面全空**
  - 现象：真机启动 v1.0.9 后"只有一个透明窗口，没有其它显示"——看不到按钮、卡片、页脚、标题栏文字。
  - 根因：WM_PAINT 里 `EndPaint(hWnd, &ps)` 写在 `BitBlt(hdcReal, ...)` 之后。`EndPaint` 之后 BeginPaint 返回的 DC 已被释放，BitBlt 实际写不进去，内存位图里的全部内容都没贴到窗口。
  - 修复：把 `BitBlt` + 释放提到 `EndPaint` 之前。
  - **自检盲区**：`--selftest-nogui` 走 SelfTestMain()，不进主窗口 DialogProc 的 WM_PAINT 路径，所以**这个 bug 自检 104/0 也抓不到**——真机目测是最后一道闸。

## v1.0.9 — 2026-09-10
按真机使用反馈做的 6 项调整：

- **操作系统不再显示安装时间**：界面与导出报告都不再输出"安装日期"（`osInstall` 已停止采集，字段保留置空仅为结构兼容）。
- **「一键复制」改为「导出报告」**：
  - 硬件页右上角按钮由"复制纯文本到剪贴板"改为**把完整报告以 HTML 格式导出到桌面**并自动用默认浏览器打开；按钮反馈态为"已导出"。
  - 报告内容 = 主页面看不到的全部明细：BIOS（厂商 / 版本 / 日期）、每根内存插槽（容量 / 频率 / 厂商 / 料号 / 空槽）、每颗 CPU、全部网卡（MAC / IPv4 / IPv6）、每块物理硬盘、音频、每个 USB 外设、分区与簇大小表、右键功能启用状态。
  - HTML 为 UTF-8（带 BOM）+ 内联 CSS，转义了 `& < > "`，可直接双击打开、另存或发给客户。
  - 新增命令行 `--report "<路径.html>"`：无头导出（不弹窗），可放进开机 / 域脚本做批量巡检（退出码 0=成功 / 2=失败）。
  - 磁盘页仍保留「一键复制」（排查时贴分区信息更方便）。
- **音频 / USB 只列真实识别到的硬件**：
  - 排除软件设备（SW 枚举器）、音频逻辑端点、微软软件合成器、USB 根集线器、通用集线器、USB 复合设备、系统设备等占位项；
  - 一项都没识别到时**整段不显示**，不再写"未知"占位。
- **新增硬盘信息**（主页面原来完全没有）：
  - `\\.\PhysicalDriveN` + `IOCTL_STORAGE_QUERY_PROPERTY`（型号、总线类型）+ `IOCTL_DISK_GET_LENGTH_INFO`（容量）+ SeekPenalty 查询（SSD / HDD），**不依赖任何驱动或 WMI**；
  - 主页面显示每块盘的型号 · 接口（NVMe / SATA / SAS / USB…）· 介质（SSD / HDD）· 容量，最多 4 块，超出归入"其他"；打不开时回退 SetupAPI 取 DiskDrive 友好名。
- **主页面精简为常规信息**：只保留操作系统 / 计算机 / 主板 / 处理器 / 内存 / 显卡 / 硬盘 / 显示器 / 网卡 / 音频 / USB 各一条概览；BIOS、每条内存、每颗 CPU、全量网卡与 USB 明细全部移入 HTML 报告。
- **修复鼠标移到组件上窗口闪烁**：
  - `WM_PAINT` 改为**双缓冲**（内存位图绘制完成后一次性 BitBlt 上屏）—— 纯自绘 + hover 全窗重绘时逐笔绘制过程被逐帧呈现，是闪烁的根因；
  - 新增 `WM_ERASEBKGND` 直接返回 1，禁止系统擦背景造成的多余一次闪白；
  - `TrackMouseEvent` 只在未跟踪时注册，避免每次 `WM_MOUSEMOVE` 反复重置跟踪。
- **体积回归**：报告缓冲由 static 数组改为堆分配（static 数组把 `.data` 段撑大 300KB，exe 从 380KB 涨到 746KB），现为 451KB。
- **自检 92 → 99 项**：新增物理硬盘采集、安装日期不再采集、HTML 报告生成 / 文档头 / 分区表段 / 不含安装日期等断言，全部通过。
- **两轮多子 agent 对抗审查**：修复 Major 1 处（HTML 转义缓冲未初始化导致拼接未终止内存）、Minor 4 处（IOCTL 字符串按显式长度读取、导出路径截断判断、裸 `wcscpy`、`fwrite` 返回值）、Nit 若干；复查确认无 Critical / Major 遗留。

## v1.0.8（v5.1.10）— 2026-09-08
- **项目更名 RightMenuX → WinSuiteX**：
  - 现在项目包含「右键增强 + 磁盘信息 + 硬件配置」三大模块，已超出"右键菜单增强"的语义边界；
  - 新名 `WinSuiteX / Windows 套件`，既保留 X 后缀的命名习惯，也表明是 Windows 桌面工具箱；
  - 资源版本字段、ProductName、FileDescription、内部文件名（`WinSuiteX.exe` / `WinSuiteXShell.dll`）与代码注释同步更新。
- **新 CLSID + 旧版残留清理**（`MigrateCleanupLegacy()`）：
  - 安装新功能前自动清理 RightMenuX 时代的旧 CLSID 注册表项、COM InProcServer32、`C:\Windows\RightMenuXShell.dll`、`%LOCALAPPDATA%\RightMenuX` 等残留；
  - 清理失败不阻塞安装（最佳努力 + 静默日志）。
- **UI 重新设计：三页签结构**
  - 顺序：硬件配置（默认页） / 右键增强 / 磁盘信息；
  - 默认页从右键增强改为硬件配置，因新用户更可能想看本机硬件；
  - 切页时窗口按中心缩放（v1.0.6 引入的方式），3 页高度自适应。
- **新增硬件配置页**（`hardware.cpp` + `hardware.h`，593 + 107 行）：
  - **驱动未装也能识别**：SMBIOS 固件表读 BIOS / 主板 / CPU / 内存插槽；CPUID 指令读 CPU 品牌串；SetupAPI 枚举设备树（设备节点存在即可读，不要求驱动已加载）。
  - **采集字段**：
    - 操作系统（产品名 / 版本 / Build / 架构 / 安装日期）
    - 计算机（计算机名 / 当前登录用户）
    - 主板（制造商 / 产品型号）
    - BIOS（制造商 / 版本 / 日期）
    - 处理器（多 CPU 颗数 + 每颗：型号 / 物理核心 / 线程 / 频率 / 插槽）
    - 内存（总量 MB + 已插/共插槽数 + 每条：插槽 / 容量 / 频率 / 厂商 / 型号）
    - 显卡（多 GPU）
    - 显示器（分辨率 / 刷新率 / 名称）
    - 网卡（型号 / MAC / IPv4，含虚拟网卡）
    - 音频设备（SetupAPI MEDIA/AudioEndpoint + 注册表 HDAUDIO 三路兜底）
    - USB 外设（SetupAPI USBSTOR + HID + Image 三类，过滤根集线器噪声）
  - **Server 系统自动隐藏**音频设备与 USB 外设两段（用 RtlGetVersion 判定 `wProductType == VER_NT_SERVER`）；
  - **自绘"标签：值"两列布局**，每段左侧主色竖条标识，密度与磁盘页表格一致；
  - 顶部「一键复制」按钮把全部硬件信息格式化为纯文本（含分类标题与对齐）写入剪贴板，按钮短暂显示"已复制"反馈（1.3 秒定时器清除）。
- **磁盘页增加一键复制**：原"刷新"按钮右侧加"一键复制"，可把当前所有分区表格数据与详情文本一次性带走。
- **多页签遗留状态清理**：`SwitchTab` 与 `WM_MOUSELEAVE` 都清掉旧页 hover，避免切页后残留高亮直到鼠标移动。
- **多子 agent 对抗审查**：4 agent 并行审（GDI 资源 / DPI 与命中 / 边界健壮性 / 交互），发现并修复 10 处问题，全部复查落实；最终数值验算覆盖 0/1/2/12 分区 × 96/144 DPI，确认不重叠/不裁字。
- **自检 79 → 92 项**：新增 13 项硬件页自检（采集完成 / 各字段非空 / 至少 1 CPU / 内存总量 / 网卡 / 显示器 / 文本摘要），全部通过。

## v1.0.6（v5.1.8）— 2026-09-08
- **UI 重新设计：新增顶部标签页**，主窗口分为「右键增强」与「磁盘信息」两页：
  - 右键增强页是原有 4 个右键菜单功能的开关卡片，布局与交互不变；
  - 磁盘信息页为本版新增的磁盘工具（见下条）。
  - 两页内容高度不同，切页时窗口**按中心缩放**（此前锚定左上角只向下伸缩），并重建圆角与全窗重绘。
- **融合「磁盘簇字节大小查看器」**（原独立项目 `查看磁盘簇字节大小/` 已并入，停止独立维护）：
  - 自绘 6 列表格展示各分区：盘符 / 类型 / 簇大小 / 文件系统 / 总容量 / 可用空间；
  - 点选某行，下方详情区显示完整信息：簇大小换算式（每簇 N 扇区 × M 字节/扇区）、总容量、可用、已用百分比；
  - 右上角「刷新」可重新枚举（插拔 U 盘后可见），并尽量保留原选中盘符；
  - 空光驱等不可访问分区显示 `-` /「无法访问」。
- 健壮性与代码质量（经 4 个子 agent 对抗审查 → 修复 → 复查闭环）：
  - 抑制未就绪介质的系统「请插入磁盘」弹窗（`SetErrorMode(SEM_FAILCRITICALERRORS)`），跳过无效根路径 `DRIVE_NO_ROOT_DIR`，避免阻塞 UI 线程；
  - 容量与簇大小乘法统一先提升 `ULONGLONG` 再乘，避免 32 位溢出；
  - 选中行限制在可见行内（表格上限 12 行），刷新后分区数减少不越界；
  - 表头/列坐标与行框统一「先求和后 Scale」，消除高 DPI 下 1px 错位；详情框加高避免第 3 行文字被裁；
  - 切页清理上一页残留 hover；抽取 `RelayoutWindow()` 消除「重算布局+改尺寸+重建圆角+重绘」重复代码。
- 自检 79/0 通过（沙箱非提权，HKLM 用例走 INFO 分支，非回归）。窗口高度变化与表格观感属目测类改动，需真机确认。
- 版本号 bump 到 `1.0.6.20260908`（四段式）。

## v1.0.5（v5.1.7）— 2026-08-20
- 右键菜单恢复系统默认排序：4 个功能的 `position` 字段由强制置顶 `L"Top"` 改为 `NULL`
  （不写注册表 `Position` 值），第三方菜单项回到系统默认顺序，排在"打开 / 发送到"等
  系统内置项之后，不再强制置顶。
- 升级兼容：安装逻辑在 `position` 为 `NULL` 时主动 `RegDeleteValueSafe` 清除旧版本残留的
  `Position=Top`（RunCommand 与 ShellExtension 两条路径、含 CmdHere 的 3 个父路径均已覆盖），
  避免旧值残留在注册表继续生效。
- 自检增强：新增/修正 Position 校验——
  - RunCommand 三类功能（计算机管理 / 设备管理器 / 命令提示符）主父路径，以及命令提示符的
    文件夹背景 / 桌面背景副父路径，均校验"Position 未设置"；
  - 补齐 SuperHidden（ShellExtension 类）的 Position 校验（此前第 4 个功能无覆盖）；
  - 新增升级模拟用例：安装前主动注入 `Position=Top` 再断言被清除，正面验证升级清理路径。
- 自检 76/0 通过（沙箱非提权，HKLM 用例走 INFO 分支，非回归）。菜单排序为目测类改动，需真机确认。
- 版本号 bump 到 `1.0.5.20260820`（四段式），发布包 `RightMenuX_v1.0.5.zip`。

## v1.0.4（v5.1.6）— 2026-07-28
- 发布前代码质量审查（win-qa）+ 缺陷修复：
  - **S1（严重）**：修复 per-user（HKCU）安装路径在标准用户+干净机器必然失败的问题。COM 服务器 DLL 与图标现按 `perUser` 落到 `%LOCALAPPDATA%\RightMenuX`（标准用户可写、Explorer 同上下文可读），注册表 `InProcServer32` 同步指向用户路径；HKLM/管理员路径保持 `C:\Windows`。标准用户无需提权即可完成「仅当前账户生效」的安装，UI 承诺成真。
  - **N1（一般）**：SuperHidden（显示/隐藏系统文件）在 Win11 新菜单无图标——新增内置「眼睛」图标 `MakeEyeIcon`，`RegisterWin11Command` 对 ShellExtension 类自动生成并写入图标。
  - **N2（一般）**：`ResolveSystemFile` 固定 `MAX_PATH` 缓冲的越界隐患——改用 `MAX_PATH*4` 栈缓冲 + 超长时 `HeapAlloc` 动态分配，确保 null 结尾、不越界、成对释放。
  - **G1/G2（一般，清理）**：`UninstallRunCommand` / `UninstallShellExtension` 卸载时未清理 per-user 图标 `.ico` 的残留——补 `perUser` 透传与 `DeleteFileW` 对称清理（最佳努力）。
- 自检 76/0 通过（沙箱非提权，HKLM 用例走 INFO 分支，非回归）。眼睛图标渲染为目测类改动，需真机确认。
- 版本号保持 `1.0.4.20260724`（四段式），发布包 `RightMenuX_v1.0.4.zip`。

## v1.0.2（v5.1.5）— 2026-07-24
- 项目文档全面对齐 RightMenuX 现状：重写 `PROJECT_ANALYSIS.md`、`ROADMAP.md`（弃用旧名 WinPersonalize 与旧架构描述），更新 `README.md`（补全功能表、四段式版本说明）、`DESIGN_SYSTEM.md`（标题与字号注释）。
- 弃用并删除历史参考项目 `SuperHiddenApp/`（功能已并入 RightMenuX 的 SuperHidden 模块）。
- 清理项目目录：移除构建中间产物（res.o / shellres.o / selftest_report.txt / RightMenuXShell.dll）与旧发布包 `RightMenuX_v1.0.0.19.zip`，以及项目根残留文件。
- 版本号 bump 到 `1.0.2.20260724`（四段式），发布包 `RightMenuX_v1.0.2.zip`。

## v1.0.1（v5.1.4）— 2026-07-24
- 版本号策略改进（用户决策）：
  - 改为四段式 `主.次.修订.编译日期戳(YYYYMMDD)`。
  - 前三段每次编译 修订号+1、满10进1（次/主随之进位），保留“编译推进”语义且位数受限，不再无限累加成 1.0.0.100。
  - 第四段为编译当天日期戳，直观可追溯是哪天编的；Windows `FILEVERSION` 数值段用 `YYMMDD`（<65535）防 16 位溢出，字符串用完整 `YYYYMMDD`。
  - `version_stamp.py` 重写：默认每次编译自动递增前三段+写入日期戳；`--set X.Y.Z` / `--bump patch|minor|major` 仍可手动发版。
  - 主界面底部版权行通过 `LoadString(IDS_VERSION)` 显示版本号（如 `© 重庆持玛多网络科技有限公司 · Z.W.  v1.0.1.20260724`）；`app.rc` 新增 `STRINGTABLE` / `IDS_VERSION=102`，`resource.h` 同步定义。

## v1.0.0.19（v5.1.3）— 2026-07-24
- UI 微调（cpp-pro 执行）：
  - 功能项名称 15→16px、说明文字 14→15px（卡片高度 108→116 联动，状态徽章下移防重叠）。
  - 页脚提示 13→14px、底部版权 12→13px（绘制矩形同步加高防裁切）。
- 版权信息 `© 重庆持玛多网络科技有限公司 · Z.W.` 已在界面底部绘制（与 exe 属性一致），本次仅随字号放大保持清晰。
- 构建：73/0 自检通过（沙箱非提权，HKLM 写入走 INFO 分支，非回归）。UI 为目测类改动，需真机确认。

## v1.0.0.18（v5.1.2）— 2026-07-23
- 内存安全体检（cpp-pro + 自审）：
  - `SubstituteSelectionPath` 弃用 `MAX_PATH` 固定缓冲，改用 `GetDisplayName` 返回的 CoTaskMem 堆串（防深目录 >260 字符截断）。
  - `MakeTerminalIcon` 修复 `hbmMask` 兜底 `CreateBitmap` 路径句柄泄漏。
  - `WriteIcoFromHICON` 防御 NULL 位图。
  - `RegWriteString` 防御 NULL value（wcslen 越界）。
  - 自检 `TestWin11ComServer` 补 `CoInitializeEx` / `CoUninitialize`。
- Windows 10+ / Server 兼容性审计：
  - 64 位产物匹配 64 位 Explorer，无 WOW64 问题。
  - API（`RegDeleteTreeW` / `SHGetKnownFolderPath` / `IExplorerCommand` / `GetModuleHandleExW`）均 ≥ Vista，覆盖 Server 2016/2019/2022。
  - 桌面背景目录回退改用 `FOLDERID_Desktop`。
- **真机实测**：Windows Server 2022 (Desktop Experience) 右键项正常显示与工作。

## v1.0.0.17（v5.1.1）
- 修复「显示/隐藏 停用误报」（注册表删除被拒绝的误判）。
- 修复「命令提示符未注册到背景」。

## v1.0.0.x（v5.1）
- 混合架构落地：管理器 exe + 独立 COM 服务器 DLL（无提权清单），DLL 内嵌 exe 并由 `EnsureShellDll()` 释放。
- 解决 %V 占位符在 Win11 新菜单失效（COM 服务器 Invoke 中 `SubstituteSelectionPath` 替换）。

## 更早
- v4.x：2 文件方案（exe + DLL 无提权清单），运行正常。
- v5.0：单文件 EXE-as-COM-server 尝试 → 失败（带提权清单的 exe 不能被 Explorer 加载为 InProcServer32）。
