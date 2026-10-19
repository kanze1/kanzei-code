# Kanzei 约束与需求清理记录

- 日期：2026-10-20
- 实现基线：`df452a2cb79055ef7ce9b28a68bf989ba058fc23`
- 范围：当前仓库规则、设计身份、需求和缺陷依赖、未使用的直接依赖。

当前规范已收敛到[当前约束](../design/current_contracts.md)和各专题真源。旧总控及未采用的总体重构退出活动范围；原验收和退役原因保留在 tracker 归档中。真实设备、SSH 和尚未实现的功能继续保留独立验收。

## 文档变更

- 统一 `conversation_modes.md` 与 `global-skills.md` 的全局 Skills 作用域，删除项目覆盖和逐对话绑定的旧定义。
- README、项目规范、Cargo 规则和 CI 专题统一真实分工：提交先检查全部目标的编译，本地 clippy 不含测试目标；测试代码另外检查，CI 手动触发并使用全目标 clippy。CI 与本地共用验证注册表和 ESM 参数。
- 本地产物保留改为发布且安装验收后保留当前及上一版安装器；公开 Release 清理与本地恢复产物分别执行。
- `conventions.md` 改为引用实际 dev 提示词和有效 cadence 策略，移除旧 §1.4 定位与固定冒烟步数；运行中的 pending 更新按下一次启动接力替换。
- 更新 `current-state.md`，将2026-10-04审查队列、旧分支和安装进度归回历史报告。使用手册同步当前 Goal、无连续次数硬上限、对话/管理导航与权限边界。
- `phase2_system_upgrade.md`、`workbench_first.md`、`softwire_context_workbench.md`、`softwire_runtime_refactor.md`、`product_operating_model.md` 转为 superseded，并给出有效替代入口；旧正文保留。
- `context_supply_bill_20260821.md` 转为 historical_snapshot，旧测量和待拍板清单不再参与取活。
- 明确 `direction_taste.md` 的现行原则与旧执行序，以及 tracker 设计中已退出的14天移面和删除 tests.md 草案。

## 退役需求

以下条目使用 `close dropped` 并显式归档，保留原验收和关闭原因。没有把未完成验收改为通过。

| 条目 | 退出原因与当前承接 |
| --- | --- |
| R-283 | 旧二期联合总控包含已退役动画和旧语音路线；按现行专题合同维护 |
| R-287 | 旧多 provider、50条基准和角色动画联合方案退出；当前本地语音设计继续保留设备和敏感数据验证边界 |
| R-281 | 旧独立子代理面板计划由 R-384 及现行团队呈现合同接替；R-369 的独立限制核验仍保留 |
| R-309 | 固定15秒、旧 globals 清单和旧验证步数的验收失去当前适用面；保留当前注册表与正式发布合同 |
| R-312 | 旧阶段上下文测量与四项待拍板退出；R-364、R-373 等当前任务继续按原验收执行 |
| R-319 | 固定步数预算的额外10轮实验退出；保留现有预算兼容机制和运行时不变量 |
| R-323 | B1 审计已说明没有声明层的实际需求；沿用 ToolConcurrency，不再新增占位编排协议 |
| R-357 | 自动移面、删除 tests.md 和新增 gaps 的旧组合方案退出；保留实际 test_record 存储和现行清单维护入口 |
| R-381 | 工作台优先和 Softwire 总体重构由现行对话/管理工作区接替；独立缺口继续保留 |

## 退役历史缺陷

| 条目 | 处置 |
| --- | --- |
| D-568 | 旧污染对象 M-014/M-015 已退出活动记忆，当前36条 INDEX 与源描述一致，FTS 无旧ID，生成路径已有一致性门禁；历史事故以 wontfix 退役 |
| D-662 | 旧删除/搬迁专用工具方案由 resident/deferred catalog 接替；R-364 保留原生延迟加载缺口，合法写通道继续保留 |
| D-769 | 文案已对齐手动 CI 和有意的 clippy 分工；旧提高本地门禁或启用自动 CI 的额外要求退出 |
| D-770 | 历史 insert 锚点使用事故没有独立产品复现和验收，按当前开发规则留执行记录并归档 |

## 依赖与保留边界

R-101、R-249、R-299、R-340、R-353、D-771 的历史队列停车已清除。R-364 的旧未提交源码阻塞已清除。已完成的 R-338/R-339/R-345/R-347/R-366 等前置转入 refs；R-341 只保留未完成的 R-340 为依赖。

R-344 保持 doing：缺环境时询问并记录准备步骤仍缺实现，真实 SSH 运行、取消和断线恢复仍待外验。R-101、R-322、R-370 和相关缺陷的真实设备/安装链路验证不由文档清理替代。R-369 的 schema/嵌套限制、R-378 的回收策略，以及当前认证授权、tracker 提交和发布证据缺陷继续保留。

## 开发依赖

删除 `kanzei-harness` 与 `kanzei-tools` 未使用的 `thiserror` 直接依赖。`kanzei-core` 和 `kanzei-llm` 仍使用该库，workspace 声明保留。本轮在 Cargo.lock 移除这两个包的直接依赖边，不升级包版本。合入的文件锁修复为 `kanzei-base` 增加实际使用的 sha2、Windows 系统 API 和 Unix libc 依赖，沿用锁文件中已有版本。

npm 的 eslint、globals、playwright-core 均有实际调用；React 和 React DOM 由浏览器表单原生验收夹具读取，继续保留。正式前端仍使用原生 ESM。

## 托管空字段格式

清理依赖和停车字段时，现有 `docstore::parse::push_field` 会输出尾随空格。本轮修正该公共序列化入口：空值仍保留为空字段，非空值及换行折叠语义不变，空字段冒号后不再附加空格。通过本轮构建的 CLI 重写活动面和归档面，未直接编辑托管文件。

## 清理结果

下表是执行时工作树的前后快照，包含开始本轮前已经存在、仍保留为未提交工作的 tracker 变更。本轮新增的退役范围为上列9条需求和4条缺陷；编号、验收和进展检查以执行前快照为基准。

| 项目 | 清理前 | 清理后 |
| --- | --- | --- |
| 活动需求 | 29 | 20 |
| 活动缺陷 | 13 | 9 |
| 归档需求 | 355 | 364 |
| 归档缺陷 | 769 | 773 |

设计索引覆盖83份正文：48份 live_design、15份 validated_design、12份 historical_snapshot、8份 superseded。原编号集合不变且每个编号只出现一次，退役条目的原验收、边界、内容和历史进展均保留。活动依赖没有悬空边或环，只剩 R-341 → R-340。

## 验证

| 检查 | 结果 |
| --- | --- |
| `cargo check --workspace --all-targets --offline` | 通过，验证删除未使用直接依赖后的全部目标编译 |
| `cargo build -p kanzei --offline` | 通过，新 CLI 用于托管格式写回 |
| `cargo test -p kanzei-memory --lib docstore --offline` | 39 passed，0 failed |
| `cargo clippy -p kanzei-memory --all-targets --offline -- -D warnings` | 通过 |
| `rustfmt --edition 2021 --check crates/kanzei-memory/src/docstore/parse.rs` | 通过 |
| `node scripts/check-design-freshness.mjs` | 通过，83份设计的身份、替代关系与声明一致 |
| `node scripts/check-readme-crates.mjs` | 通过，8个 crate 与 README 一致 |
| `node scripts/verify-policy-smoke.mjs` | 通过 |
| `req/defect/decision normalize` | 全部 dry-run 为0 finding、0 fix |
| `output/constraints-cleanup-20261020/audit.py` | 编号、原验收、依赖无环和包版本集合检查通过 |
| 本轮文件的 `git diff --check` 与新文档本地链接检查 | 通过 |
| `cargo fmt --all -- --check` | 合入文件锁修复后通过，先前并行锁测试的格式差异已消除 |
| `cargo test --locked --offline --workspace --quiet` | 组合后的 Windows 工作树：2700 passed，0 failed，10 ignored |
| `cargo clippy --locked --offline --workspace --all-targets -- -D warnings` | 组合后的全部目标通过 |

## 合入文件锁修复

按用户要求合入另一项已经完成的文件锁修复。协调文件由完整物理路径（含扩展名）派生到操作系统账号的内部缓存目录，文档数据文件与锁身份分离；等待者和持有者全部退出后回收内存锁槽。文档编辑、记忆树和偏好写事务共用同一入口，权限限定的文档读取与原子替换继续使用目录能力。

新增回归覆盖工作区不生成锁文件、不同扩展名互不阻塞、`.lock` 文档原子替换后仍互斥、锁槽回收、跨进程及不同配置根、Windows 大小写与长路径。组合验证日志为 `output/constraints-cleanup-20261020/integration-workspace-tests.log`、`integration-clippy.log` 和 `integration-fmt.log`。这些结果来自本地 Windows；未将它们作为其它操作系统或桌面安装验收。

清理前工作树已有 tracker 归档、项目配置、记忆索引和报告修改；提交按执行前快照拆分 tracker 的条目变更，只纳入本轮清理、文件锁修复和组合验证记录，既有无关改动继续留在工作树。执行前快照、工具回执和结构核对保存到 `output/constraints-cleanup-20261020/`；本记录不声明发布或安装完成。
