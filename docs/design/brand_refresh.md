# kanzei 品牌与界面质感

日期：2026-10-20。状态：依据已发布配色基线整合用户确认的原型 03。

- 身份: live_design
- 配色核对基线：[build-8fb9f71d](https://github.com/kanze1/kanzei-code/releases/tag/build-8fb9f71d)。
- 源码提交：`8fb9f71d0b3c0a6f0f42c7c1c24ae26a158721de`。
- 用户确认：当前发行版的底色合适，维持现有底色、正文色与明暗层次；后续配色调整只讨论提示色的使用位置和用量。

本文记录保持不变的发行版颜色，以及原型 03 确认后的提示色用法。颜色核对依据为发布标签中的 `app.css` 加载顺序、`style.css` 基础变量，以及 `familiar-workspace.css`、`skills.css`、`surface.css` 等组件样式的最终覆盖；本轮复用相同变量，以字号、间距、轮廓和橙青点缀优化界面。

历史设计保留在 `app_icon.md`、`ui_color_semantics.md`、`ui_chat_backdrop.md`。发生冲突时，以本页绑定的发行版及其实际渲染为准。

## 当前页面底色与文字

页面使用现有的深灰、白色和中性灰文字。下表为组件使用的不透明底色；侧栏的轻微纹理、悬停和选中叠色另按对应 CSS 合成。

| 部位 / 用途 | 暗色主题 | 亮色主题 | 实际来源 |
|---|---|---|---|
| 主区、对话区、项目页头 | `#181818` | `#ffffff` | `--bg` / `--chat-bg` |
| 侧栏、活动栏、任务活动面板 | `#1f1f1f` | `#f7f7f7` | `--sidebar-bg` / `--activitybar` |
| 输入区外框 `#composer` | `#232323` | `#ffffff` | `familiar-workspace.css` 覆盖为 `--panel` |
| 菜单、弹窗等浮层 | `#2c2c2c` | `#ffffff` | `--surface-overlay`，经 `--surface-bg` 使用 |
| 围栏代码块、工具源码输出 | `#1c1c1c` | `#f7f7f7` | `--code-output-bg` |
| 用户消息气泡 | `#2b2b2b` | `#f2f2f2` | `--user-bubble` |
| 普通状态栏 | `#181818` | `#f7f7f7` | `--statusbar` |
| 标题、强调文字、输入文字 | `#ffffff` | `#0d0d0d` | `--fg-strong` |
| 正文 | `#e5e5e5` | `#2b2b2b` | `--fg` |
| 辅助文字 | `#9e9e9e` | `#656565` | `--dim` |
| 常规边框 | `#343434` | `#dcdcdc` | `--border` |
| 轻分隔线 | `#282828` | `#ebebeb` | `--border-soft` |

基础变量 `--surface-raised` 仍为暗色 `#303030`、亮色 `#ffffff`，但当前对话输入区已由后加载的组件样式改用 `--panel`，不能将该基础变量直接记为输入区最终底色。文本框 `#prompt` 本身透明，显示外框底色。

围栏代码块 `.md-code-block` 和工具输出 `pre.code` 使用 `--code-output-bg`，暗色为 `#1c1c1c`；基础变量 `--code-bg` 的暗色 `#202020` 不能作为这些组件的最终底色记录。

`--chrome-texture` 仍作用于侧栏和活动栏，叠加极轻的暖灰与冷灰过渡。当前项目页头直接使用 `--chat-bg`；输入区的 `background: var(--panel)` 同时清除了基础样式中的纹理。浏览器核对时，这两个组件的 `background-image` 均为 `none`。正文背景不叠加纹理。

## 矢量图标

采用用户选定的「协作模块」方向：三个相同模块旋转排列，围出共享空间，表达多个 agent 并行工作。主体不再包含字母 K。

| 用途 | 色值 |
|---|---|
| 品牌底板 | 亮橙 `#ff8700` |
| 三个模块 | 墨蓝 `#191c2b` |
| 信号窗口 | 电光青 `#39d8f6` |

- 几何真源：`crates/kanzei-app/ui/00-brand.js`。
- 完整图标：`crates/kanzei-app/ui/assets/kanzei.svg`。
- 透明底符号：`crates/kanzei-app/ui/assets/kanzei-symbol.svg`。
- SVG 只包含矢量路径与矩形，不嵌入位图、字体或外部资源。
- 欢迎页使用同一份 SVG；侧栏的 `kanzei` 字标使用页面文字色。Windows ICO、macOS ICNS、各平台 PNG 和 PWA 图标均由同一 SVG 导出，不单独手工改色。

生成命令：

```powershell
node scripts/generate-brand-assets.mjs
node scripts/generate-brand-assets.mjs --platform
```

第一条生成 SVG；第二条通过 Tauri 图标工具同时生成平台资源。更换资源不会替换已经运行的安装版，需后续正常构建安装包。

## 当前提示色与按钮

品牌图标的三种原色与页面底色、正文色分别记录。页面提示色按深浅色适配；代码高亮、记忆图谱和代码差异保留各自的颜色语义。

目标树的节点类型也使用独立分类色：目标蓝、需求浅蓝、实现青绿、缺陷珊瑚、调研紫、设计靛、文档沙金、验证草绿。用于类型文字与图标，`--work-*` 在深浅主题分别定义；扩展类型沿用所属基础类型的颜色。卡片背景复用 `--panel`，画布复用 `--bg`，选择与焦点使用中性色；不引入原型的暖灰背景、绿色主强调或大面积类型色填充。完成使用 `--ok`，执行使用 `--accent-text`。普通主要操作沿用 `--primary-bg` / `--primary-fg`。详见 `recursive_goal_development.md` §9。

| 语义 | 暗色主题 | 亮色主题 |
|---|---|---|
| 运行信号、发送按钮填充 `--accent` | `#ff8700` | `#c56500` |
| 强调文字 `--accent-text` | `#ffad66` | `#934100` |
| 完成 `--ok` | `#39d8f6` | `#006579` |
| 注意 `--warn` | `#ffc18b` | `#874500` |
| 失败 `--err` | `#ffad66` | `#963f00` |
| JSON 键辅助色 `--info` | `#8d9ac7` | `#191c2b` |

`--info` 在该发行版的直接 CSS 使用点为 `.sv-json .sv-key`。标题和正文的实际颜色使用上一节的中性灰值。

失败与注意属于橙色系，必须同时显示状态文字或字形，不能只靠色相区分。浅色主题的交互填充和文字已加深；品牌图标本身使用原始三色，不把图标的亮橙直接替换为所有浅色按钮的填充。

| 按钮用途 | 暗色主题 | 亮色主题 |
|---|---|---|
| 普通主按钮 | `#f0f0f0` 底、`#181818` 字 | `#0d0d0d` 底、`#ffffff` 字 |
| 发送按钮 | `#ff8700` 底、`#191c2b` 箭头 | `#c56500` 底、`#191c2b` 箭头 |
| 中性焦点环 | `#9a9a9a` | `#7a7a7a` |

普通主按钮使用 `--primary-bg` / `--primary-fg`，发送按钮单独使用 `--accent` / `--on-accent`。不能将“主要操作”概括成所有主按钮都为橙色。

## 组件中的使用位置

- 对话 / 管理顶层页签：当前项保持原有文字色，以橙色短线标记。当前对话与设置分组也用小型橙色短线呼应。
- 管理页：需求 / 缺陷编号、当前页内页签和已完成批次使用 `--ok`；进行中批次使用橙色；普通计数及被替代批次使用中性灰。
- 运行统计：已有观测的常规读数使用 `--ok`，非零失败数使用错误色，预期拒绝保持中性，缺失观测显示 `—`。
- 需求验收：验收编号和当前版本已有证据的反馈使用 `--ok`；旧版本证据与待验证保持中性。
- 资源与反馈：附件、交付文件、输入资源链接的类型图标使用 `--ok`；保存 / 复制成功复用既有 `kind: ok` 反馈，不将整行或每次工具调用染色。
- Skills：已启用开关使用适配深浅色的 `--ok`，用青色点缀可用状态。
- 工作选择：当前工作的图标与编号使用 `--accent-text`，长标题保持正文色。
- 活动入口：图标与数量在同一行自然排布，运行数使用 `--accent-soft` 底与 `--accent-text` 字；数量隐藏时不占位，不沿用侧栏角标的绝对定位。
- 页签悬停：完整点击面积保留，悬停底色限制在文字附近的 28px 高圆角区域，避免灰底铺满页签高度。

提示色集中在短线、小图标、编号、进度和开关；正文与大面积底色保持基线。初始化说明只在空白对话欢迎区显示，登记失败和待开始需求仍保留可操作反馈。实现并入现有组件样式，不加载原型覆盖层。

## 代码差异的独立配色

`familiar-workspace.css` 在基础样式之后覆盖差异色，当前发行版保留红删绿增。不能用 `style.css` 中较早的 `--diff-add: var(--ok)` / `--diff-del: var(--err)` 推断最终差异色。

| 用途 | 暗色主题 | 亮色主题 |
|---|---|---|
| 新增文字 `--diff-add` | `#76c99a` | `#237442` |
| 删除文字 `--diff-del` | `#ee8585` | `#ba3535` |
| 新增底色 `--diff-add-bg` | `#32834f22` | `#32834f13` |
| 删除底色 `--diff-del-bg` | `#cb434322` | `#cb434313` |

## 对话与欢迎页

- **消息对话没有背景图案**：所有图案预设都返回 `hidden`，清空 Canvas、隐藏 SVG、停止背景动画帧。
- 欢迎页与语音舞台保留可关闭的装饰，默认三模块 SVG；真实星座 / 用户点集继续使用 Canvas。
- 欢迎页文案仍有避让和对比度保护；消息对话不再使用宽屏沟槽、窄屏徽记或大幅水印。
- 欢迎页与消息页切换时立即更新装饰显隐。运行事件只消费现有状态，不制造运行进度。

## 验证入口

2026-10-20 核对：GitHub 最新正式 Release 为 `build-8fb9f71d`。浏览器预览直接读取该标签中的 124 个 HTML / CSS / JavaScript / SVG 等前端文件，数据使用隔离 IPC，分别取得深浅色组件的计算样式；运行时错误为 0。

本次证据为 `output/brand-release-baseline-20261020/colors.json` 与 `release-dark.png`、`release-light.png`。该记录用于核对发行版前端配色，不等同于重新安装或启动桌面安装包。

后续回归入口：

- `node scripts/ui-cyan-interactions-smoke.mjs`：提示色及管理页、设置、编辑器和附件相关交互。
- `node scripts/ui-constellation-smoke.mjs`：构图、几何与对比度计算。
- `node scripts/ui-constellation-browser-smoke.mjs`：真实浏览器内的显隐、动画帧与像素审计。
- `node scripts/ui-a11y-smoke.mjs`：主题 token、文字与状态对比度。
- `node --experimental-vm-modules scripts/ui-runtime-smoke.mjs`：前端模块运行回归。
- `node scripts/ui-preview/shoot.mjs --out output/ui-brand-v3 --scenes chat,empty --themes dark,light --width 1600 --height 900 --dpr 1`：使用模拟 IPC 的实际前端截图，不等同于桌面安装版验收。
