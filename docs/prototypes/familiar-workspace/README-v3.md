# Kanzei 熟悉交互原型 · 第三版

历史记录。最新试用入口、行为与验证方式见 [第四版说明](README.md)。

2026-10-04。[本轮设计说明](revision-3.md) · [当前功能归属与遗漏](../../reviews/2026-10-04-ui-information-architecture/feature-reachability-v3.md)。第二轮文档保留历史，当前导航以本版为准。

保留熟悉的侧栏、对话和输入操作。调用结果在对话右侧；需求、缺陷、交付和项目地图同级切换；低频运行记录与规范归设置。记忆统一从左下管理，VS Code 从项目右键打开，差异保留红绿。

## 打开与试用

[打开当前原型](http://127.0.0.1:14105/)。沿用原地址和浏览器存储，保留之前的草稿、设置与反馈。需要手动启动时：

```powershell
node docs/prototypes/familiar-workspace/serve.mjs --port 14105
```

重点试这五条：

1. 左下问号 → **演示工具调用**。子代理、终端、网页预览依次在右侧展开。试着切换标签、发送子代理补充要求、停止单个命令、在网页里添加书签。
2. 调用过程中手动选择一个标签，或关闭面板。后续输出应保留你的选择；切到另一个对话，当前草稿与页面不能被后台活动改掉。切回后查看原对话记录。
3. 管理 → **需求 → R-381**。完整阅读正文和稳定 AC 编号；展开来源、证据、执行记录。编辑含义后旧证据待复核。新建时只填原始描述，应保存为草稿；补齐后明确转为待开始。
4. 管理 → **项目地图**。点击模块，检查依赖、被依赖、清单来源和文件索引；搜索 `requirement` 等路径。需求/缺陷/交付与格子进度仍按第二轮习惯保留。
5. 右键左侧项目 → **在 VS Code 打开 / 项目设置**。设置里能找到运行记录、开发规范、测试记录；左下记忆管理仍统一查看全局和所有项目。

试用说明中的“验收与反馈”可保存意见并导出。重置示例保留反馈。Ctrl+P 可搜索页面；Shift+F10 可打开已聚焦项目的右键菜单。

## 这一版的范围

原型运行、终端、子代理、需求、交付和记忆使用本地示例数据；网页预览是可交互的内嵌示例。没有连接正式 agent、进程服务或真实 tracker。

两处使用本机真实对象：项目地图从当前仓库生成；项目右键里的 VS Code 会实际打开 `kanzei code` 目录。阅读器等虚拟项目没有真实目录。附件仅保存名称，语音仍为输入演示。

需求呈现参考本轮已发布结构，不创建另一套正式存储。旧格式的边界、迁移回滚和未知字段保留可读。想法与研究已退出原型，正式应用的代码退役尚未实施。

会话高级菜单、待回答/决定复核、工作条目依赖、子代理工作树采纳、记忆图谱与真实连接配置，仍未完成整套原型流程；见功能归属报告，不能以“有入口”代替“已可用”。

## 更新项目索引

```powershell
python docs/prototypes/familiar-workspace/generate-project-index.py
```

当前索引：8 个工作区模块、23 条直接依赖、531 个 Rust/前端文件。主图展示 22 条非纯开发依赖，来源保留所有依赖类型。依据根与成员 Cargo 清单，记录 HEAD 和清单指纹；属于静态依赖，不表示函数或运行时调用。清单改变后重新生成即可。

## 验证

```powershell
node docs/prototypes/familiar-workspace/verify.mjs
node docs/prototypes/familiar-workspace/verify-v3.mjs
```

使用项目现有 playwright-core 和本机 Edge，独立浏览器上下文、临时端口，不改动当前试用窗口的草稿与反馈。自动验证记录：[已有流程](../../../output/playwright/familiar-workspace/verification.json)、[本轮侧栏与需求结构](../../../output/playwright/familiar-workspace/verification-v3.json)。

本轮已有流程 85 项、新增流程 45 项检查通过，页面运行错误为 0。另在用户可见的深色预览中核对第三版导航、真实索引与调用面板。

检查包括：草稿/队列隔离、完整详情、记忆同编号隔离、右键编辑器、反馈与存储迁移；调用展开、手动选择、关闭抑制、跨对话后台事件、预览页面内操作、工具停止与刷新暂停；新需求草稿/AC 身份/来源/证据版本；静态索引路径与窄屏布局。原型自动检查不代表正式应用接入验收。

截图：[实际试用窗口](../../../output/playwright/familiar-workspace/v3-live-preview.jpg)、[对话右侧预览](../../../output/playwright/familiar-workspace/activity-preview.png)、[子代理](../../../output/playwright/familiar-workspace/activity-agent.png)、[终端](../../../output/playwright/familiar-workspace/activity-terminal.png)、[完整需求](../../../output/playwright/familiar-workspace/requirement-full.png)。
