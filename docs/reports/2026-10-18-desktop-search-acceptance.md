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

## 补充评价与原文证据修复

继续按 WEB-06 核对时，发现上述固定集还没有独立评价所有返回结果的相关性和时间过滤，不能将其概括为完整的搜索质量验收。补充评价对两条后端的全部 27 条返回记录、23 个不同 URL 逐项标注：2 为直接说明目标 coding agent 的权限/沙箱，1 为相关指针，0 为其他产品或通用公司资料。单人事后标注，只报告这四个固定查询的实际返回集，不声称全库召回率或一般质量分数。

| 后端 | 返回数 | 相关（含指针） | 直接论据 | 有直接论据的对象 |
| --- | --- | --- | --- | --- |
| Codex 订阅直接搜索 | 9 | 7/9 | 4/9 | 3/4 |
| DuckDuckGo | 18 | 15/18 | 14/18 | 4/4 |

同一官方域名也可能是另一产品：Agents API 环境安全、Codex Security 与 Claude Code Security 的扫描产品页不能自动证明 coding agent 的执行权限；仓库 SECURITY.md、security/policy 等重复指针也不能算多份独立直接论据。标注与可重复判分在 `output/remediation-20261018/search-relevance-annotations.json`、`grade-search-relevance-recency.mjs` 和 `search-relevance-recency-assessment.json`。

时间过滤对照固定于 2026-10-18T12:27:36Z，使用第一方 GitHub release 元数据区分最近一周的 v2.1.302 与区间外的 v2.1.296。每后端各执行 recent-week、old-week、old-unfiltered 一次。两后端带 week 的结果没有发现精确的官方版本 release 页面，而是聚合页、镜像和 issue，不能从这些页面计算已核实的发布日期精度。DDG 无过滤旧版本查询遇到 CAPTCHA，作为失败保留，未重试；参数被正确传递不等于时效保证。

另用最终 14dace59 二进制进行两道冻结自主任务。第一轮主模型为真实 Luna，辅助 web_extract 却为 fixture，因此只保留为受限观察。修正配置后 primary/fast 均使用真实 gpt-6-luna，每题新做一次，保留第一轮结果：安全调查 8 步，三个对象有语义依据，Codex 的两份第一方页面返回 403，明确披露缺失；三个引文只有 Kimi 的引文是对应网页的连续原文。Claude 引文改了枚举连接词，CodeBuddy 引用了辅助摘要对表格的解释。日期任务 5 步，正确核对 v2.1.302、2026-10-16T20:53:06Z；精确时间来自已获取的第一方 API，release 页面仅显示到分钟。两题都有一次把批量字段 q 写成 query 的输入错误，随后自行纠正；批量中的 locale 在显式订阅后端还得到 unsupported_filter，未静默改服务。运行完成、部分覆盖、正确事实和精确引文是不同结果。

这暴露了工具证据通道的实际缺口：`webfetch(prompt=...)` 以前把辅助模型生成的文字直接作为网页提取结果。现在辅助模型仅返回结构化行号范围，程序核对范围后从已采集、带 SHA-256 的正文中返回原文。模型不能提供要输出的引文或摘要；无效 JSON、越界或模型失败明确返回 WEBFETCH_PASSAGE_SELECTION_FAILED，原文仍可通过缓存的 find/from_line 读取。空选择只说明所给预览未选到相关段落，不代表完整页面没有证据。模型选择与渲染分到 webfetch/passages.rs，沿用原有 web_extract 配置和采集所有者。另修复搜索 schema 将 locale 约束写到了未被引用的 $defs，而实际 draft-07 引用 definitions 的问题。

旧二进制的自主观察保存在 `search-autonomous/` 与 `search-autonomous-full-model/`，结构核对在各目录 assessment.json，前一轮 CodeBuddy 引文同样未通过原文核对。它们不作为新源码和后续发布二进制的通过证据。新的完整门禁、发布资产和实测仍须绑定实际提交。

## 普通联网读取的研究范围纠错

d2b19ca7 最终二进制的四对象权限边界题得到 4/4 第一方正文及对应短引文；日期题却在未知字段被拒绝后，误把 `topic="web research"` 当成搜索分类。普通 websearch 暴露的 topic/task_id 原先没有字段说明，只有一个字段时的错误码又是 RESEARCH_LOOP_TASK_REQUIRED。Luna 因此误解为所有搜索都必须先有任务编号，转而提问；没有最终日期答案，也没有发出供应商查询。这一失败完整保留在 search-autonomous-d2b19ca7/，不计为服务故障或通过。

共用 SourceScope 现在为 websearch/webfetch 提供一致的成对字段 schema 与解释：这是已登记 research_loop 活动任务的可选来源归属，普通读取省略两者，不是搜索分类。只传一者返回 INVALID_WEB_SOURCE_SCOPE，并说明如何纠正原请求及尚未联网；伪造或失效的成对身份仍被拒绝。research 档位仍强制提供 begin_search 发出的有效 topic/task_id，专用包装保留字段说明与既有校验。回归同时核对普通读取、单字段误用、伪造身份和 research 必填边界；不新增自动重试或静默降级。

## 完整门禁中的修复

第一轮全量验证发现项目文档链接的 Enter 路由被活动独立工作树的文件预览抢占。现在项目文档沿项目归属进入文档查看器；对话附件和显式文件打开继续沿各自的所有者解析。文档异步读取沿用查看器的导航代次校验，读取失败明确展示。文件链接回归 34 项通过。

删除最后一个会话的回归仍断言旧“无监管模式”的开关选项；它已按现有三种下一轮执行模式核对显示文字与实际值，原来的首次绘制、删除、陈旧轮询和空收件箱检查全部保留并通过。

归档缓存失效的订阅原先强制重绘管理页，会在后台刷新时替换搜索框。归档区域现由列表自己的填充函数更新，搜索输入节点和焦点保持不变；项目管理回归 57 项通过。工具 schema 账单沿用实际桌面延迟工具清单，包含新增 desktop，并明确核对它不驻留在首轮 schema 中。

记忆存储的体量门禁发现单文件增长超限。创建准入事务及带来源的复核候选写入整体移入 store/creation.rs，公开接口、锁和准入规则保持一致；原门禁阈值未调整，独立复验通过。第一轮失败记录保留在 dist/verification-runs；最终完整门禁的提交、树、逐项结果和日志哈希以 dist/verification.json 为准。发布二进制复验与资产校验单独保存为 output/remediation-20261018/release-acceptance.json，不能用开发二进制结果替代。
