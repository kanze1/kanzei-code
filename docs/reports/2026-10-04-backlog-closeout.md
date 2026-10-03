# 2026-10-04 需求与缺陷清单收口

本轮只对清单与验收证据收口，不扩大实现所有历史需求。以主目录当前源码和真实测试为准，通过 tracker API 更新和关闭，未直接修改受管 Markdown，未把外部验收写成完成。

## 核对范围与规则

- 起始活跃清单：41 条需求、24 条缺陷，共 65 条；R-366 已归档，保持原记录。
- 只关闭下表已经逐条对照原验收的 11 项；其余保持原队列，不能因为旧进展写“已实现”就批量关闭。
- 所有原始验收文本保留。旧进展不删除；R-367 恢复 HEAD 中原实现进展，再保留 BATCH_CLOSING 历史检查点与本轮复测结果。
- 原工具逐条验收锚和前端证据新鲜度门禁实际执行。R-367 首次因证据锚未就近关联被拒，补逐条锚后通过；R-382 首次因旧记录器没带源码指纹被拒，改用公开 TestRecordTool 对已执行浏览器测试记录真实范围及当前冻结源码指纹后重试。没有放宽产品门禁。
- 临时 evidence driver 的 rustc 初次重链遇到多版本 harness，随后只对记录器绑定与 tools 同批构建的 harness，产品源码未因此改动。

## 已充分核对的关闭条目

| 条目 | 原验收对照与当前证据 |
|---|---|
| R-367 | ①未读已有文件拒绝且字节不变；②部分读取；③外改拒绝与实际上下文；④新建/连续编辑账本刷新；⑥无账本旧行为由 `read_receipt_tests` 直接覆盖。⑤`core/runner/subagent.rs:514` 子代理独立账本、`app/conversation.rs:74` 新对话清空，桌面/CLI 注入合同已读。T-1786922727132，定向 3/3。 |
| R-382 | ①类型/优先级/标签筛选；②状态分组与数量；③刷新保留筛选/展开、详情入口和顺序且不改调度。真实浏览器 24 项检查，含侧栏滚动与输入框隔离。 |
| R-383 | ①外验保留证据与未验证事项并可退回开发；②外验不占开发/取活/阻塞且开发依赖放行；③侧栏/完整列表保留详情。maintenance 6/6 与浏览器 24 项。 |
| D-748 | ①commands 不再扫描或注入、②skills 仍注入、③旧 commands API 移除、④core/skills key；历史 harness 171/171。⑤`harness_m1.md:11`、架构索引 README:55 及 registry/harness 注释现在均为五类，旧索引缺口已由 ae1370ff 修复。 |
| D-755 | 原条目没有独立验收字段，以原复现和影响核对：既有问题不再阻止合法更新，新增问题/空索引/漏条目仍被拒。architecture 11/11，包括原复现与路径别名回归；未补造历史验收。 |
| D-760 | ①小结果不取锁/扫描；②计量排除 shadow；③头8KiB尾4KiB并说明省略；④锁/计量失败降级保留工具结果；⑤display 与 UI 配额提示；⑥同hash复用先于配额。原 core 424/424 对应直接回归及本轮 runtime 配额夹具。 |
| D-761 | ①循环依赖进入 blocked；②R-910/R-911 互依夹具断言两条 blocked。本轮 runtime 通过。不把未要求的 docs_snapshot 结构重构当成原验收缺口。 |
| D-763 | ①规范明确 commit/verify/CI 三种 clippy 口径；②测试修改需 all-targets；③`git.rs` gate_checklists 守护测试通过。旧 tools 日志中该测试通过但整份有其他失败，因此不声称旧 tools 全量通过。 |
| D-775 | ①显式维护读双队列；②普通执行仍走 work next；③拒绝纠错码及维护入口/schema 提示。maintenance 6/6 中直接覆盖。 |
| D-776 | ①非空 reason 且保留原条目；②取消与完成不同证据门禁；③close/update 同口径；④CLI/桌面传原因且重复关闭不重写。maintenance 6/6、CLI 公共参数、桌面实际 IPC 浏览器断言。 |
| D-777 | ①BATCH_CLOSING 独立 WorkflowBlocked/暂缓；②缺路径 NeedsCorrection 保留 code；③只有真正 Failed 计失败率且导航诊断保留；④live/history 一致。当前 telemetry 5/5、浏览器 24 项，serial_tools 传递路径静态核对；前轮 core 424/app 624 含 outcome 接线。 |

逐条 T-/file:line 锚已附进展，完整请求/返回保存于 `output/backlog-audit-2026-10-04/closeout-*.json`。关闭完成情况以文末最终盘点为准。

## 保持待外部验收的 5 项

| 条目 | 未完成的现场验收 |
|---|---|
| R-370 | 真实 SSH 服务器注册/触发/结果拉取及一次回写。 |
| D-504 | 安装版退出/重启后的持久化自动运行状态与 UI 一致。 |
| D-577 | 外部“文章获取器”项目 R002 的 raw_lines/raw_delete 现场复验。 |
| D-592 | 真实 llama-local/server 小上下文长工具循环中，400 前触发压缩。 |
| D-746 | 安装版长历史指标页响应。 |

这些条目保留原验收、既有本地证据和外部事项，不归档，也不作为开发阻塞。

## 确认仍需开发或补证的条目

- **R-344 恢复 doing**：原⑥要求新环境缺准备步骤时询问并记录。当前 `research_environment.rs:204` 只有 preparation_steps 校验，runner 缺环境返回 INVALID_ENVIRONMENT；R-345 完成环境登记不等于实现询问流程。④持久事实恢复已有 R-347 实现，不再把旧承接文字当当前缺功能；真实 SSH/断线仍保留外验说明。恢复开发是纠正错误分类，不是等待产品决策。
- **D-762 保留**：正常前像捕获和锁测试不能证明原③“捕获失败留下哨兵、后续不补采前像”；`write.rs` 捕获前后均失败仍警告后续可能捕获中间态，不满足原验收。
- **D-768 保留**：`tracker/actions.rs` 顶层 id 缺失仍是通用 error，未实现原明确纠错码等完整要求。
- **D-769 保留**：CI 仍手动触发，verify 是轻量 clippy；注释准确是 D-763，但不代表 D-769 要求的执行范围已完成。
- **R-369 保留**：原 task 后台/隔离/恢复/merge_task 与当前 task/adopt 的合同差异未完整对账，不擅自当作等价产品验收。
- **D-759 保留**：已有移动审批脚本，但本次不为扩大关闭范围再补专项核验；不能仅凭脚本存在声明原全部场景已验收。
- 其他旧条目本轮没有足够逐条验收证据，保持当前队列。这不等于认定每一项都缺功能，也不等于全审计失败。R-372/R-373 的真实实验与质量判据不以普通单测替代。

## 证据与边界

| 证据 | 实际结果与范围 |
|---|---|
| `output/joint-release-2026-10-04/read-receipts-final.log` | 3/3；首次 `read-receipts.log` 功能断言通过、末尾清理 OS32 失败，原样复跑通过，未降低断言。 |
| `output/joint-release-2026-10-04/maintenance.log` | 6/6；包括 reopen/claim/release 状态镜像、取消与交付边界。 |
| `output/joint-release-2026-10-04/architecture.log` | 11/11；含历史问题容忍及等价路径归一化。 |
| `output/joint-release-2026-10-04/telemetry.log` | 5/5；直接构造真实 Rust outcomes 验证失败率分类。 |
| `output/backlog-merge-ui/backlog-final.log` / `output/playwright/backlog-maintenance/verification.json` | 24项浏览器检查，errors=[]；渲染/真实IPC入参，tracker落盘另由Rust回归验证。 |
| `output/backlog-merge-ui/runtime.log` | 本轮完整 runtime exit0，当时含15项 backlog；后新增事项由24项定向再覆盖。不是发布 Full。 |
| `output/backlog-merge-ui/overview.log` / `spaces.log` | 54项/38项，通过；侧栏与工作台布局相邻合同。 |
| `output/audit-first/automation/fix-core-tests.log` | 前轮 core 424/424，D-760 直接回归。 |
| `output/audit-first/automation/fix-tools-tests.log` | 只引用明确通过的 gate_checklists 等单项；该旧整份日志不是全绿证据。 |

本轮新增 T-1786922727132–7136 分别记录先读后写、维护、架构、telemetry、浏览器定向；通过公开 TestRecordTool 的后续浏览器记录补了源码指纹。测试范围均按真实命令记载，未假称完整发布验证。最终代码提交与正式 Full 由联合发布主线程执行，本报告不预先声称 Full 通过或已发布。

## 最终盘点（冻结）

- 11项先通过 close 成为终态，随后单独执行 req archive / defect archive，已实际移入归档：3条需求、8条缺陷；不是仅在活跃文件改状态。
- 当前活跃清单 **38条需求 + 16条缺陷 = 54条**；其中 **49条开发队列 + 5条待外部验收**。需求为 doing 24、todo 13、awaiting_external 1；缺陷为 fixing 3、open 9、awaiting_external 4。
- R-344 为 doing；R-366 原归档只有一个副本。11项原验收/内容/复现/影响和原进展完整保留，活跃无重复，归档各一份。机器校验记录：`output/backlog-audit-2026-10-04/closeout-integrity.json`。
- `git diff --check` 检出 tracker 序列化空字段固有的尾空格（如 `- 阻塞: ` / `- 停车: `）；未为清理格式绕过受管写入 API。无实际状态或内容丢失。
- 本代理未修改产品源码，未提交，未运行 Cargo；实际测试由联合发布主线程/相邻任务执行并提供日志。报告与清单现已冻结，可进入最终提交与正式 Full。
