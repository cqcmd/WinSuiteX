# WinSuiteX 项目长期记忆（精简版，细节看 .workbuddy/memory/ 当日日志）

## 代码分工红线
- C++ 版（`dist/客户机/winsuitex.exe`）冻结只读；只维护 Rust 版（rust/ workspace，产物 `dist/winsuitex.exe` CLI + `dist/winsuitex-gui-gdi.exe` GUI）。对拍差异以 Rust 为准。
- 5 crate：core(模型) / platform(Win32 采集) / report(渲染) / gui-gdi(面板) / cli。

## 门禁与部署（铁律）
- 改完必跑：`cargo fmt --check`；`cargo clippy --workspace --all-targets --all-features -- -D warnings`；`cargo test --workspace`；发版加 `tools/verify_gui_resource.py`。
- 改共享 crate（platform/core/report）必须重编并部署 GUI exe：`taskkill //IM winsuitex-gui-gdi.exe //F` → build → cp 到 dist/。CLI 重渲只是验证不是交付。

## Win7 通用 exe（2026-10-27 起 GUI+CLI 都走此路线）
- 构建：`RUSTFLAGS="--cfg winsuitex_win7 -C target-feature=+crt-static" cargo +nightly build --release -Z build-std=std,panic_abort --target x86_64-win7-windows-msvc -p winsuitex-gui-gdi -p winsuitex-cli`。Tier3 无预编译 std，`rustup target add` 报错属预期（build-std 自编）。
- **windows-rs 按模块整块链接"连坐"**：用到某模块一个函数，整个模块含 Win8+ 函数全进 IAT。Win8+ 符号必须在 `platform/src/win7_compat.rs`（仅 cfg winsuitex_win7 编译）加 `#[no_mangle]` 强符号覆盖：运行时 `GetProcAddress` 探测真函数，Win7 回退等价实现（CoTaskMemFree→ole32；GetSystemTimePreciseAsFileTime→GetSystemTimeAsFileTime；DPI 四函数→SetProcessDPIAware/96）。`load_fn` 返回原始 fn 指针、调用点 transmute（泛型 F 直接 transmute 报 E0512）。
- 验收以 PE 导入表为准（objdump -p），零 api-ms-win-*/combase/Win8+ 符号才算过；本机 Win10/11 实跑 + gdi_shot.py 截图佐证。

## 版本号 / dist
- 四段式 `主.次.修订.日期戳`；GUI bump：`python ~/.workbuddy/scripts/version_stamp.py --root rust/crates/winsuitex-gui-gdi`（现 0.5.x 线）；**CLI 禁日常 bump**，只 `--set 0.1.0` 刷日期戳（保 workspace parity）。禁 `--bump patch`。发版才 `--set`。
- dist 只留 6 类文件；bak 只留最近 2；不打 zip（发版 zip 用 `tools/release_zip.py <版本戳>`）。GUI/CLI 资源（图标+VERSIONINFO+清单）经 build.rs rc.exe 链入；rc.exe 五坑见 2026-10 日志。

## GDI 观感铁律（勿回退）
图标全自绘禁 emoji；DrawTextW 硬裁 RECT 右界给够；滚动钳制读→算→clamp→写回；命中坐标=绘制坐标（滚动区先减 scroll_y，标题栏用 screen_y）；ROW_H=22 列宽按权重；无自绘阴影 SHADOW_PAD=0；窗口尺寸 min(scale(WIN_*), 工作区)（勿改回裸 scale，低分屏出屏）；玻璃 GLASS_ALPHA=0.50 固化勿调，卡片压暗(C_GLASS_TINT 深 navy)不提亮，白色蒙层(0.08~0.22)是提亮不属玻璃勿"统一"；标题栏两键（最小化/关闭）；硬件页 WIN_H=948+滚动条；量高/绘制双趟必须同宽（给行加右侧预留必须同步量高趟）。

## 关键 API 坑（windows-rs 0.62 / GDI）
ComGuard{own:false} 永不 CoUninitialize；ExtSelectClipRgn(相交)勿用 SelectClipRgn(替换)；滚动内 region 坐标手动减 scroll；CreateFontW 字体名逐码点核对+GetTextFaceW trim NUL+FONT_GOOD_FACES 中英都认；DIB→GDI+ PNG 必补 alpha=0xFF；BOOL 返回 .as_bool()；WM_MOUSELEAVE 在 UI::Controls、TME_LEAVE 每次移动重注册。API 详情见当日日志。

## 单源规范
- 品牌 `winsuitex_core::brand_display_or` / GPU `gpu_brand_for_display`，格式 `英文（中文）` 全角括号。
- `fmt_link_speed`/`real_link_speed` 在 core pub 单源。网卡只显示已连接+多 IPv4；SMART 温度滤 Some(0) 假数据；空槽合并一行。
- 磁盘页 9 列权重 `[8,5,9,9,8,10,13,14,6]`（增删列按最小宽重算，中文表头 4 字≈48px）。

## 单实例 / 其他
- 单实例静默切前台不弹窗（`WinSuiteX.GDI.SingleInstance`，回归 tools/single_instance_check.py）。
- 仓库：github.com/cqcmd/WinSuiteX（origin+main+tag 已推）。推送认证用 GitHub PAT 嵌 URL、用完 set-url 重置；**沙箱网络一律 curl（urllib 被拦）**，uploads 要 `--ssl-no-revoke`，git push 加 `-c http.version=HTTP/1.1 -c http.schannelCheckRevoke=false`。
- CHANGELOG/README 有 Rust/C++ 两线说明，改版本要同步。
