# WinSuiteX 项目长期记忆

## 代码分工红线
- C++ 版（`dist/客户机/winsuitex.exe`）冻结，只作只读参照；只维护 Rust 版（rust/ workspace，产物 `dist/winsuitex.exe` + `dist/winsuitex-gui-gdi.exe`）。除非 Z.W. 点名改 C++，否则不动。Rust 与 C++ 对拍差异以 Rust 为准。
- workspace 仅剩 5 crate：`winsuitex-core`(模型/trait 无平台 API) / `winsuitex-platform`(Win32 采集) / `winsuitex-report`(HTML/文本渲染) / `winsuitex-gui-gdi`(面板) / `winsuitex-cli`。egui/coreui 废弃版已删（第四十一轮）。

## GUI 路线 + 玻璃主题
- 主路线 `winsuitex-gui-gdi`：纯 Rust + windows 0.62 + GDI 自绘，单 exe 零 dll 零 GPU（≈680KB），VM/RDP 都能画。
- 整窗渐变 `(135,81,238)`→`(38,98,235)`；底栏实色 `(27,54,122)`。半透明靠逐行混合（GDI 无原生玻璃）。
- **卡片压暗不提亮**：内容卡 `C_GLASS_TINT=(20,24,52)` 深 navy 0.22/0.30，白字对比度 6.7~7.5:1（提亮仅 3.5:1，Z.W. "字看不清"根因）。
- **图标柔和粉彩**（第九轮定案）：`C_ACCENT_SOFT[8]` = 高饱和 C_ACCENT×0.55 + C_ICON×0.45。禁用 emoji（豆腐块）；禁用"一刀切淡紫"和"高饱和 8 色彩虹"两端。
- 字号 body14/sub13/tiny12/section15；字段名/提示用 `C_ON_GRAD_DIM`。

## GDI 观感铁律（勿回退）
1. 图标全 GDI 自绘线段，禁 emoji。
2. `DrawTextW` 单行硬裁剪，RECT 右界给够。
3. 滚动钳制"读→算→clamp→写回"，禁 `fetch_add` 先累加。
4. 命中坐标=绘制坐标；内容区缓存矩形是内容系、鼠标 lparam 是窗口系，命中前 `y -= scroll_y()`；标题栏/页签/头部按钮用 `screen_y`（第二十六轮：on_mouse_move 漏改致滚到底按钮失效）。
5. 行高 `ROW_H=22`；列宽按权重分配，禁写死。
6. 无自绘阴影，`SHADOW_PAD=0`，圆角交 `SetWindowRgn`。
7. 绘制/命中静态几何必须有显式同步点（`WIN_CACHED_W` 在 `on_paint` store 真实 `w`）。
8. 窗口描边用中深灰 `C_WIN_BORDER`，不用卡片灰。
- frameless：`WS_POPUP`+`WS_EX_APPWINDOW`，拖动/缩放/双击最大化/圆角全自实现；`WM_GETMINMAXINFO` 限 `ptMaxTrackSize` 到工作区；`ZOOMED` 存最大化态。
- **标题栏只有两键**（第十轮）：`─ 最小化 | ✕ 关闭`，无最大化键（还原走双击标题栏）。`hit_test` 把 `(right-x)/bw` "从右数"显式映射回绘制序（最左=0 最小化、最右=1 关闭）。
- **硬件页固定 `WIN_H=948` + 滚动条**（第二十轮）：不自适应长高、不压行高、不裁分组、不做双列（理由见当日日志，勿再提议塞一屏）。⚠️ 但**窗口实际尺寸受工作区钳制**：`max_win_w()/max_win_h() = min(scale(WIN_*), 工作区)`（`main.rs`），建窗尺寸/`center_window`/`WM_GETMINMAXINFO`/`WM_DPICHANGED`/`on_paint` 五处统一钳到工作区——小屏（1024×768 工作区≈738<948）窗口高=738、标题栏贴屏顶、内容滚动；大屏仍取设计尺寸（零回归）。**勿改回裸 `scale(WIN_*)`，否则低分辨率窗口出屏**。
- **磁盘页自适应窗口高度仅 tab 2 未最大化时**（`desired=clamp(DISK_MIN_H=360, max_win_h())`，上限**已非** `WIN_H` 而是 `max_win_h()` 以便小屏也进工作区；`SetWindowPos` 只改高，记得 `apply_rounded_region`）。

## 关键 API 坑（windows-rs 0.62）
- **COM 采集**：`ComGuard { own:false }` 永不 `CoUninitialize`，否则 WMI 释放交叉 → STATUS_HEAP_CORRUPTION。单一常驻采集线程。
- **Region 是设备坐标、不随视口偏移移动**（第十九轮）：滚动变换内用 region 裁剪，坐标必须手动减 `scroll`。
- **⚠️ SelectClipRgn 是"替换"不是"相交"**（第二十四轮）：滚动内裁剪一律 `ExtSelectClipRgn(hdc, Some(hrgn), RGN_AND)`，否则裁剪区变全局、画到固定区（空玻璃横条）。
- **⚠️ GDI 字体三连坑（第二十八轮，勿回退）**：① `CreateFontW` 请求不存在字体名不报错静默换宋体——`FONT_UI` 曾多 `0x4E`("微软雅黑N")全机器宋体，字体名逐码点核对；② `GetTextFaceW` 返回长度多算结尾 NUL，`real_face` 统一 trim；③ 中文系统返回本地化名「微软雅黑」，`FONT_GOOD_FACES` 必须英文+中文都认。回退链：首选→YaHei→YaHei UI→Segoe UI→空名兜底。
- **⚠️ GDI DIB → GDI+ ARGB 编码必须补 alpha=0xFF**（"截图没内容"教训）：`COLORREF` 写 32bpp DIB 时 alpha 恒 0 → 全透明 PNG。PNG 走 GDI+（GdiplusVersion=1；PixelFormat32bppARGB=0x26200A 手写；PNG encoder CLSID 手写 GUID）。回校验 `tools/check_shot.py`（alpha 占比/颜色数/白字墨水三道断言）。
- `WM_MOUSELEAVE(675)` 在 Win32::UI::Controls；`TrackMouseEvent` 的 TME_LEAVE 每个 WM_MOUSEMOVE 重注册。

## 厂商品牌显示规范（第二十轮 Z.W. 定案，单一数据源）
- 全产品统一 `英文（中文）` 全角括号（`Supermicro（超微）`）；en==zh（AMD/PNY/ODM）只显示一段。
- 唯一入口 `winsuitex_core::brand_display_or`（vendor_zh.rs 三元组表 `(key, 规范英文, 中文)`），支持中文输入反查。长尾写法收敛规范英文名（`AMI`→`American Megatrends`）。
- **GPU 板卡品牌**：`winsuitex_core::gpu_brand_for_display(vendor, name)`（vendor 优先 + ASPEED/MATROX 按 name 兜底 + `英文（中文）`）；名称已含该品牌（单段）时去重不重复挂。GUI 报告共用同一份。
- 网卡/显示器 label 是"名称（中文）"追加结构，勿改嵌套括号。

## 硬件页功能增量（build_hw_sections，pub(crate)）
- **网卡**：只显示已连接（`filter(|n| n.up)` 须在 `.enumerate()` 之前）；每行带速率（`fmt_link_speed`，哨兵/0 用 `real_link_speed` 拦掉）；**多 IPv4 全部显示**（逗号连接，经 `winsuitex_core::nic_ipv4_for_display`）；有真实地址时隐藏 APIPA 兜底地址、仅已连接且无真地址才显示；面板只显示 IPv4（IPv6 在报告）。
- **音频**：逐设备列真实声卡（名+厂商+类型标签音箱/耳机/耳麦+默认播放/录音/蓝牙标记）；采集层 `audio.rs` 读 `SPDRP_FRIENDLYNAME` 优先。
- **主板**：新增「芯片组」行（型号后、BIOS 前，None 不加行）——`platform/chipset.rs` SetupAPI 枚举 PRESENT 设备按 FriendlyName 含 "Chipset" 命中（优先含 Series 家族名、裁 Family/Controller 后缀），未命中→None 诚实留白（VM 无 Chipset PnP 设备，不显示属预期）。
- **显卡**：每行「显卡 #N」，值=型号+显存+`gpu_brand_for_display` 品牌（虚拟/软件适配器改标类型、不渲染品牌）。
- **SMART 健康徽标**（第二十九轮）：硬盘分组每行右侧彩色小条+状态词；读不到 SMART→None→不画（诚实留白不误报）；危险红底白字、绿/橙/淡紫底深字。
- **空槽**：`MemStick::is_populated()` 判，合并成"N 条未安装+位置"一行；键值行值列 `kv()` 按 ` · ` 贪心折行（`wrap_value`，量高趟必须同函数）。
- **⚠️ 量高/绘制双趟同宽（第三十一轮，勿回退）**：凡给某类行加**值列右侧预留**（如硬盘健康徽标收窄 `DISK_BADGE_RESERVE=64`），量高趟必须同步用同一收窄宽折行——否则长值行"量 1 行画 2 行"→总高量矮→滚动到底最后一组被裁、截图长图同步裁尾。回归：`hw_measure_height_accounts_badge_narrowed_wrapping`（有/无徽标两形态总高必须不同；口径助手 `#[cfg(test)] hw_value_widths`）。以后再给行加右侧预留（速率/温度色等）必须同步改量高趟。

## 磁盘页
- 7 列（盘符/磁盘/文件系统/簇大小/类型/总容量/可用），列宽按权重，`col_x` 长度 NCOL+1。表头首列顶格 `col_x[0]+scale(2)`（与分区图标左缘齐平，第二十七轮"表头左边空档"定案）、`numeric=i>=5` 右对齐。
- 选中分区详情卡 SMART 健康：经 `v.disk_index` 关联 `disks[i].smart`，取 health/temp/percent；拿不到写"未读取（可能需管理员）"。详情卡高 `scale(116)`。

## 报告（HTML 导出）
- `winsuitex-report` crate；**GUI「导出详细」也走此 crate**（`pages.rs:1996 build_html_with_features`）。C++ 版冻结、仍生成旧 verbose 样式。
- 内存段：空槽合并一行「空槽 N 个」；单通道提示（插 1 条且有空槽）；重复 locator 消歧（追加 CHANNEL X）。
- `fmt_link_speed`/`real_link_speed` 是 `winsuitex_core` pub 单源（报告与 GUI 共用）。

## 门禁（改完必跑）
`cargo fmt --check`；`cargo clippy --workspace --all-targets --all-features -- -D warnings`；`cargo test --workspace`。
- **⚠️ 改共享 crate 必部署 GUI exe（铁律，Z.W. 纠偏："不是让你在结果中渲染，是要解决程序中的问题"）**：凡改 `winsuitex-report`/`winsuitex-core`/`winsuitex-platform` 等 GUI 依赖的 crate，**必须** `taskkill //IM winsuitex-gui-gdi.exe //F` → `cargo build --release --bin winsuitex-gui-gdi` → `cp rust/target/release/winsuitex-gui-gdi.exe dist/`。CLI 重渲只是验证，不是交付。

## 单实例（Z.W. 定案：不提醒，直接切换）
- 只允许一个实例：二次运行静默把已跑实例切前台（最小化先 `SW_RESTORE`），本进程立即退出，不弹 MessageBox。`CreateMutexW(None, false, "WinSuiteX.GDI.SingleInstance")` + `FindWindowW` 激活；判重放 `main` 最前。名字不加 `Global\` 前缀。
- 回归 `tools/single_instance_check.py`。

## dist 整洁 / 版本号
- 只留 6 类文件；bak 只留最近 2；不打 zip。
- **Rust GUI 资源已嵌入**（winsuitex-gui-gdi crate）：`build.rs` 调 MSVC `rc.exe`（`C:\Program Files (x86)\Windows Kits\10\bin\10.0.26100.0\x64\rc.exe`）编译 `app.rc`→`app.res` 链入 exe，含 **图标(IDI_APP_ICON=101) + VERSIONINFO(公司名/版权/版本) + 应用清单(UAC=asInvoker、PerMonitorV2 DPI、supportedOS)**。`app.ico` 从根目录复制进 crate（单一真源=根 app.ico）；`version.ini` 在 crate 内独立跟踪（现 `0.2.x` 线，0.1.9→0.2.0 是修订满 10 进 1，与冻结 C++ 管理器 `1.7.x` 分开）。验证用 `tools/verify_gui_resource.py`（GetFileVersionInfoSizeW>0 且 err≠1813 + FindResourceW 查图标组#101/清单#1）。
- 版本号四段式 `主.次.修订.日期戳(YYYYMMDD)`；**GUI 版本号 bump 命令**
  `python ~/.workbuddy/scripts/version_stamp.py --root rust/crates/winsuitex-gui-gdi`
  （C++ 冻结，平时编译不再无参跑根 version_stamp）；发版才 `--set X.Y.Z`；禁用 `--bump patch`。
- 面板底栏版权行（pages.rs:245）经 `env!("WSX_GUI_VERSION")` 显示完整 `主.次.修订.日期戳`（build.rs 从 app.rc 的 `VALUE "FileVersion"` 抽取注入），与文件属性版本号同源。
- ⚠️ rc.exe 5 坑（勿回退）：① `#define VS_VERSION_INFO 1`（rc 找不到 winver.h 会把字面当字符串名→GetFileVersionInfo 返回 1813）；② 链 app.res 用 `CARGO_MANIFEST_DIR` 绝对路径；③ FILEVERSION 第四段禁放 YYYYMMDD（>65535 溢出），数字段用 `0,1,0,0` 占位、完整日期戳放字符串（version_stamp 写 MMDD 到数字段也安全）；④ 改 app.rc 后靠 build.rs `rerun-if-changed=app.rc` 强制重链；⑤ 嵌清单加 `cargo:rustc-link-arg=/MANIFEST:NO` 避免与链接器默认清单冲突。

## Rust GUI 待办
- 磁盘页"一键复制"；板载声卡芯片型号（ALC897 走 WMI）；内存类型枚举补 HBM/LPDDR5/HBM3；真机复验玻璃主题+frameless 交互+10 分组滚动。
