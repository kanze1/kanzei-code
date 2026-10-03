# 需求清理后端收口

范围：审查当前 dev 的差异块与直接调用合同，保留已经合入 dev 的39项系统修复；未提交、未跑Cargo、未改实际需求清单。

backlog-maintenance 路径仅剩 .codex-worktree-name，git worktree list 无该树，因此没有从不存在的副本覆盖；审查以当前工作区差异为准。

依赖顺序：Entry/DocKind → DocStore 流转与完整性 → 两个调度消费者 → tracker update/close/reopen → work claim/release。

确认合同：待外部验收不自动取活、不占开发 WIP、不阻塞开发依赖，也不代表验收通过或归档；取消/不修复必须留理由，允许保留未完成验收而不伪造交付证据；done/fixed 仍走交付门禁。终态仍不通过 reopen 复活，原有终态纠错入口不变。

## crates/kanzei-memory/src/docstore/model.rs

### 职责
状态词表及 Entry 状态真源。

### 判断
P1

### 确切问题
BACKLOG-STATE-01：状态迁移未同步旧镜像，导致后续合法操作被完整性门禁拒绝。

### 修改
新增 sync_status_fields，统一已存在的状态正文副本同步。

### 影响范围
现有 Markdown 格式兼容；header 仍为状态真源。直接 caller 见依赖顺序；不改业务清单。

### 验证
静态 caller/状态合同检查；新增两条端到端工具回归，Cargo 由 root 统一执行。

## crates/kanzei-memory/src/docstore/validation.rs

### 职责
校验合法状态流转与完整性。

### 判断
PASS

### 确切问题
未发现有实际影响的问题。

### 修改
保留待外部验收返回 doing/fixing 的合法路径，无额外改动。

### 影响范围
现有 Markdown 格式兼容；header 仍为状态真源。直接 caller 见依赖顺序；不改业务清单。

### 验证
静态 caller/状态合同检查；新增两条端到端工具回归，Cargo 由 root 统一执行。

## crates/kanzei-memory/src/scheduling.rs

### 职责
记忆注入的可执行任务与依赖判断。

### 判断
PASS

### 确切问题
未发现有实际影响的问题。

### 修改
保留 awaiting_external 排除候选、满足开发依赖的实现。

### 影响范围
现有 Markdown 格式兼容；header 仍为状态真源。直接 caller 见依赖顺序；不改业务清单。

### 验证
静态 caller/状态合同检查；新增两条端到端工具回归，Cargo 由 root 统一执行。

## crates/kanzei-tools/src/tracker/actions.rs

### 职责
更新与关闭条目、执行交付门禁。

### 判断
P1

### 确切问题
BACKLOG-STATE-01：状态迁移未同步旧镜像，导致后续合法操作被完整性门禁拒绝。

### 修改
更新路径复用 Entry 状态同步；保留取消/不修复要求 reason，并保留原验收。

### 影响范围
现有 Markdown 格式兼容；header 仍为状态真源。直接 caller 见依赖顺序；不改业务清单。

### 验证
静态 caller/状态合同检查；新增两条端到端工具回归，Cargo 由 root 统一执行。

## crates/kanzei-tools/src/tracker/actions/maintenance.rs

### 职责
带理由重新打开及其他维护动作。

### 判断
P1

### 确切问题
BACKLOG-STATE-01：状态迁移未同步旧镜像，导致后续合法操作被完整性门禁拒绝。

### 修改
reopen 同步旧状态副本，避免合法重新打开后完整性门禁自锁。

### 影响范围
现有 Markdown 格式兼容；header 仍为状态真源。直接 caller 见依赖顺序；不改业务清单。

### 验证
静态 caller/状态合同检查；新增两条端到端工具回归，Cargo 由 root 统一执行。

## crates/kanzei-tools/src/tracker/fields.rs

### 职责
字段语义登记。

### 判断
PASS

### 确切问题
未发现有实际影响的问题。

### 修改
外部验收/关闭原因已登记且有消费者；计数及分类测试保留。

### 影响范围
现有 Markdown 格式兼容；header 仍为状态真源。直接 caller 见依赖顺序；不改业务清单。

### 验证
静态 caller/状态合同检查；新增两条端到端工具回归，Cargo 由 root 统一执行。

## crates/kanzei-tools/src/tracker/scheduling.rs

### 职责
执行取活与依赖调度。

### 判断
PASS

### 确切问题
未发现有实际影响的问题。

### 修改
待外部验收不作为可执行/WIP/阻塞候选，对下游开发依赖视为满足；不当作真正终态归档。

### 影响范围
现有 Markdown 格式兼容；header 仍为状态真源。直接 caller 见依赖顺序；不改业务清单。

### 验证
静态 caller/状态合同检查；新增两条端到端工具回归，Cargo 由 root 统一执行。

## crates/kanzei-tools/src/work.rs

### 职责
统一执行状态和取得线释放。

### 判断
P1

### 确切问题
BACKLOG-STATE-01：状态迁移未同步旧镜像，导致后续合法操作被完整性门禁拒绝。

### 修改
保留 pending_external 分区；release 将 doing/fixing 退回初态时同步旧状态副本。

### 影响范围
现有 Markdown 格式兼容；header 仍为状态真源。直接 caller 见依赖顺序；不改业务清单。

### 验证
静态 caller/状态合同检查；新增两条端到端工具回归，Cargo 由 root 统一执行。

## crates/kanzei-tools/src/work/tool.rs

### 职责
执行 claim 和 work-unit 父需求激活。

### 判断
P1

### 确切问题
BACKLOG-STATE-01：状态迁移未同步旧镜像，导致后续合法操作被完整性门禁拒绝。

### 修改
两个真实状态写入口同步旧状态副本，防止取活后状态分叉。

### 影响范围
现有 Markdown 格式兼容；header 仍为状态真源。直接 caller 见依赖顺序；不改业务清单。

### 验证
静态 caller/状态合同检查；新增两条端到端工具回归，Cargo 由 root 统一执行。

## crates/kanzei-tools/src/tracker/maintenance_tests.rs

### 职责
需求清理和状态流转合同回归。

### 判断
PASS

### 确切问题
未发现有实际影响的问题。

### 修改
新增 reopen_external_acceptance_preserves_reason_and_status_mirrors、claim_and_release_keep_legacy_status_mirrors_consistent 两项回归。

### 影响范围
现有 Markdown 格式兼容；header 仍为状态真源。直接 caller 见依赖顺序；不改业务清单。

### 验证
静态 caller/状态合同检查；新增两条端到端工具回归，Cargo 由 root 统一执行。

# Module Summary

## 已修复
- P0：0。
- P1：BACKLOG-STATE-01（1项根因，5个写入口）。
- P2：0。

## PASS 文件
- validation.rs、两个 scheduling.rs、fields.rs、maintenance_tests.rs。

## 仍需人工判断
- 无。

## 依赖影响
- 新增 Entry::sync_status_fields；没有持久化格式或UI字段变化。

## 剩余风险
- 编译和回归尚未由本代理执行，root已经收到冻结及测试清单；不能将静态检查记作测试通过。

## 集成验证补记
root 已执行维护回归 6/6 通过（包含两条新增状态同步回归），全工作区 all-targets Clippy 与格式检查通过。完整 workspace 测试由最终提交的正式 Full 执行，以上为冻结后实测结果。
