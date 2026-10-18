# 原生窗口与搜索交付验收

本次交付范围为归档后详情残留、Windows 原生窗口操作、网页搜索论据与引用质量、Luna 订阅搜索兼容，以及最终发布。完整门禁和发布二进制复验记录在修复进度文档；此处区分开发版本证据和发布状态。

## 归档详情

原问题在独立 WebView2 实例复现：R-001 手动完成后已经写入 requirements-archive.md，详情仍停留在旧状态。管理页原先使用活动快照查找条目，找不到便回退到打开时的对象。

现在活动详情属于当前文档快照；条目移出活动集合时，选择和旧编辑节点一起退场，返回该类型的列表。显式打开归档历史仍为只读。归档缓存随文档版本失效，重新展开会取最新内容。非终态流转更新状态和按钮，普通刷新保留有效草稿。

`ui-management-archive-native-smoke.mjs` 的 23 项真实软件检查通过，包括需求完成/放弃、缺陷修复/不修复、取消、待外部验收、后台归档编辑中的条目及已加载归档更新。另有文档资源事件 10 项回归通过。

## Windows 原生窗口

入口为当前对话更多菜单中的“选择原生窗口”。用户选择具体窗口实例后，desktop 工具才能观察、截图、填写、点击、切换控件、按单个命名键和选择一个明确项目文件。窗口身份包括 HWND、进程和创建时间；动作引用观察产生的快照和控件，修改后旧快照立即作废。文件对话框使用范围校验后的保留副本，敏感文件仍需精确授权。

只操作选定窗口及同一进程拥有的对话框；密码控件不接受自动填写，工具不给出密码值，也不控制 Kanzei/Codex/ChatGPT 审批窗口。当前实现需要 Windows 与 PowerShell 7。这是窗口能力约束，不能据此声称目标程序运行在 OS 沙箱中。

开发证据：系统 UIA/键盘/鼠标/文件选择 19 项检查、原生软件合同流程 17 项检查、旧 UIA 密码边界 2 项检查通过；合同流程包含等待批准期间断开窗口后拒绝旧动作。真实 gpt-6-luna 在独立 Kanzei 实例中自主发现工具、填写姓名、点击提交；目标程序自己的回调确认收到 LUNA_NATIVE_FORM。该实测覆盖一张测试表单，不能代表任意桌面软件任务的成功率。真实 TCP EOF 单测证明调用方断开会取消仍在等待的原生动作 future。

## 搜索后端与 Luna

Responses API 的模型回合与 Codex 订阅的直接搜索命令具有不同契约。旧适配器向订阅 Responses 端点发送 max_tool_calls 得到 HTTP 400；该失败证据保留在 `search-live-capability-responses-v1.json`。

本次探针确认：当前 Luna 订阅的原生搜索可返回 web_search_call，且同通道重放成功；直接 alpha/search 接口返回结构化搜索结果。适配器据认证通道选择协议，不按模型名称列白名单。订阅的直接搜索每个查询只发送一个 search_query 命令，不带 Responses 的 max_tool_calls；API 的 Responses 搜索保留其预算和完整 sources 契约。两者分别有权限资源路径、响应验证和尝试记录。

`search_backend=codex` 现在明确选择订阅直接搜索；responses 明确选择 API 搜索；auto 按 primary 路由决定，只有 auto 可以记录原因后切换 DuckDuckGo。显式 DuckDuckGo 不向订阅端点发送查询。alpha/search 是非公开接口，任何结构变化都产生类型化失败，不把模型文字或未知结构当作搜索结果。

生产适配器的真实四对象查询均使用 codex_search，一次尝试、没有回退，分别返回 Codex 5、Claude Code 1、CodeBuddy 2、Kimi Code 1 条结果。另一次协议探针得到合法空结果，该观察保留，不计为内容覆盖成功。搜索系列定向回归 32 项通过，新增响应结构、过滤、单命令和不跨服务测试均包含在内。

## 论据与引用质量

固定数据集范围为四个产品的安全文档发现与引用，不是漏洞/CVE 完整性。第一版 Codex 旧 security 链接已转到代码安全扫描产品，不能证明 agent 的审批/沙箱策略。第一版数据和观察保留，第二版改为实际 agent 文档，同时把“已知 URL 命中比例”与完整相关性精度区分；未标注的其他结果不自动视为不相关。

第二版通过真实 Kanzei 的 DDG 路径取得四个对象的候选结果与四份 HTTP 200 正文。保存的正文与采集 SHA-256 一致后冻结；真实 gpt-6-luna 分别回答“完整四份来源”和“移除 CodeBuddy 来源”两组问题，每组一次，没有重试或排除。自动核对来源身份、属性归属、逐字摘录与缺失披露；人工复核其解释与上下文一致。

| 对象 | 正文支持的属性 | 第一方来源 |
| --- | --- | --- |
| Codex | 技术沙箱与请求批准的策略属于不同控制层 | [Agent approvals and security](https://learn.chatgpt.com/docs/agent-approvals-security) |
| Claude Code | 文件系统与网络均有隔离边界 | [Sandboxing engineering article](https://www.anthropic.com/engineering/claude-code-sandboxing) |
| CodeBuddy | 默认只读，执行/修改请求明确许可 | [Security](https://www.codebuddy.ai/docs/cli/security) |
| Kimi Code | 权限模式对敏感行为的处理不同 | [Interaction and input](https://www.kimi.com/en/help/kimi-code/cli-interaction) |

完整组四个属性与四个引用均有对应正文；缺失组的三个引用正确，CodeBuddy 明确为证据不足，未编造 URL 或引文。共七个已回答属性的引用核对通过，一个缺失对象正确披露。判分器另有 12 项自检，覆盖缺对象、品牌 URL 冒充、错配引用、伪造引文和重复对象。

证据文件：`output/remediation-20261018/search-answer-v1/acceptance.json`、`model-results.json`、`frozen-input.json`；实时检索分别见 `output/playwright/search-quality/native/live-results.json` 和 `output/remediation-20261018/luna-native-four-object-search.json`。上述结果不评估近期新闻时效、所有问题的相关性或长程自主漏洞调查质量。
