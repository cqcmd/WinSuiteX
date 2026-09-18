# WinSuiteX 项目长期记忆

## 代码分工红线（2026-09-16 Z.W. 明确）
- **C++ 版本（`WinSuiteX.exe`，`main.cpp` / `hardware.cpp` / `hardware.h` / `selftest.cpp` 等）一律视为冻结**：Z.W. 要求我**以后不要再动 C++ 版本的任何代码**。
- **我只负责 Rust 版本**（`rust/` 下的 workspace：winsuitex-core / winsuitex-platform / winsuitex-report / winsuitex-cli，产物 `dist/winsuitex.exe`）。
- 例外场景（仅当 Z.W. 明确点名要改 C++ 时）：默认不碰；若用户主动要求，再单独确认后再动。
- 历史遗留：C++ 版曾是我（AI）改的（如内存空槽表头逻辑、版本戳、自检用例），但自本约定起不再维护。Rust 与 C++ 对拍时的差异，以 Rust 为准，不要为了对齐去改 C++。

## 双版本产物现状（截至 2026-09-16）
- Rust：`dist/winsuitex.exe`（report crate 渲染层做中文厂商名映射、占位串清洗、空槽表头=原始 NO DIMM）。
- C++：`WinSuiteX.exe`（老版，与 Rust 功能大体对齐但不再演进）。

## 记忆/技能纪律
- 改完 Rust 直接编译（`cargo build --release`）+ 部署 `dist/winsuitex.exe` + 重渲验证，门禁：`cargo fmt --check` / `clippy --all-targets --all-features -- -D warnings` / `cargo test --workspace` 全绿。
- 占位串（NO DIMM / Unknown / To Be Filled By O.E.M. / Default string 等）只在 Rust **渲染层**归一，不动 JSON / core model，保住老存档与对拍兼容性。
- 中文厂商名映射原则：绝不错译（unknown 厂商原样返回英文），以 `英文 (中文)` 括号内联。

## dist 目录整洁约定（2026-09-18 Z.W. 确认后执行）
- **`dist/` 只应保留 6 类文件**：`winsuitex.exe`、`inventory.bat`、`inventory_debug.bat`、
  `inventory.json`、`inventory.html`、`wmi_debug.log`。
- Z.W. 远程作业、靠手动复制文件分发到客户机，**目录里堆旧 exe 会让他挑错文件**。
  2026-09-18 已清理：22 个 `.bak_*`（11 MB）+ 12 个调试残留（crash/probe/rep_a/t45test 等）
  + 1 个误重定向产生的名为 `-` 的文件。
- **部署时必须裁剪备份，只留最近 2 个**（dist 与根目录各 2 个）：
  `ls -1t dist/*.bak_* | tail -n +3 | xargs -r rm -f`
  不裁剪的话一天能攒 20+ 个（实测一天 22 个）。
- **不再打 zip 包**：Z.W. 明确说"文件我直接复制就行了"，不要自作主张打包。

## 门禁必须跑三项（我曾在十几轮里漏了第一项）
1. `cargo fmt --check`
2. `cargo clippy --workspace --all-targets --all-features -- -D warnings`
3. `cargo test --workspace`
- 2026-09-18 补跑时发现树**早就不是 fmt-clean**（`winsuitex-core/src/model.rs` 有我从未碰过的漂移），
  说明此前多轮一直只跑了后两项。已 `cargo fmt` 收敛（需跑两次才稳定收敛，属 rustfmt 常态）。
- **改完源码后必须重跑全部三项再部署**——`cargo fmt` 只改格式不改语义，但仍需重新验证。

## 版本号（Rust 侧）
- 四段式 `主.次.修订.编译日期戳(YYYYMMDD)`，如 `0.1.0.20260918`；
  `--version` 输出形如 `winsuitex 0.1.0.20260918  (built 2026-09-18 01:54:05 本地)`。
- 日期戳取**本地时间**（读注册表 `ActiveTimeBias`）；拿不到偏移就退回三段式，**绝不显示可能差一天的日期**。
- JSON 快照顶部也带 `app_version` / `build_time`，便于归档后回溯是哪个版本采的。
- 判据不只是 `--version`：批处理会打印 exe 的 mtime + size，JSON 里有 `collected_at`。

## 排查纪律（本项目高频踩坑，务必遵守）
- 先读结果里的**归因字段**（如 `smart.source`）再猜实现，它能直接告诉你哪条路真走通了。
- 日志要记**清洗前原始值**，否则分不清"数据源没返回"和"返回了但被判废"。
- 报"发现问题"前**先确认字段名/取值口径**（我曾把网卡字段 `desc` 当成 `name`，误报一次假故障）。
- 本机 VM 无 NVMe，**任何 NVMe 相关结论都必须靠真机数据或官方文档，不靠记忆**。
