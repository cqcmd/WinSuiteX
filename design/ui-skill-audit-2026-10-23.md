# UI 技能库审查报告（2026-10-23）

> 审查人：颜值（UI 设计师视角）
> 范围：全部用户级技能中 13 个 UI/GUI 相关技能
> 四问：①覆盖面是否全面 ②是否同时适用 C++ 与 Rust ③横评同类 ④哪些不必保留

---

## 0. 总表

| 技能 | 定位 | 语言适用 | 规模 | 结论 |
|---|---|---|---|---|
| djjz-gdi-widget-drawing | 控件画法卡（度量/配色/状态/AA） | 配方通用，落地栏偏 Rust | 273 行 + 深参 | ✅ 核心，保留（需补双语） |
| djjz-gdi-glass-borderless | 窗口壳层（无边框/玻璃/拖拽/抗闪烁） | **仅 C++** | 158 行 + cpp 参考 | ✅ 核心，保留（需补 Rust 侧） |
| win32-gdi-custom-notice | C++ 自绘模态提示框 | **仅 C++** | 114 行 | ⚠️ 合并后退役候选 |
| djjz-gui-render-nogpu | 无 GPU 渲染验证（4 框架） | **双语齐全** | 293 行 | ✅ 保留（验证方法论） |
| djjz-rust-msvc-gui-res | Rust exe 资源嵌入（图标/版本/清单） | **仅 Rust** | 119 行 | ✅ 保留（编译规范铁律） |
| egui-rust | egui/eframe 落地 | **仅 Rust** | 226 行 | ⚠️ 观察名单 |
| rust-desktop-applications | Rust 桌面选型（Tauri/egui/iced/slint） | **仅 Rust** | 228 行 | ✅ 保留（与上配对） |
| fltk-1-4-5 | FLTK 工具箱手册 | **仅 C++** | 91 行 | ⚠️ 观察名单 |
| imgui-ui-ux-engineering | Dear ImGui UI/UX 工程 | **仅 C++** | 149 行 + 全套件 | ⚠️ 观察名单 |
| huashu-design | HTML 高保真原型/幻灯片/动画 | Web | 527 行 + 全套件 | ✅ 保留（web 产出主力） |
| web-design-engineer | HTML 设计工程（stunning 取向） | Web | 494 行 + 参考 | 🔻 退役候选（与上重叠 ~70%） |
| refactoring-ui | Web 设计规则/诊断层 | Web（Tailwind 映射） | 221 行 + 脚本 | ✅ 保留（需限缩触发） |
| windows-shell-feature-dev | 注册表/Shell 扩展（MinGW C++） | 仅 C++ | 156 行 | ✅ 保留（不属 UI 族，勿混） |

---

## 1. 问题一：覆盖面是否全面 —— 骨架全，缺四块「细」

现有体系已是完整的三层结构：
**窗口壳层**（glass-borderless）→ **控件画法**（widget-drawing）→ **渲染验证**（nogpu），
ZSUI 21 张画法卡也补上了未用组件的预案。作为「画法/度量」库是全面的。

但以设计系统标准（目标用户：Z.W. 的 GDI 单 exe 工具；标准：细腻、成体系、可访问）审视，缺 4 块：

| # | 缺口 | 现状 | 影响 |
|---|---|---|---|
| 1 | **字体排印 token（Type Scale）** | widget-drawing 只有字宽估算 7.2dp/字符；ZSUI 的 TypographyToken（Caption 12 / Body 14/20 / BodyStrong / BodyLarge / Subtitle / Title / TitleLarge / Display）没沉淀；项目自有 body14/sub13/tiny12/section15 散在 memory | 字号不成体系，新控件字号靠拍脑袋，「细」的第一杀手 |
| 2 | **图标画法体系** | 项目 icons.rs 的 GDI 线段图标 + C_ACCENT_SOFT 粉彩 8 色是现成资产，未入技能 | 图标是控件「细」的一半，换项目即失传 |
| 3 | **对比度/可访问性数值** | 项目有实测经验值（卡片压暗白字 6.7~7.5:1、提亮仅 3.5:1），refactoring-ui 的 WCAG 规则只面向 web | GDI 深浅底配色无检查清单，易重蹈「字看不清」 |
| 4 | **动效/过渡参考** | 无。ZSUI 有 progress 不确定态动画参数、toast 淡入淡出 | 弹层/加载态硬切，观感掉档 |

**补法建议**（按性价比排序）：①③成本低收益大——type scale 半页表 + 对比度检查 5 行清单，并入 widget-drawing §1/§2 即可；②把 icons.rs 粉彩规则提炼成一节；④可后置。

---

## 2. 问题二：是否同时适用 C++ 与 Rust —— **不对称，配方通、示例偏科**

| 层 | C++ | Rust | 判定 |
|---|---|---|---|
| GDI 绘制配方（度量/配色/状态/AA） | ✅ API 级通用 | ✅ API 级通用 | **通**——GDI 是同一套 API，画法卡本身双语可用 |
| 画法卡的「GDI 落地映射」栏 | ❌ 只写 `draw.rs::button` 等 Rust 符号 | ✅ | **偏科**——C++ 读者需自行换算 |
| 窗口壳层 | ✅ glass-borderless（SetWindowRgn 路线，glass_showcase.cpp） | ❌ 无——WinSuiteX 的 **WS_EX_LAYERED + UpdateLayeredWindow + SDF 羽化圆角** 只活在项目 memory | **断裂**——两代技术各占一边 |
| 弹窗/模态 | ✅ custom-notice（消息循环/owner 禁用） | ❌ 无 | 单边 |
| 资源嵌入（图标/版本/版权） | ⚠️ 经验在，无独立卡（MinGW windres / rc.exe） | ✅ djjz-rust-msvc-gui-res | 单边（好在 rc 五坑两语言通用） |
| 渲染验证 | ✅ | ✅ | 唯一双语齐全的技能 |
| 框架技能（egui / fltk / imgui） | 各自单边 | 各自单边 | 合理（框架本身单语言） |

**结论**：画法配方双语通用（这正是「只学画法」策略的红利），但**落地示例与窗口壳层是 C++/Rust 各半本**——C++ 项目看得到壳层看不到新画法示例，Rust 项目反过来。建议（不动手，待定）：
- widget-drawing 每张落地栏补一行 C++ 对照（同一 GDI 调用的 C++ 写法），或统一声明「示例符号以 WinSuiteX/Rust 为准，API 双语同名」；
- glass-borderless 增补「Rust 分层窗口路线」一节（WS_EX_LAYERED + ULW_ALPHA + SDF，从 WinSuiteX 抽提），消除断裂。

---

## 3. 问题三：横评同类 UI 技能

### 3.1 GDI 自绘族（我们的主线，三件套 + 一散件）
- **widget-drawing × glass-borderless**：分工干净（控件内 vs 窗口壳），已互相引用，无重叠。✔
- **win32-gdi-custom-notice**：与 widget-drawing §3.14（ContentDialog 画法卡）**绘制部分重复**，且自身 WM_PAINT 只有注释桩（「绘制圆角白底+图标+…」无代码）——它是「模态模式卡」不是「画法卡」。独有内容仅约 40 行：模态消息循环、owner 自动禁用、WM_DESTROY 勿 PostQuitMessage、WS_POPUP 勿 WS_CHILD。
- **nogpu**：验证方法论，与画法族正交，双语齐全，无替代。✔

### 3.2 框架族（egui-rust / fltk-1-4-5 / imgui-ui-ux-engineering）
三者是三种不同技术路线的工具箱手册，彼此不重叠，但与「GDI 自绘零依赖」主线 **路线相悖或无交集**：
- egui-rust：WinSuiteX 已明确弃用 egui（5.66MB+wgpu，VM/RDP 起不来，main.rs 注释在案）；
- fltk-1-4-5：工具箱（~80 widget 类），与自绘路线是替代关系；
- imgui-ui-ux-engineering：只在 ImGui 证据下触发，质量不错（含 UX 状态/设计系统章节），但当前项目无 ImGui。
共同点：**都是「有项目用它时才回本」的沉没型技能**，91~226 行占位不碍事，但不该抢触发。

### 3.3 Web 产出族（huashu-design / web-design-engineer / refactoring-ui）
- huashu-design（527 行）vs web-design-engineer（494 行）：**职能重叠约七成**——都做 HTML 高保真/幻灯片/动画/变体探索/评审。差异仅在气质：huashu=「100% 先出三个方向让用户选」+反 AI slop+事实验证优先；web-design-engineer=「bar is stunning」+Design Critique+Tweaks 面板。对 Z.W. 的实际用途（皮肤预览页 theme-preview.html 这类选型展示），**huashu 的三方向决策流更贴你的「先方案后确认」习惯**。
- refactoring-ui：明确自我定位为「规则/诊断层，不生成美学」，与上面两个生成器**互补不重叠**，是设计质量的下限保障。✔ 但它的中文触发词（配色/对比度/间距/排版/设计评审/设计系统）**会误抢桌面 GDI 任务**（见 §4）。

---

## 4. 问题四：去留建议

> 删除属破坏性操作——以下仅为建议清单，**未经你确认不动手**。

### 🔻 退役候选（2）
| 技能 | 理由 | 处置方案 |
|---|---|---|
| **web-design-engineer** | 与 huashu-design 重叠 ~70%，同装必争触发、增加每次选型成本；huashu 的三方向流程更贴你的决策习惯 | 直接停用。若想要英文工程化视角，留 huashu 单件即可 |
| **win32-gdi-custom-notice** | 画法与 widget-drawing §3.14 重复且自身不完整（绘制是注释桩）；独有 40 行「模态模式」是真价值 | **先合并后退役**：把模态循环/owner 禁用/WM_DESTROY 三条并入 widget-drawing 对话框卡附录，再停用原技能 |

### ~~⚠️ 观察名单（3）~~ → ✅ 已于 2026-10-23 17:15 全部停用
| 技能 | 处置 |
|---|---|
| fltk-1-4-5 | ✅ 已移入 `_del_bak/`（与 GDI 自绘铁律相悖；nogpu 里的 FLTK 四坑已自带，不受影响） |
| imgui-ui-ux-engineering | ✅ 已移入 `_del_bak/`（UX 状态矩阵已提炼进 widget-drawing 状态表规则） |
| egui-rust | ✅ 已移入 `_del_bak/`（WinSuiteX 已弃 egui 在案；rust-desktop-applications 选型技能保留，仍覆盖 egui/iced/slint 选型知识） |

### ✅ 必留（8）
djjz-gdi-widget-drawing、djjz-gdi-glass-borderless、djjz-gui-render-nogpu、djjz-rust-msvc-gui-res、huashu-design、refactoring-ui、rust-desktop-applications、windows-shell-feature-dev（非 UI 族，注册表/Shell 域，勿动）。

### 🧹 触发边界卫生（横评时发现，建议顺手修）
1. **refactoring-ui 中文触发词越界**：「配色/对比度/间距/排版/设计评审/设计系统」会命中桌面 GDI 请求 → 描述补一句「仅限 web/Tailwind；桌面原生 GDI/控件 → djjz-gdi-widget-drawing」。
2. **huashu-design 触发词「设计风格/评审」同理** → 补「仅限 HTML 产出」。
3. **widget-drawing 描述含「统一界面度量/配色」** 与 refactoring-ui 撞词 → 已含「Win32 自绘控件」前置语义，风险较低，保持观察。

---

## 5. 行动清单（✅ 已执行 2026-10-23）

| 优先 | 动作 | 状态 |
|---|---|---|
| P0 | 触发边界修 2 处描述（refactoring-ui / huashu-design） | ✅ 已改，桌面 GDI 任务不再被 web 技能抢触发 |
| P1 | widget-drawing 补 Type Scale 表 + 对比度检查清单 + 图标粉彩一节 | ✅ 新增 §1.1/§1.2/§1.3 |
| P1 | custom-notice 模态模式并入 widget-drawing §3.14 附录 | ✅ 四条铁律（owner 禁用/勿 PostQuitMessage/WS_POPUP/按键收口） |
| P2 | widget-drawing 补 C++ 对照表；glass-borderless 补 Rust 分层窗口路线 §2.9 | ✅ 双语断裂已修复 |
| P2 | 停用 web-design-engineer；custom-notice 合并后停用 | ✅ 均移入 `skills/_del_bak/`（未删除，可找回） |
| P3 | fltk / imgui / egui 三件按项目计划定去留 | ✅ 2026-10-23 17:15 Z.W. 确认停用，三件移入 `skills/_del_bak/`（未删除可找回） |
