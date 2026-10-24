# 网页预览与浏览器操作合同

- 身份：live_design
- 核对日期：2026-10-23，当前工作树源码；验证结果按版本和场景另记。
- 范围：内置 WebView2 面板、独立浏览器、已有浏览器连接及其会话归属。布局遵循 [熟悉的工作区](familiar_workspace.md)。

## 执行面与页面身份

一个 browser 工具提供相同 schema 与输出合同。CLI 使用独立浏览器或显式连接；桌面入口增加面板执行能力。当前入口为 [browser_tool](../../crates/kanzei-tools/src/browser_tool.rs)、[DesktopBrowserTool](../../crates/kanzei-app/src/harness_ext.rs) 与 [preview](../../crates/kanzei-app/src/preview/mod.rs)。

| 执行面 | 选择与身份 | 生命周期 |
| --- | --- | --- |
| pane | 桌面显式 backend=pane；页面归属绑定对话/进程 | 面板隐藏、遮挡或切换对话不转移页面身份；截图需要可见的原生表面 |
| headless | backend=headless；主会话和子代理以 browser_owner 隔离 | open 后继续操作同一 owner 的页面；不能驱动其他会话或父会话面板 |
| attached | connect(endpoint) 连接用户授权的本地 CDP Edge/Chrome；pages 列出目标 | 操作必须绑定 connection_id、page_id、expected_url；iframe 还绑定 frame_id 和 expected_frame_url；断开后显式重新连接 |

未指定 backend 时沿用对话已选浏览器。旧“面板不可见就自动换无头”的路径已废弃；结果首行标记实际 backend。排队动作在执行时重新核对归属、页面与版本，迟到动作不能因页面重新绑定而进入另一个目标。连接需要兼容的 Node.js 运行时，具体要求见工具说明；普通已打开窗口不等于已经提供可连接的 CDP 端点。

## 动作与结果

- 基础动作：open、dom、console、screenshot、click、type、press、scroll、wait、eval。
- 表单动作：select、check、upload、validate；正常控件优先使用这些语义操作。动作回执与业务提交成功分别核对。
- 连接动作：connect、pages、frames、disconnect；目标从实际枚举结果选择，不能跨页面沿用旧身份。
- source 保存当前渲染正文和 DOM 证据；topic 与 task_id 绑定研究归属。抓取原文、摘要、模型结论分别保留，来源完整性不代替引用语义核对。
- 截图通过统一工具图片路径进入模型、对话与历史；图片外置与生命周期以 [tool_images](../../crates/kanzei-core/src/runner/tool_images.rs) 和工件管理实现为准。

支持动作、参数与限制由 browser_tool 的 schema/description 和 [browser-helper.mjs](../../scripts/browser-helper.mjs) 维护，本文不复制整份字段清单。

## 权限与内容边界

审批资源与执行动作绑定同一浏览器目标。已有浏览器连接、面板和子代理页面各自维护所有权；窗口可见性不代替授权。连接由本地端点发现，实际页面必须再次核对。外部页面内容作为资料处理，不提升为用户授权。

面板使用 WebView2 进程内 CDP；外部浏览器连接是独立的显式能力。导航闸阻止页面进入应用特权来源。打开本地文件只授予该页面及允许的静态前端资源；需要页面读取目录数据时显式打开相应目录，仍遵守敏感资源规则。内联片段与静态预览服务的边界见 [preview_server](../../crates/kanzei-tools/src/preview_server.rs)。

## 面板与前端

网页是独立浮动窗口，支持拖动、调整尺寸和缩放。隐藏保留页面，关闭释放页面；切换对话隐藏旧页面并保留归属。后台对话不会因普通动作抢占前台，用户关闭后的自动弹出遵循前端抑制状态。

IPC 参数和事件以 [command_registry.json](../../crates/kanzei-app/src/command_registry.json)、生成的 [ipc-commands.json](../../scripts/ipc-commands.json) 与 [前后端协议](../architecture/06_frontend_protocol.md) 为准。旧停靠栏位置、手抄参数表和按遮挡时长切换后端规则不作为当前合同。

## 验证与历史

验收按执行面分别覆盖表单报错修正与最终回显、隐藏/切换后的页面连续性、跨会话与子代理隔离、iframe/导航、断开与取消，以及渲染正文的来源回读。模拟 IPC、独立浏览器、真实 WebView2、真实模型和安装版分开记录。当前活动任务的剩余验收以 tracker 为准。

早期试验、旧布局和历次观察完整保留在 [整理前历史快照](../reports/2026-10-23-preview-pane-history.md)。原生软件控制是独立执行能力，见 [原生控制记录](../reports/2026-10-19-native-control-experience.md)；浏览器截图不单独证明原生软件操作通过。
