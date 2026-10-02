# R-379 第一批接入验证

日期：2026-09-27。基线：`21c36e9d` 加现有未提交工作。当前结果属于源码与本地验证，未发布或替换安装版。

## 已完成的检查

| 检查 | 结果 | 覆盖范围 |
| --- | --- | --- |
| `cargo test --workspace --no-run --quiet` | 通过 | 全工作区测试目标编译，包含 ToolCtx 新 session_id 的装配兼容 |
| `cargo test -p kanzei-core -- --quiet` | 337 通过 | 决策持久化、同运行重试、不同运行隔离、纠正入原会话、版本冲突、独立人工验收 |
| `cargo test -p kanzei-harness -- --quiet` | 192 通过 | 公共上下文与自动续跑策略回归 |
| `cargo test -p kanzei-app --bin kzapp -- --quiet` | 379 通过 | 真实 review/accept 命令、项目记忆范围、重试、图谱来源、自动提问停机判定 |
| 新增命令 IPC 契约 | 通过 | Rust 命令实际输出生成契约，浏览器夹具与同一契约比较 |
| 修改 UI 模块的 ESLint | 通过 | 控制台、工具摘要、国际化与图谱相关模块 |
| `node --experimental-vm-modules scripts/ui-runtime-smoke.mjs` | 通过 | 73 个 UI 模块、4042 次模拟 invoke、10 个主视图、0 运行时错误 |
| `node scripts/ui-decision-console-smoke.mjs` | 通过 | 真实 Edge 页面中的批量通过、跨项目纠正、双击去重、旧证据失效、人工验收、失败保留草稿、窄屏及英文亮色 |
| 图谱纯函数、变更与浏览器回归 | 通过 | 小图、大图、选中关系、隐藏页面暂停、减少动效 |

图谱浏览器样本：65 个节点 / 89 条边在约 465 ms 稳定；677 个节点 / 1866 条边在约 904 ms 稳定。减少动效样本首次布局约 22 ms，首帧之后没有物理布局 tick。这些是本机夹具结果，不外推到任意规模。

浏览器截图：`output/playwright/decision-console/projects-dark.png`、`decisions-dark.png`、`correction-mobile.png`、`decisions-light.png`。预览使用真实前端与内存中的示例 IPC，不读取或改写真实项目决策。

## 本批范围

已打通：自动选择并留档 → 现有项目控制台 → 用户纠正 → 原会话队列 → 显式范围偏好 → 图谱来源。机器完成与人工复核分别展示。既有旧会话、记忆、需求及其他未提交工作保留。

尚未验证：安装版运行、真实模型连续多轮决策及后续偏好采用。尚未实现：冻结版本的后台验证调度、闲置线路收到纠正后自动唤醒、交付产物直接启动入口。R-379 保持 doing。

接入边界和职责见 [接入说明](../design/decision_console_integration.md)。

后续进展：同日第二批已实现冻结快照后台验证，并完成手机阅读器真实实验；上面的“尚未实现”是第一批结束时的边界。最新结果见 [阅读器实验报告](2026-09-27-reader-verification-lab.md)。
