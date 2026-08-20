# Work Unit 底座：把 Outcome、执行状态与历史分开

## 1. 为什么重构

旧模型把一条 Requirement 同时当作用户目标、排期单元、WIP lease、批次容器、进展日志、上下文快照、验收清单和审计档案。短任务里这些角色看起来相近，长程任务中却会互相放大：每完成一批都向同一段 Markdown 追加进展、锚点、对账和例外，恢复任务时模型又必须把整条历史重新读入，才能判断“现在究竟做到哪”。

于是上下文成本随历史长度增长，而不是随当前决策复杂度增长。更糟的是，模型既是执行者，又承担压缩历史、维护状态机和证明自己完成工作的职责；一旦压缩遗漏或字段陈旧，后续轮次会重复实现、错误关闭或继续扩写防御性说明。

本设计把这几个角色拆开：

- Requirement 是长期 Outcome：保存用户为什么要它、边界和 Outcome 级验收。
- Work Unit 是短期执行单元：一次可装载、可认领、可验证的工作。
- Work Event 是不可变事实：创建、认领、checkpoint、阻塞、证据、完成等只追加。
- Work Projection 是当前表面：由事件重放得到，可删除后重建。
- Context Capsule 是给模型的有界视图：只含当前单元、父 Outcome 白名单字段和最后 checkpoint。

## 2. 真源与派生关系

```text
requirements.md (Outcome 真源)
          │
          ├── 执行模型: work_units_v1（显式启用）
          │
          └── R-123
                ├── R-123/W1 ── work_events ──┐
                ├── R-123/W2 ── work_events ──┼── replay ── work_surfaces
                └── R-123/W3 ── work_events ──┘                │
                                                               ├── work next
                                                               ├── 模型 Context Capsule
                                                               └── 桌面需求详情
```

Markdown 仍是人可编辑的产品目标真源。SQLite v18 只承担高频执行事实与派生投影，不接管需求文本。`work_surfaces` 可以从 `work_events` 完全重建；事件表不能被投影覆盖。

## 3. Work Unit 契约

单元 ID 由引擎分配为 `R-xxx/W<n>`。创建时冻结：

- `objective`：这个单元完成后产生什么结果；
- `scope`：允许触碰的模块或文件范围；
- `dependencies`：必须先完成的 Work Unit；
- `acceptance`：逐条登记证据的验收标准；
- `verification`：建议执行的验证命令或人工路径；
- `base_revision`：拆分时看到的代码基线。

为了避免新容器再次无限膨胀，存储层执行硬预算：目标最多 1,000 字符，各列表最多 32 项、单项最多 2,000 字符，checkpoint 摘要最多 4,000 字符。超预算直接拒绝写入，而不是截断后假装成功。

## 4. 状态机

```text
ready ── claim ──> active ── verify ──> verifying ── complete ──> done
  │                  │  │                    │
  │                  │  └── checkpoint ─────┤
  │                  └──── block ──> blocked ── unblock ──> ready
  └──────────────────── supersede ─────────────────────────> superseded
```

- `claim` 只允许 `ready`；并行线接管使用独立的 `reassigned` 事件并强制写理由。
- `checkpoint` 覆盖投影里的“最后 checkpoint”，但旧 checkpoint 仍在事件表。
- `complete` 只允许从 `verifying` 进入，并要求每一条 acceptance 都有同名 evidence。
- `done` 与 `superseded` 是终态，终态后拒绝追加事件。
- Work Unit 依赖只有在被依赖单元为 `done` 时才满足；`superseded` 不等于交付完成。

## 5. 上下文胶囊

旧 Requirement 继续按原路径工作。启用 `work_units_v1` 后，`resolved-control-state.selected` 的 `fields` 为空，模型只收到：

- 当前 Work Projection；
- 父 Outcome 的 `目标/内容/边界/验收/参考/refs`；
- 最后 checkpoint 的摘要、下一步、关键决策、按需检索引用和仓库锚点；
- 当前单元自身的依赖、验收与证据。

父需求里的批次、历史进展、对账、停车审计和旧锚点不进入当前单元上下文。模型若要审计历史，显式调用 `work get-unit` 读取事件；正常执行不为“可能会用到”预付全部历史 token。

这把执行期上下文从“随 Outcome 历史增长”改为“受单元预算上界约束”。审计信息仍完整保存，只改变默认装载策略。

## 6. 调度与 WIP

启用新模型的父 Requirement 退出旧的整条需求 WIP 路径。`work next` 按以下顺序处理：

1. 当前线已有 `active/verifying` 单元时返回 Resume；多个则返回 WipViolation。
2. 选择依赖已完成、父 Outcome 未阻塞/停车的 `ready` 单元。
3. `blocked` 单元进入阻塞清单，不占当前线 WIP。
4. 其他线持有的单元进入 `foreign_wip`；显式接管必须给理由。
5. Requirement-first/Defect-first 仍决定 Work Unit 队列与 legacy defect/requirement 队列的先后。

认领第一个单元时，父 Requirement 转为 `doing`，但不再在父文档写取得线和执行进展；这些事实属于单元事件。

## 7. Outcome 关闭

`req close` 对 `work_units_v1` 增加硬门禁：

- 至少创建过一个 Work Unit；
- 所有 Work Unit 都已终态；
- 至少一个 Work Unit 为 `done`，不能用“全部 superseded”冒充交付；
- 每个 `done` 单元在自己的完成事件前已经逐条覆盖验收证据；
- 父 Requirement 原有的批次、测试记录和 Outcome 验收门禁继续执行。

单元全部终态而父 Outcome 未关闭时，`work next` 返回明确的 `req close` 收口提示。

## 8. 命令面

```powershell
kz work create-unit --requirement R-123 --objective "实现事件存储" `
  --scope crates/kanzei-core --acceptance "事件可回放" `
  --verify-with "cargo test -p kanzei-core"

kz work claim R-123/W1
kz work checkpoint R-123/W1 --summary "表结构已落地" --next-action "跑迁移测试" `
  --retrieval-ref crates/kanzei-core/src/store/work.rs
kz work verify R-123/W1
kz work evidence R-123/W1 --criterion "事件可回放" --evidence "test:work_events_replay"
kz work complete R-123/W1
```

辅助动作：`list-units`、`get-unit`、`block`、`unblock`、`supersede`。CLI 使用短横线，工具 JSON action 使用下划线。

## 9. 迁移、兼容与回滚

- schema v17 升 v18 前沿用现有 `VACUUM INTO` 整库备份，迁移测试确认旧库对象和列补齐。
- 新行为只对带 `执行模型: work_units_v1` 的 Requirement 生效；所有存量条目保持 legacy 行为，无批量回填。
- 降级旧二进制前应恢复迁移备份；旧二进制检测到更高 schema 会拒绝打开并给出指引，不会盲写。
- `work_surfaces` 损坏时调用重建路径从 `work_events` 回放；不得反向用 surface 覆盖事件。

## 10. 当前边界

本底座解决 Requirement 定义、执行拆分、恢复上下文和验收证据四条主链。缺陷仍是独立队列；“某个 Work Unit 内发现的局部缺陷”如何绑定并影响抢占优先级，留给后续 defect-affinity 设计，不在 v1 偷加含混字段。自动拆分也不在 v1：拆分需要人或上层 Agent 作语义判断，底座负责把结果保存、调度和验证，避免让系统在没有外部校准时靠自身旧定义自举自己。
