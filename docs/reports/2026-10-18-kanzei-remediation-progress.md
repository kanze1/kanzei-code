# Kanzei 修复 验收与发布进度

目标：完成待处理清单的全部 27 项、软件实际操作验收和正式发版。起点为 8d479de1；开发分支 kanzei/reliability-browser-memory。源码、定向回归、软件操作、完整发布门禁分别记录，不能互相替代。

## 逐项状态

| 项目 | 当前状态 | 下一项完成证据 |
| --- | --- | --- |
| RUN-01 | 待实施 | 相同调查任务的正确性及上下文消耗对照 |
| RUN-02 | 待实施 | 双会话实际并行调查与无冲突写入 |
| RUN-03 | 已修成功终态同步及队列投影，待完整验收 | 批准等待、重启恢复及真实 UI 状态一致 |
| RUN-04 | 待实施 | 引擎事实进入报告，日期及构建信息正确 |
| SEC-01 | 待实施 | 多个内容访问出口的敏感资源正反测试 |
| SEC-02 | 待实施 | 真实浏览器不能读取未授权 sibling 或外传 |
| SEC-03 | 定向反例先失败后通过，待完整验收 | 子代理、并行及串行路径的整体回归 |
| SEC-04 | 待实施 | OS 文件、网络与进程树隔离实测 |
| SEC-05 | 待实施 | 宽授权下敏感启动配置的专项策略 |
| SEC-06 | 待实施 | 外部内容标记及对抗网页操作验收 |
| SEC-07 | 待实施 | 只读子代理错误快照负向用例 |
| WEB-01 | 14 项搜索回归通过，含无跨服务 HTTP 断言 | 软件内搜索与最终完整门禁 |
| WEB-02 | 待实施 | 请求、授权、引用及历史回放验收 |
| WEB-03 | 待实施 | finding 对应任务、来源版本及工件 |
| WEB-04 | 待实施 | 真零结果与异常页面的区别 |
| WEB-05 | 待实施 | 动态页面正文取证及未核验状态 |
| WEB-06 | 待实施 | 固定检索集、覆盖及引用质量结果 |
| WEB-07 | 待实施 | 地区、排除域和预算的具体场景 |
| BRO-01 | 待实施 | 内部面板与 Edge 完整填写提交 |
| BRO-02 | 待实施 | 用户授权的现有 Edge 标签页接管 |
| BRO-03 | 待实施 | 授权、页面身份和状态连续性验收 |
| BRO-04 | 待实施 | 原生窗口、对话框和画布操作 |
| MEM-01 | 待实施 | 类型化使用与结果反馈 |
| MEM-02 | 待实施 | 活跃记忆降级、清退和恢复 |
| MEM-03 | 待实施 | 会话做梦按钮及可追溯产物 |
| MEM-04 | 待实施 | 作用域、去重、冲突与撤销 |
| MEM-05 | 待实施 | 统一作业进度、停止及恢复 |

## 已取得的原问题与回归证据

- 成功收尾反例：durable_outcome_commits_before_success_publication 原实现断言失败，LiveRun.last_outcome 为 None，预期 completed。修复后 durable_outcome 系列 10 项通过。
- 步数反例：真实 runner 的 mock provider 在最后一步返回工具调用，原实现生成 forbidden-side-effect 文件。修复后 exhausted_budget_rejects_provider_tool_calls_before_side_effects 通过，调用在子代理派发和普通工具执行之前被拒绝。
- UI 状态：ui-state-admission-smoke 16 项通过；parallel-lines-regression 和已改 UI 文件 ESLint 通过。这些是 UI 冒烟，尚不能证明安装版桌面操作验收。
- 协作状态：collaboration::tests 5 项通过，含真实协调器排队快照、completed/incomplete/queued 投影。
- 搜索路由：websearch::tests 14 项通过，使用本机 HTTP 服务确认显式 DDG 不连接 Codex、显式 Codex 失败不连接 DDG、auto 只按 primary 判定。

## 发布验收

尚未发布。发布前需全部逐项证据成立，最终提交通过 verify.ps1 -Full，验证清单绑定最终 HEAD，无失败或跳过；然后执行正式打包发布，独立核对安装包大小、SHA-256、Range 和软件版本。软件操作测试须包含真实 Kanzei 窗口、内部浏览器、Edge、做梦与记忆管理；本机安装与重启后的结果另行记录。
