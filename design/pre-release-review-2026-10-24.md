# WinSuiteX 发布前审查报告（v0.5.5.20261023）

- **审查类型**：Workflow B（已有代码审查）→ 质量保障专家 win-qa（质守严）主审，主理人（定盘星）二次复核门禁
- **审查范围**：Rust 重写线 GUI（`rust/crates/winsuitex-gui-gdi` 及其依赖 core/platform/report），C++ 冻结线除外
- **审查结论**：✅ **GO（可发布）**
- **复核原则**：用户铁律「不许假成功」——主理人对四道门禁全部实跑复核，非采信 agent 自报

---

## 1. 门禁结果（主理人实跑复核，真实证据）

| 门禁 | 命令 | 结果 |
|---|---|---|
| fmt | `cargo fmt --check`（rust/） | EXIT=0 ✅ |
| clippy | `cargo clippy --workspace --all-targets --all-features -- -D warnings` | EXIT=0，零警告 ✅ |
| test | `cargo test --workspace` | EXIT=0，**366 passed / 0 failed** ✅（2+3+71+6+53+148+83） |
| 资源校验 | `tools/verify_gui_resource.py dist/winsuitex-gui-gdi.exe` | EXIT=0，**RESULT: PASS** ✅ |

资源校验明细：
- CompanyName = 重庆持玛多网络科技有限公司
- LegalCopyright = 持玛多网络 / Z.W.
- ProductName = WinSuiteX / OriginalFilename = winsuitex-gui-gdi.exe
- 图标组 #101（RT_GROUP_ICON，48×48）✅
- 清单 #1（RT_MANIFEST，1677B：asInvoker + PerMonitorV2 DPI + supportedOS）✅

---

## 2. 分维度发现

### 🔴 阻断级：0

### 🟠 严重级：0

### 🟡 一般级：1
- **COM 音频采集线程的 STA/MTA 套间意图不符**
  - 位置：`winsuitex-platform` 音频采集路径（WMI/MMDevice 调用）
  - 现象：当前线程套间模型与音频设备枚举 API 期望的 STA 不完全匹配
  - 影响：**已确认 fail-safe**——采集失败仅导致声卡列表留白、不影响主流程、不崩溃、不误报；非阻塞
  - 建议：发版前在**带声卡的物理机/真机**上冒烟一次确认声卡列表正常；如需精确，补 `CoInitializeEx(COINIT_APARTMENTTHREADED)` 显式声明

### 🟢 建议级（非阻塞，可后续优化）
1. **字体对象每帧重建**：`draw.rs` 部分路径每帧 `CreateFontW` 后未复用缓存，高频重绘有微小幅创建开销。建议引入字体缓存（按字号+粗细+语言命中）。
2. **渐变/玻璃逐行刷子**：整窗渐变按行 `CreateSolidBrush` 构建，长窗口有少量 GDI 对象抖动。建议改一次离屏位图后 `BitBlt`。
3. **已知 TODO 增强项**（不阻塞本次发布，来自项目记忆）：
   - 磁盘页「一键复制」
   - 板载声卡芯片型号（ALC897 走 WMI）
   - 内存类型枚举补 HBM / LPDDR5 / HBM3
   - 真机复验玻璃主题 + frameless 交互（本会话已做三页目检 + 断言，通过）

---

## 3. 内存与资源安全审计（win-qa 主审）

- **GDI 对象全配对**：HBITMAP / HDC / Brush / Pen / Region 均 `SelectObject` 配对 + `DeleteObject`/`DeleteDC`/`ReleaseDC` 闭合；Backbuffer / AaSurface 成对释放；GDI+ 全路径释放。**无泄漏**。
- **COM guard**：`ComGuard{own:false}` 永不 `CoUninitialize`（守住历史 STATUS_HEAP_CORRUPTION 坑），单一常驻采集线程。
- **单实例**：`CreateMutexW` + `FindWindowW` 跨线程正确，二次启动静默切前台、本进程立即退出（单实例回归 PASS 3/3）。

---

## 4. 铁律回归（全部有断言钉死且通过）

| 铁律 | 落点 | 状态 |
|---|---|---|
| 玻璃透明度固化 50% + 深海军蓝 tint(20,24,52) | `theme.rs` `GLASS_ALPHA` / `tint_color()` / `theme_constants_regression` | ✅ |
| 4 皮肤 N 路 + 越界钳制 `min(id, THEME_COUNT-1)` | `theme.rs` `set_theme` / `theme()` | ✅ |
| 共享 token 层（S_* 常量）跨皮肤值一致 | `shared_tokens_identical_across_all_skins` | ✅ |
| AA 3x 超采样：准入 radius≥3 且边≥16、>24MB 降 2x、失败降级 | `draw.rs` `AaSurface` / `aa_worthwhile` | ✅ |
| ZSUI 开关三铁律（开大白/关小灰/开无描边关 1px） | `pages.rs` `draw_switch` | ✅ |
| 字体回退链 + 中文本地化名 trim + 中英文都认 | `draw.rs` `real_face` / `FONT_GOOD_FACES` | ✅ |
| DIB→GDI+ ARGB 补 alpha=0xFF | `draw.rs` 截图路径 | ✅ |
| `SelectClipRgn` 用 `RGN_AND`（非替换） | `pages.rs` 滚动裁剪 | ✅ |
| 视口偏移下 AA 落点正确（逻辑坐标，不双倍补偿） | `draw.rs` `AaSurface` / `aa_draws_at_logical_coords_under_viewport_offset` | ✅ |

---

## 5. 发布产物合规

- 版权署名「持玛多网络 / Z.W.」✅
- 版本四段式 `0.5.5.20261023`（FileVersion 字符串 + 数字段占位，build.rs 抽取注入底栏 `env!("WSX_GUI_VERSION")`）✅
- 图标 #101 / 清单 #1 齐全 ✅
- `dist/` 整洁（6 类：bat/html/json、inventory_debug.bat、两个 exe、客户机/）✅
- exe 与 main 同源：含四皮肤字符串 + 版本号（本会话三页真机目检已确认渲染正确）✅

---

## 6. 发布建议（GO）

**可立即发布 `v0.5.5.20261023`**。发布动作清单：
1. 仓库已单主线 `main`，标签 `v0.5.5.20261023` 就位；
2. 分发 `dist/winsuitex-gui-gdi.exe`（单文件、零依赖、双击即跑）；
3. 发版前建议补一次**带声卡真机冒烟**（验证 🟡 一般项，非阻塞）；
4. 发版后同步 CHANGELOG.md / README 双线说明（Rust 线 0.5.x + C++ 冻结 1.7.x）。

> 注：win-qa 在受限环境下 `SendMessage` 不可用，故以结构化全文作为最终交付；主理人已独立实跑四门禁复核，结论与 QA 一致。
