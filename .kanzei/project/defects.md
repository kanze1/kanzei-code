# Defects

## D-031 自主选择后刷新导致进入页面选项异常 [open] (high)
- 原始描述: 自主推进的模式我选了之后，似乎每次进来选项会被刷新
- 复现: 1.选择/开启自主推进模式; 2.进入页面或返回查看
- 优先级: medium

## D-033 子代理调用慢 - 可能未启用并发导致？ [open] (medium)
- 原始描述: 主要模型调用的子代理似乎比较慢，是因为没启用并发吗？
- 复现: 观察主模型调用时，检查是否启用了并发机制。

## D-038 队列输入相关问题 [open] (medium)
- 原始描述: 排队输入相关的功能可能有点问题
- 复现: 待补充

## D-041 R-059 通知 sequence 未按 thread_id 隔离 [fixed] (medium)
- 原始描述: R-059 通知 broker 的 sequence 当前全局递增，不同 thread_id 的通知交错时，单线程订阅会看到其他线程造成的跳号。
- 复现: 1. 发布 thread_a 通知；2. 发布 thread_b 通知；3. 再发布 thread_a 通知；4. 以 thread_a cursor replay，观察 sequence 不连续。
- 根因: InMemoryBroker 只有一个 next_sequence，replay_notifications_for_thread 过滤线程后仍使用全局序号。
- 验收: 每个 thread_id 的通知 sequence 从 1 独立递增；线程订阅 cursor 不受其他线程通知影响；全局 replay 行为有明确且不与线程订阅混淆的语义。
- 优先级: P1
- 修复: 将通知 sequence 从 broker 全局计数改为按 thread_id 独立计数；补充交错发布 A1/B1/A2 时 A=1/2、B=1 的回归测试，thread cursor 不再受其他线程影响。
- 验证: cargo test -p kanzei-core 21 项通过；cargo test -p kanzei-app 1 项通过；scripts/r050-poc-check.ps1、git diff --check 通过。
