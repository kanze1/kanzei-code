# kanzei 品牌与界面质感

日期：2026-09-26。状态：已接入当前源码；安装包验收以对应发布的验证证据为准。

- 身份: live_design

本文替代旧 K 图标、绿红状态配色与消息背景水印的设计约定。历史设计保留在 `app_icon.md`、`ui_color_semantics.md`、`ui_chat_backdrop.md`，冲突时以本文和当前源码为准。

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
- 项目切换器与欢迎页共用 SVG。Windows ICO、macOS ICNS、各平台 PNG 和 PWA 图标均由同一 SVG 导出，不单独手工改色。

生成命令：

```powershell
node scripts/generate-brand-assets.mjs
node scripts/generate-brand-assets.mjs --platform
```

第一条生成 SVG；第二条通过 Tauri 图标工具同时生成平台资源。更换资源不会替换已经运行的安装版，需后续正常构建安装包。

## 提示色与质感

三色约束用于品牌和提示。代码高亮、记忆图谱的分类色保持各自语义。

| 语义 | 暗色主题 | 亮色主题 |
|---|---|---|
| 运行 / 主要操作 `--accent` | `#ff8700` | `#c56500` |
| 强调文字 `--accent-text` | `#ffad66` | `#934100` |
| 完成 `--ok` | `#39d8f6` | `#006579` |
| 注意 `--warn` | `#ffc18b` | `#874500` |
| 失败 `--err` | `#ffad66` | `#963f00` |
| 结构提示 `--info` | `#8d9ac7` | `#191c2b` |

失败与注意属于橙色系，必须同时显示状态文字或字形，不能只靠色相区分。浅色主题加深可交互色和文字以维持对比度，品牌图标本身始终使用原始三色。

中性底色仍是原来的深灰 / 白色。侧栏、活动栏、项目头和输入框通过 `--chrome-texture` 叠加极低不透明度的暖灰渐变与微弱冷灰过渡，形成参考图中的柔和质感。没有噪点图、动画纹理、额外图片请求或模糊滤镜；正文背景不叠加渐变。系统原生标题栏不属于这次 CSS 修改。

## 对话与欢迎页

- **消息对话没有背景图案**：所有图案预设都返回 `hidden`，清空 Canvas、隐藏 SVG、停止背景动画帧。
- 欢迎页与语音舞台保留可关闭的装饰，默认三模块 SVG；真实星座 / 用户点集继续使用 Canvas。
- 欢迎页文案仍有避让和对比度保护；消息对话不再使用宽屏沟槽、窄屏徽记或大幅水印。
- 欢迎页与消息页切换时立即更新装饰显隐。运行事件只消费现有状态，不制造运行进度。

## 验证入口

- `node scripts/ui-constellation-smoke.mjs`：构图、几何与对比度计算。
- `node scripts/ui-constellation-browser-smoke.mjs`：真实浏览器内的显隐、动画帧与像素审计。
- `node scripts/ui-a11y-smoke.mjs`：主题 token、文字与状态对比度。
- `node --experimental-vm-modules scripts/ui-runtime-smoke.mjs`：前端模块运行回归。
- `node scripts/ui-preview/shoot.mjs --out output/ui-brand-v3 --scenes chat,empty --themes dark,light --width 1600 --height 900 --dpr 1`：使用模拟 IPC 的实际前端截图，不等同于桌面安装版验收。
