# WB1 工作台状态与回复路由

日期：2026-10-03。源码起点 `746b69a824f38b4d5fc5502c4ec9ff78bc06e956`。本包新增全文审查 6 文件，修改 4 个生产文件，修复 **P1 × 3、P2 × 1**；没有 Rust、数据库、IPC 或持久草稿格式变更，没有发布。

## 依赖地图与审查顺序

```text
AppState.runtimes / SessionRuntime.asks + pending_question 持久记录
  → durable_questions / core question mailbox 的交付与结算
  → app/softwire.rs：问题投影、版本校验、送达收据
  → 25-softwire-model.js：目标身份、原问题选项、展示投影
  → 25-softwire-view.js：只渲染投影，回传原选项

03-workspaces.js：native 编辑器 scope 与离开后的草稿缓存
  → 25-softwire-composer.js：借用同一编辑器，按目标保存模块草稿
08-compose-runtime.js：每条线路的自动运行状态更新队列
  → 25-softwire-run.js：显示选中线路、提交动作、归属失败回执
  → 25-softwire.js：导航、捕获原接收者、问题/历史加载、提交编排
```

状态 owner 没有迁移。修复发生在草稿恢复、迟到回执和选项身份的接口处；没有另建会话存储。后端 sender、持久问题、mailbox 与已送达状态分别由现有 owner 负责，不能把 UI 成功收据扩大解释为业务已经完成。

设计依据：[Softwire 操作台](../../design/softwire_context_workbench.md) 的目标冻结、独立草稿和原问题版本合同；[工作台优先](../../design/workbench_first.md) 的切页保留草稿合同。这里只引用已有设计，不沿旧设计中的全产品计划扩大当前四块范围。

## crates/kanzei-app/src/softwire.rs

### 职责
读取等待问题，校验项目、会话及 revision，将回复交给原等待通道并返回幂等收据。

### 判断
PASS

### 确切问题
- 未发现有实际影响的问题。

### 修改
- 无；持久答案成功后再移除 sender，关闭 sender 不产生成功收据，原合同保留。

### 影响范围
- 直接 caller：`25-softwire.js`、main IPC 注册；下层为 durable_questions / pending_question / core question mailbox。无兼容性变化。

### 验证
- 全文及 4 个现有测试检查；沿实际问题创建、Stop 取消、持久交付和 mailbox settle 路径核对。现有 Rust 测试本轮未重跑，不计入 267 项。

## crates/kanzei-app/ui/25-softwire-model.js

### 职责
统一工作台目标 key、问题投影、运行状态及展示数据的纯函数。

### 判断
PASS

### 确切问题
- 未发现有实际影响的问题。

### 修改
- 无；选项原值和模块草稿 key 合同不变。

### 影响范围
- 直接 caller：view、composer、controller，以及浏览器验证脚本。无格式或 API 变化。

### 验证
- 全文审查；267 项相关检查中的目标隔离、模块草稿、问题版本与原选项回传测试使用实际 model 实现。

## crates/kanzei-app/ui/25-softwire-view.js

### 职责
渲染工作台状态和问题选项，将明确点击交给 controller。

### 判断
P1

### 确切问题
- **WB1-01**：英文 UI 把模型选项“回答/取消”翻译为 `Answer/Cancel` 再回传；controller 按原标签删除旧单选，结果草稿留下两个选项并可一起发送。

### 修改
- 模型选项显示、data 属性及提交均保留原值；只对内建“本次通过/验收通过/需要修改”动作翻译，避免改变问题答案身份。

### 影响范围
- 直接 caller：controller 的 chooseReply / sendReply。无后端协议变化；英文 UI 中模型原文选项不再被词表替换。

### 验证
- 旧 Edge 真实负例得到 `keep my explanation\nAnswer\nCancel`，断言失败且浏览器无错误；修复后 8 项通过，包括仅保留一个答案、说明保留、原项目/session/revision 和内建英文动作。

## crates/kanzei-app/ui/25-softwire-composer.js

### 职责
借用 native 输入节点，管理每个项目/会话/模块/问题的草稿、附件及送达后的清理。

### 判断
P1

### 确切问题
- **WB1-02**：主对话切到工具等模块后离开概览，模块草稿覆盖主对话草稿；主对话中尚未发送的输入丢失。
- **WB1-03**：发送在途时离开概览或切项目，成功回执只清模块 Map，不清恢复到 native 的草稿及其缓存；返回后已发送内容复活。

### 修改
- 只有主对话可更新借来的 native 草稿；在模块切换前记住最新主对话内容。
- 成功回执按原目标和提交快照，条件清理 native 快照及 workspace 缓存；正文或附件变更后保留新输入。失败不清理。

### 影响范围
- 唯一直接 caller：controller；依赖 03-workspaces 的草稿 owner API。没有 localStorage 格式变化，不改变模块独立草稿和附件语义。

### 验证
- 完整 ESM owner 回归 17 项和真实 Edge 回归 12 项通过；覆盖同会话离开、跨项目/跨 process、不同模块、晚到 ACK、新正文/附件保护及失败保留。旧 owner 回归 8 个断言失败，其中多个断言对应同一根因，不重复计 bug。

## crates/kanzei-app/ui/25-softwire-run.js

### 职责
把选中线路的自动运行配置投影为控制按钮，并提交该线路动作。

### 判断
P2

### 确切问题
- **WB1-04**：A 操作在途时切到 B，A 的保存失败会显示到 B 的控制栏。请求本身仍发给 A，影响是失败状态归属错误。

### 修改
- 捕获动作的项目、process、session；失败只回写仍匹配该接收者的控制栏。解析时新建线路使用解析后的身份。

### 影响范围
- 唯一直接 caller：controller；下层 setLineAutoState 队列及业务配置无变化。

### 验证
- 完整 ESM 回归证明旧实现把 A 错误写到 B；修复后 B 不显示 A 错误，同 A 的真实失败继续可见。现有概览 54 项检查通过。

## crates/kanzei-app/ui/25-softwire.js

### 职责
编排工作台导航、目标捕获、状态加载、问题回复和消息提交。

### 判断
PASS

### 确切问题
- 未发现需在本文件独立修复的实际问题；WB1-01/02/03/04 根因已在选项和草稿/操作 owner 修复，不在 caller 叠补丁。

### 修改
- 无。

### 影响范围
- 消费上述模型、视图、输入和运行控制；调用 process_list/run_prompt/softwire_answer_question 等既有 IPC。业务专题仅核对 UI 接口，未修改任务管理、记忆或科研实现。

### 验证
- 全文阅读；现有浏览器 97、概览 54、导航 41、空间隔离 38 项及两组新增 Edge 检查通过。包括原目标冻结、问题 revision、迟到历史快照、失败草稿保留和不因导航启动任务。

## crates/kanzei-app/ui/03-workspaces.js（关联合同复核）

### 职责
持有 native 编辑器实际 scope 及离开后的草稿缓存。

### 判断
P1

### 确切问题
- WB1-03 的缓存边界：发送成功后原 scope 缓存仍保存提交正文。与 composer 同一根因，不新增问题计数。

### 修改
- 增加 acknowledge_composer_draft，由缓存 owner 清理匹配的提交快照；使用实际 composer_scope，防止导航先更新全局变量后误清其他编辑器。

### 影响范围
- 新 caller 只有 composer.clear；原 sync_composer_scope 调用方、导航和持久偏好合同不变。复核现有全文文件，不增加主审完成数。

### 验证
- 完整生产 ESM 同时加载真实 workspace 缓存；跨项目/跨 process 回到原 scope 不复活、其他 scope 和新草稿不被清空。真实 Edge 再验证同路径。

## 验证证据

| 检查 | 结果 |
| --- | --- |
| `node --experimental-vm-modules scripts/ui-softwire-ownership-smoke.mjs --before` | 旧基线 9 PASS / 8 FAIL，真实断言退出 1 |
| 同脚本当前源码 | 17 PASS / 0 FAIL |
| `ui-softwire-draft-browser-smoke.mjs` | 12 PASS |
| `ui-softwire-choice-smoke.mjs` | 旧 Edge 目标断言失败；修复后 8 PASS |
| 原 `ui-softwire-browser-smoke.mjs` | 97 PASS |
| 原 `ui-softwire-overview-smoke.mjs` | 54 PASS |
| 原 `ui-softwire-flow-smoke.mjs` | 41 PASS |
| 原 `ui-softwire-spaces-smoke.mjs` | 38 PASS |
| 修改文件及新测试 ESLint、runtime gate 语法、git diff --check | 通过 |

合计 **267 项通过**，Edge 六组均无 pageerror。新三个脚本已接入既有 `ui-runtime-smoke.mjs` 门禁；本轮运行相关脚本，未重跑整个发布 Full、Rust 全工作区或 native Tauri IPC。无 Rust 源码修改，浏览器交付使用隔离 IPC fixture。

旧负例和新增回归证据在 `output/audit-WB1/`；既有 Edge 验证在 `output/playwright/softwire/`。实际源码指纹与报告计数记录在 coverage.json 的 WB1 batch。

# Module Summary

## 已修复
- P0：0。
- P1：3，原选项身份、模块覆盖主草稿、迟到送达回执导致草稿复活。
- P2：1，旧线路操作错误显示到新线路。

## PASS 文件
- softwire.rs、25-softwire-model.js、25-softwire.js。

## 仍需人工判断
- 无。

## 依赖影响
- 仅增加 native 草稿 owner 的条件确认 API；调用方固定为 Softwire composer。
- 模型选项回到原值合同；系统按钮继续翻译。未改 IPC、持久格式、数据库或锁语义。

## 剩余风险
- 本轮未执行 native Tauri 和发布 Full，不能作为新版本发布验证；本包没有待解决的已确认缺陷。
- WB1 完成不等于整个工作台完成：工作台还剩 32 文件，四块合计还剩 201 文件。第二块 C7 的 3 个既有 P1 仍未修复，不计入本包。
