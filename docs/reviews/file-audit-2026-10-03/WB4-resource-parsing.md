# WB4 资源身份、结构化解析与 Markdown

日期：2026-10-03。基线 `26acb7f0`，保留 WB1–WB3 修复。本包全文审查 4 文件，修改 3 文件；确认 3 个 P1、3 个 P2。状态文案文件 PASS。报告内 WB4 编号仅用于定位，不冒充已有 issue。

## 依赖地图

```text
Rust attachments / tool outcome / runner preview / handoff / Git worktree diff：原始内容与协议
  → 04-resource-types：附件 MIME、资源类型与链接身份
      → 08-compose-runtime.addFiles → FileReader → shell.attachments → 附件展示/发送
  → 04-status-words：仅翻译显示词
      → 02-i18n → 状态 UI；原枚举仍用于 IPC、CSS 与业务判断
  → 04-structured-parse：纯文本事实与内部交接过滤
      → 04-structured / 05-tool-summary：工具详情与摘要
      → 05-chat-render / 15-views-misc / 07-events / 25-softwire-model：正文、历史、复制与概览
      → 20-lines：逐文件 diff 和增删计数
  → 04-markdown → 04-diagram：唯一 Markdown DOM 入口与闭合图块
      → 对话、历史、查看器、结构化结果
      → 19-research 的共享点击委托 → structuredNav.openPath
```

资源类型、状态词和结构化解析没有持久化写入；原文本仍是内容真源。Markdown 只持有装饰钩子及渲染函数，图渲染的异步队列由 diagram 持有。本包没有增加状态副本或更改锁、数据库、IPC、文件布局；FileReader 异步回调只用于验证实际附件链，未变更其 owner。共享导航只检查本包调用合同，不扩展科研专项。

修改前已检索全部公开函数 caller。协议对照包括 harness 的 `tool.rs` outcome 头、core 的 `runner/event.rs` preview 和 `runner/tool_exec.rs` 存储标记、harness 的 `handoff.rs` 内部字段、tools 的 `worktree.rs::worktree_status` Git diff。遵循源内 UI-0926 #6/#10、UX-008/UX-037 的已有职责；图块边界依据 [图渲染设计](../../design/architecture_diagrams.md)，表格可达样例见 [预览面板设计](../../design/preview_pane.md) 中的 `back \| forward`。

## crates/kanzei-app/ui/04-resource-types.js

### 职责
统一附件 MIME、文件/网页类型图标与资源链接识别。

### 判断
P1

### 确切问题
- **WB4-01 / P1**：扩展名解析将所有输入的 `#` 都当 URL fragment。合法本地文件 `budget#2026.csv`、`plan#final.txt` 被真实附件入口拒绝；本地及 URL 编码文件名还显示错误图标。

### 修改
- 仅 HTTP(S) URL 从 pathname 取文件名；本地名称完整保留 `#`。继续复用同一扩展名解析，不在附件 caller 打补丁。

### 影响范围
- MIME 的唯一调用方是 `08-compose-runtime.js::addFiles`（不是测试桥 `08-compose.js`）；图标/资源链接、交付物、Markdown 共享资源分类。
- 不改变 MIME 白名单、发送结构或后端附件解析；仍拒绝不支持的类型。

### 验证
- 生产 ESM、实际 Edge、原生 FileReader 和附件 DOM：CSV/TXT 的名称、MIME、字节、正文及展示均保留；普通附件与拒绝 exe 控制通过。
- URL query/fragment、编码 `#`、Windows 大写扩展名和行号、MIME fallback、Git host 和带括号 URL 控制通过。
- 资源/状态共 16 项通过；旧模块精确替换对照为 12 PASS / 4 FAIL。

## crates/kanzei-app/ui/04-status-words.js

### 职责
提供通用及执行单元语境的状态词与阶段显示翻译。

### 判断
PASS

### 确切问题
- 未发现有实际影响的问题。

### 修改
- 无。

### 影响范围
- 唯一直接 UI caller 为 `02-i18n.js` 的显示包装；不参与状态转换或 IPC 提交。零 import，避免 i18n 反向依赖。

### 验证
- 全文与 caller 审查；执行单元 active 与默认 active 的中英文分别验证，未知值保留、缺省值为空、阶段翻译控制通过。计入上述 16 项，不重复计数。

## crates/kanzei-app/ui/04-structured-parse.js

### 职责
从协议文本提取路径、计数、错误和 diff 事实，并统一过滤内部交接信息。

### 判断
P1

### 确切问题
- **WB4-02 / P1**：行内代码中的交接字段触发截断，代码及后续说明丢失；短围栏或带语言的围栏行提前结束较长代码块，随后示例字段被删。流式正文、历史、复制、概览都实际使用该过滤器。
- **WB4-03 / P2**：Git 默认引用中文路径时，`diff --git` 文件头不被识别，两个文件被并到一个文件，名称仍是八进制转义，增删计数归属错误。影响差异展示，不改变磁盘内容。

### 修改
- 检测行内交接字段前屏蔽完整行内代码，保留原文位置；围栏按字符、长度及闭合行内容判断。
- 识别 Git 引用路径文件头，按 Git C 转义和 UTF-8 八进制序列解码路径；逐文件建立统计边界。无额外依赖。

### 影响范围
- `05-chat-render`、`15-views-misc`、`07-events`、`25-softwire-model` 和 `20-lines`；公共函数签名及返回形状不变。
- 不修改后端 handoff/outcome 协议，也不把显示结果用于提交运行状态。

### 验证
- 19 项通过；精确旧 ESM 为 10 PASS / 9 FAIL。
- 真实 `git diff --no-index` 两个中文文件验证文件数、可读文件名和各自 1 增/1 删；另测引用转义不重复解码、普通含空格路径、删除文件。
- 代码示例完整保留，真实内部块/行内交接仍隐藏；普通人类模板保留。Rust outcome/storage/preview、错误、路径及 token 原文拼接控制通过。

## crates/kanzei-app/ui/04-markdown.js

### 职责
将安全 Markdown 子集写入 DOM，并统一装饰资源链接、图块与预览钩子。

### 判断
P1

### 确切问题
- **WB4-04 / P1**：链接目标在首个右括号处截断，`C:/project/(draft)/main.rs:12` 点击交付错误路径；外链和裸 URL 也丢失配对括号。
- **WB4-05 / P2**：表格直接按竖线拆分，转义竖线被当分隔符，实际 API 表格错列且后列可能被截掉。
- **WB4-06 / P2**：代码块内带语言的围栏会提前闭合，四反引号示例不能容纳三反引号；流式代码的闭合状态与内容错误。

### 修改
- 链接目标按括号深度读取，裸 URL 仅去掉多余右括号及末尾标点；保留现有尖括号包裹空格路径方式。
- 表格扫描时区分转义竖线和真正列分隔符。
- 代码围栏保存字符和长度，只由同字符、足够长度且不带语言的闭合行结束。

### 影响范围
- 全部 `renderMarkdownInto` caller，以及共享路径点击委托、diagram 和预览钩子；API、安全 URL/路径过滤与 HTML 转义保持原合同。
- 没有换 Markdown 引擎、增加框架或持久化格式。

### 验证
- 实际 Edge 14 项通过；旧 Markdown 精确替换为 5 PASS / 9 FAIL。
- 实际委托点击收到完整路径与行号；表格内容、嵌套围栏、流式 `data-open`、尖括号空格路径与行内码标签控制通过。
- 危险 URL 与 HTML 保持惰性；旧 `ui-markdown-smoke.mjs` 的 Markdown/结构化解析回归通过。

## 整合验证

- 新增回归 **49 项**（资源/状态 16、结构化 19、Markdown 14），均接入 `scripts/ui-runtime-smoke.mjs`。重复运行不重复计数。
- `npm run lint`、三个新增脚本 ESLint、既有 Markdown smoke 通过。
- 完整 UI runtime：exit 0；90 个生产 UI 模块、3606 次初始化 invoke、10 个主视图、0 运行时错误，既有浏览器套件和深浅主题 Harness 操作通过。
- `ui-lint-smoke.mjs`：exit 0；182 文件静态检查、1646 导出 ESM 守卫和附带弹层/布局/背景浏览器检查通过。
- `git diff --check`、514 文件唯一性、四块范围/队列一致性、WB1–WB4 的 19 个源码指纹和 Markdown 引用检查通过；既有轮次源码未被本轮改动。
- 证据集中在 `output/audit-WB4/`：三个功能目录的 before/current acceptance 和日志、`runtime.log`；Git 夹具位于 ignored output，不改仓库状态。旧对照只替换被审模块，不 checkout 旧树。

当前未提交、未发版。本包完成后新增全文累计 19 文件；全仓历史全文 155/514，四块已完成/可复用 80/268、待处理 188；工作台 32/51。下一包 WB5 沿状态词/图引擎 → 结构化渲染 → 工具摘要：`02-i18n.js`、`04-diagram.js`、`04-structured.js`、`05-tool-summary.js`。

# Module Summary

## 已修复
- P0：0。
- P1：3（WB4-01、02、04）。
- P2：3（WB4-03、05、06）。

## PASS 文件
- 04-status-words.js。

## 仍需人工判断
- 无。

## 依赖影响
- 修复共享内容解析；所有 caller 沿用同一 API，不增加各处补丁，不改变后端状态或持久化合同。

## 剩余风险
- 本轮没有 Rust 修改；未重跑 Rust 全工作区或原生 WebView2/真实后端端到端验证。当前证据为生产 ESM、真实 Edge、原生 FileReader、实际 Git 及隔离 IPC。
- C7 原有三个 P1 仍排在第二块，本轮界面通过不表示这些后端问题已修。
