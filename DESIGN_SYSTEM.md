# WinSuiteX UI 设计系统 v4.0（Ant Design 语言 · 结构移植版）

> 平台：Windows 桌面（Win32 GDI 自绘）｜ 目标：色彩语言统一到 Ant Design + 结构级移植
> v4.0 变更（相对 v3.5）：① 主色由 `#1677FF` 改为 **`#0958D9`**（Ant blue-7，守 WCAG AA）；② 圆角由 12px 收至 **8px**（按钮 6px）；③ 卡片新增 **hover 三层阴影 elevation 浮起**；④ 间距收进 **8px 栅格**；⑤ 新增语义色 token（备用）。
> 语言来源：Ant Design 5 的 SeedToken / MapToken / AliasToken（即 AntdUI 的底层设计语言）。

---

## 🎨 Design Foundations

### 色彩系统（Color Tokens）

> 全部取自 Ant Design 5 默认 token。**GDI 无 alpha 通道**，凡 Ant 用 `rgba(0,0,0,a)` 的
> 文本/阴影，一律按 `c = 255 × (1 − a)` 与白底**织白**成实色。换算表见文末。

| 本工程 Token | Hex | Ant Design 对应 | 用途 |
|---|---|---|---|
| `--bg` | `#F5F5F5` | `colorBgLayout` | 窗口背景（中性灰，去蓝调） |
| `--surface` | `#FFFFFF` | `colorBgContainer` | 卡片 / 控件表面 |
| `--border` | `#D9D9D9` | `colorBorder` | 卡片边框 |
| `--divider` | `#F0F0F0` | `colorBorderSecondary` | 分隔线 |
| `--text-1` | `#1F1F1F` | `colorText`（rgba .88 织白） | 主标题 |
| `--text-2` | `#262626` | Ant gray-9 | 功能名 |
| `--text-3` | `#595959` | `colorTextSecondary`（rgba .65 织白） | 副标题、卡片描述 |
| `--primary` | `#0958D9` | Ant blue-7（=官方 `colorPrimaryActive`） | 主操作按钮、品牌强调（**守 WCAG AA**；我们作常驻主色） |
| `--primary-hover` | `#1677FF` | Ant blue-6（=官方 `colorPrimary` 种子色） | 主按钮悬停（比常驻主色浅一阶，符合 Ant 悬停**变亮**方向） |
| `--primary-active` | `#003EB3` | Ant blue-8 | 激活/按下更深一阶（非官方 alias，按 Ant 方向自定） |
| `--success` | `#52C41A` | `colorSuccess` | 语义绿（token 备用） |
| `--warning` | `#FAAD14` | `colorWarning` | 语义琥珀（token 备用） |
| `--error` | `#FF4D4F` | `colorError` | 语义红（token 备用） |
| `--green` | `#16A34A` | —（保留，AA 友好） | RunCommand 类型标识 / 已启用 |
| `--green-bg` | `#DFF2E6` | —（保留） | 已启用徽章底色 |
| `--amber` | `#CA6E04` | —（保留，AA 友好） | ShellExtension 类型标识 |
| `--amber-bg` | `#FEF4E1` | —（保留） | 未启用暖色徽章 |

**⚠️ 未照搬 Ant 的两处（刻意保留，守工程约束）**
1. **语义绿/琥珀不采用 Ant 的 `#52C41A` / `#FAAD14` 作为图标瓦片**：Ant 语义色为"标签/徽章"场景设计，作 44px 图标瓦片底过艳，且黄绿在白底做小字不达 AA。仅在 token 层定义备用（`--success`/`--warning`/`--error`），UI 主体仍用较深的 `--green`/`--amber`，更耐看、更省眼。
2. **Ant 的三级文字 `rgba(0,0,0,.45)`（织白 `#8C8C8C`）未用于正文小字**：该色在白底对比度仅 ≈3.3:1，**不达 WCAG AA 4.5:1**。本工程卡片描述等小字统一用 `--text-3 #595959`（≈7:1）。

**对比度校验（WCAG AA）**

| 组合 | 对比度 | 判定 |
|---|---|---|
| `--text-1` `#1F1F1F` / `--surface` `#FFFFFF` | ≈ 16.5:1 | ✅ 远超 4.5:1 |
| `--text-3` `#595959` / `--surface` `#FFFFFF` | ≈ 7.0:1 | ✅ 达标 |
| 白字 / `--primary` `#0958D9` | ≈ 6.16:1 | ✅ 达标（v4.0 选 B，守 AA） |
| 白字 / `--primary-hover` `#1677FF` | ≈ 4.10:1 | ⚠️ 低于 4.5:1（仅用于 hover 瞬时态，与 Ant 官方一致） |
| 白字 / `--primary-active` `#003EB3` | ≈ 8.98:1 | ✅ 达标（最强） |
| （对比）白字 / `#2563EB`（旧主色） | ≈ 5.2:1 | ✅ |

> ✅ **主色 AA 取舍已定（v4.0 选 B）**：上轮 v3.5 取 A（`#1677FF`，4.1:1 不达 AA）；本轮改为主色
> `#0958D9`（Ant blue-7，6.16:1），仍在 Ant 家族内、且守住 AA。hover/active 遵循 Ant 三态
> （常态 `#0958D9` → hover 变亮 `#1677FF` → active 更深 `#003EB3`）。

### 字体系统（Typography）

| 角色 | 字号 | 字重 | 行高 | 说明 |
|---|---|---|---|---|
| 窗口标题 | 24px | 700 | 32px | "Windows 个性化" |
| 功能名 | 16px | 600 | 22px | 卡片主标题 |
| 段标题 | 14px | 600 | 20px | 硬件页分段标题（v4.0 由 13px 放大） |
| 说明文字 | 12px | 400 | 18px | 卡片副描述 |
| 状态徽章 | 11px | 500 | 16px | 药丸内文字 |
| 按钮文字 | 13px | 500 | — | 启用/停用 |
| OS 副标题 | 13px | 400 | 18px | Windows 11 (Build xxxx) |
| 页脚提示 | 11px | 400 | 16px | 使用说明 |

**字体栈**：`"Microsoft YaHei", "Segoe UI", system-ui, sans-serif`（中文优先雅黑，英文回退 Segoe）

### 间距系统（Spacing · 8px 基准栅格）

```
8 / 16 / 24 / 32 px   （v4.0 由 4/8/12/16/24/32 收进 8 的倍数）
```
约定：
- 卡片内边距 `16px 18px`
- 卡片之间间距 `16px`（v4.0 由 14px 调整）
- 窗口边距 `24px`（v4.0 由 28px 调整）
- 硬件段间距 `16px`（v4.0 由 12px 调整）
- 图标与文字间距 `16px`
- 文字行组间距：标题→说明 `2px`、说明→徽章 `8px`

### 阴影与层级（Elevation · Ant Design 语言）

Ant Design 的阴影是**中性、极浅、多层**（不是 Tailwind 系的"单层可见灰"）。其 AliasToken：

```
--shadow (boxShadow):      0 6px 16px 0 rgba(0,0,0,0.08), 0 3px 6px -4px rgba(0,0,0,0.12), 0 9px 28px 8px rgba(0,0,0,0.05)
--shadow-tertiary:         0 1px 2px 0 rgba(0,0,0,0.05), 0 1px 6px -1px rgba(0,0,0,0.03), 0 2px 4px 0 rgba(0,0,0,0.03)
```

**GDI 无高斯模糊**，用多层实色圆角矩形向下偏移近似。v4.0 引入**两级状态**：

**常态（rest）**——极淡单层阴影 + 1px 边框，卡片"轻贴"背景（Ant 卡片常态靠边框立起，阴影仅作极轻提示）：

| GDI 常量 | 实色 | 偏移 |
|---|---|---|
| `C_SHADOW` | `#F0F0F0` | `(0, +2)` |

**悬停（hover）**——三层阴影模拟 elevation 上浮（远层偏移大且浅 → 近层偏移小且深）+ 主色变亮边框：

| GDI 常量 | 实色 | 偏移 |
|---|---|---|
| `C_SH_HOVER_3` | `#E8E8E8` | `(+2, +8)` |
| `C_SH_HOVER_2` | `#DCDCDC` | `(+1, +4)` |
| `C_SH_HOVER_1` | `#D0D0D0` | `(0, +1)` |

卡片默认（rest）叠 1 层 `C_SHADOW` + 1px `--border`；hover 时叠三层 + 2px `--primary-hover` 边框。左缘 3px 彩色标识条区分功能类型（绿=RunCommand / 琥珀=ShellExtension）。

> ⚠️ 旧版（≤v3.4）近层用 `#BCC3D0`（偏冷偏重），卡片下缘有一道明显深灰"台阶"；v3.5 换成中性浅灰后该台阶消失；v4.0 进一步将阴影降为"靠边框立起"，并把视觉重量转移到 hover 浮起，使交互更有"Ant 感"。
> 注：窗口四周阴影（`C_WIN_SHADOW_*`）在 v4.0 仍交由系统原生 CS_DROPSHADOW 绘制，窗口内无自绘外阴影。

---

## 🧱 Component Library

### 1. 窗口框架（Window）
- 宽度：560px（紧凑不浪费）
- 背景：`--bg`
- 标题栏：系统原生（WS_CAPTION），不做自绘标题栏以保系统习惯

### 2. 页头（Header）
- 24px 粗体标题 + 32px×3px 品牌蓝装饰短线 + 13px 灰色 OS 副标题
- 下方 1px 分隔线

### 3. 功能卡片（Feature Card）
```
┌─(3px 彩条)──────────────────────────────────────┐
│  ┌────┐                                          │
│  │图标│  功能名称                     [启用/停用] │
│  │44px│  说明文字...                              │
│  └────┘  ● 已启用 / ○ 未启用                     │
└──────────────────────────────────────────────────┘
```
- 白色圆角 **8px**（v4.0 由 12px 收敛），1px 边框，极淡单层阴影
- **hover：三层阴影浮起 + 主色变亮边框**（v4.0 新增，Ant 标志性交互）
- 左侧 3px 类型彩条（圆角右端）
- 图标：44px 圆角方块（绿/琥珀）+ 白色滑块纹
- 文字区：标题→说明→状态徽章，垂直堆叠不重叠
- 按钮：右侧垂直居中

### 4. 状态徽章（Status Badge）
- 药丸形（border-radius 11px）
- 已启用：绿底 `#DFF2E6` + 绿字 `#15803D`
- 未启用：灰底 `#F0F0F0` + 灰字 `#595959`
- 固定宽 70px、高 22px

### 5. 按钮（Button）
| 类型 | 样式 | 悬停 |
|---|---|---|
| 主操作（启用） | 蓝底白字，圆角 **6px**（v4.0 由 8px 收敛） | 亮蓝 `#1677FF`（Ant 悬停**变亮**） |
| 次操作（停用） | 白底深字 + `#D9D9D9` 边 | 浅灰底 `#FAFAFA` |
- 尺寸：90×32px，足够点击区域（>44px 高度要求仅触屏必需，桌面 32px OK）

### 6. 页脚（Footer）
- 11px 浅灰提示文字，说明右键菜单位置与管理员权限需求

---

## 📐 布局规格（Layout Spec）

| 区域 | 高度 | 说明 |
|---|---|---|
| 页头 | 120px | 标题(24) + 装饰线 + OS副标题 |
| 卡片 × N | 104px 每个 | 含内边距与内容 |
| 卡片间距 | **16px**（v4.0 由 14px 调整） | — |
| 窗口边距 | **24px**（v4.0 由 28px 调整） | — |
| 页脚 | 56px | 提示文字 |

**卡片内部垂直坐标（CARD_H=104，top=0）：**
```
y+12 ~ y+36   功能名（24px）
y+38 ~ y+58   说明（20px）   ← 与标题间隔 2px
y+66 ~ y+88   状态徽章（22px）← 与说明间隔 8px
底部留白 16px
```
**零重叠验证**：36 < 38，58 < 66 ✅

---

## ♿ 无障碍（Accessibility）

- 文字对比度：正文小字全部 ≥ 4.5:1（`--text-1` 16.5:1、`--text-3` 7.0:1）；主色按钮白字 6.16:1 ✅（v4.0 已守 AA）
- 按钮/徽章均有清晰边界与状态色区分
- 悬停态有明确 elevation + 色彩反馈（v4.0 强化）
- 键盘：ESC 关闭窗口（既有逻辑保留）
- 点击区域 ≥ 32px（桌面标准）

---

## 🔁 附：rgba → GDI 实色换算表

GDI `COLORREF` **没有 alpha 通道**。Ant Design 里凡带透明度的颜色（文字、阴影），
必须先在**目标底色**上合成实色：

```
c = 底 × (1 − a) + 前景 × a
```

在白底（255）上合成 `rgba(0,0,0,a)`：

| alpha | 算式 | 实色 | 对应 Ant token |
|---|---|---|---|
| 0.88 | 255 × 0.12 | `#1F1F1F` | `colorText` |
| 0.85 | 255 × 0.15 | `#262626` | gray-9 |
| 0.65 | 255 × 0.35 | `#595959` | `colorTextSecondary` |
| 0.45 | 255 × 0.55 | `#8C8C8C` | `colorTextTertiary` ⚠️ 小字不达 AA |
| 0.12 | 255 × 0.88 | `#E0E0E0` | 阴影近层 |
| 0.08 | 255 × 0.92 | `#EBEBEB` | 阴影中层 |
| 0.05 | 255 × 0.95 | `#F2F2F2` | 阴影远层 |
| 0.03 | 255 × 0.97 | `#F7F7F7` | 阴影最外层 |

---

## 📌 落地状态

| 文件 | 状态 |
|---|---|
| `main.cpp`（Token v4.0） | ✅ 已落地并编译通过（产物 `WinSuiteX_v4.exe`，716288 字节） |
| `preview.py`（配色/几何常量） | ✅ 已同步（`preview.png` 为改后效果；首卡演示 hover 浮起） |
| `DESIGN_SYSTEM.md` | ✅ 本文档已同步至 v4.0 |
| 主色 AA 取舍 | ✅ 已定：选 B `#0958D9`（守 AA，仍在 Ant 家族） |
