# 2026-10-23 文档整理记录

本次按用户“文档梳理清楚，该更新的更新，该清理的清理”的授权整理当前入口与过时口径。核对范围为 `dev` / `ea4f1560` 及当时的未提交工作树。生产源码、发布和安装状态按对应机器证据判断。

## 维护结果

| 范围 | 整理后的合同 |
| --- | --- |
| 文档入口 | `docs/README.md` 统一导航；`docs/目录.md` 保留短兼容入口；`current-state.md` 导向当前实现、约束和事实来源 |
| 代码结构 | `structure-map.md` 改为模块职责、所有者和查找入口，去除旧固定计数、已删除路径与完整文件清单的重复维护 |
| 工程与发布 | 根 README 中英文、`current_contracts.md`、`ci_release_evidence_chain.md` 统一采用本地验证规则，移除可手动触发 GitHub Actions CI 的现行承诺；设计索引同步为 live_design |
| 浏览器 | `preview_pane.md` 按 pane/headless/attached、页面身份、表单动作、存证和权限维护；退役的布局、可见性切换后端及手抄 IPC 参数移出当前正文 |
| 记忆 | `memory_system.md` 和 `memory_feedback_reliability.md` 对齐项目知识、机会/使用/反馈、内容修订、保留归档和对话做梦；旧 INDEX 常驻注入与固定每轮模型成本不再作为当前合同 |
| 交付账本 | `tracker_evidence_ledger.md` 补充现有 deliver 消费路径；原问题推导保留并注明历史适用范围 |
| 旧问题与交接 | 10 月 18 日清单注明历史观察，补浏览器/记忆/来源存证的最新对账；10 月 22 日交接的原待办和顺序标明时点，保留后续修复事实 |
| 下一步建议 | 优化调研保留采样时点与研究结论，指向当前导航；建议不自动变成活动任务 |

整理前的浏览器设计完整保存在 [历史快照](2026-10-23-preview-pane-history.md)。记忆文档内的旧迁移、测试与发布记录继续保留，并明确适用版本。原始问题、验收条件、历史提交与证据没有因口径更新而抹去。

## 受管文档与活动范围

设计身份索引经 `ArchitectureTool.get/update`，带 `expected_hash` 写入；R-353、R-364 经 `TrackerTool.update` 写入。专用写通道的请求与回执保存在 [维护证据目录](../../output/docs-maintenance-20261023)。

- R-353：修正“尚未消费 deliver”的旧进展，指向实际 `work/reconcile.rs`；四态真实实例和原验收继续保留。
- R-364：以现有 B2 实现替代旧“未提交/先由用户隔离”的进展，清除过时解除条件，保留原生延迟协议等原验收。
- 两条仍为 doing，验收和批次未变；未依据源码存在就关闭条目，也未新增研发任务。

架构工具在整理前已报告 `global-skills.md` 不符合 snake_case 的命名问题；本次保留既有路径与引用，写入采用工具对新增问题的校验。工作副本设计身份、覆盖和 tracker 声明另行检查。

## 核对与验证入口

本次文档核对结果见 [文档审计结果](../../output/docs-maintenance-20261023/validation.json)，覆盖编辑文档的本地链接、工作副本设计身份、索引覆盖、受管条目变更范围和正文空白。变更前快照位于同目录 `before/`，可用于区分原有未提交内容与本次维护。

按仓库规则执行 `scripts/verify.ps1`，原始输出保存在 [本地门禁日志](../../output/docs-maintenance-20261023/verify.log)，完成状态见 [执行结果](../../output/docs-maintenance-20261023/verify-result.json)。成功时门禁生成 `dist/verification-targeted.json`，绑定当次 HEAD 与工作树摘要；原定向回执另存于维护证据目录。

共享工作树还有大量原有源码改动，验证计划由脚本对整个当前工作树选择，不能把它的检查数量当成本次文档新增测试数。正式发布仍要求最终干净提交的 `scripts/verify.ps1 -Full`；本次整理不产生发布或安装结论。

## 本次实际检查结果

- 文档专项：18 份文件的本地链接、空白和变更范围核对通过；83 份设计文档的工作副本身份、索引覆盖与 tracker 声明一致。R-353、R-364 的状态、验收与批次保持原值。
- 本地门禁：`scripts/verify.ps1` 选择 82 项，81 项通过、1 项失败，执行约 570 秒。Rust 默认测试汇总为 2735 passed、0 failed、20 ignored；ignored 项本次未显式执行。
- 唯一失败为 `artifact_lifecycle`：其 PowerShell 5.1 子进程在 `scripts/artifact-lifecycle.ps1:50` 找不到 `Get-FileHash`。单独执行 `node scripts/artifact-lifecycle-smoke.mjs` 再次出现同一错误，见 [复跑日志](../../output/docs-maintenance-20261023/artifact-lifecycle-rerun.log)。当前终端可发现该命令，子进程内差异的根因尚未确认。
- 门禁整体失败，未生成新的成功定向回执或发布证据。完整结果与各检查日志索引见 [验证汇总](../../output/docs-maintenance-20261023/verification-summary.json)。本节在门禁结束后补入，文档专项随后按最终正文重跑；本次没有改动生产脚本来处理该回归失败。
