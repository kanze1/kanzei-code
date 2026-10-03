# 第三批：请求结构、工具调用归属与模型流结束

日期：2026-10-03。起点：4671709a。按既有地图从 request 进入 Chat Completions 状态机，再沿结束事件消费链检查 client 与 runner。未发布。

## 依赖与状态归属

request.rs（请求/消息/工具 schema 类型）→ protocol/openai.rs（请求构造、调用累积、结束原因）→ client.rs（HTTP 字节流、统一终态验证）→ core/runner/drive.rs（整步收集成功后再调工具）→ 工具副作用。

- 每个 OpenAiState 独占本轮 calls、finish、usage，无跨请求共享状态。调用槽是内部实现，call id 是已创建调用的身份；网关复用 wire index 时，新分配槽不能在后续帧中丢失。
- HTTP EOF 只表示传输结束，模型完成由协议终态确定。client 消费协议层 StepFinish；runner 只有在整步成功后才开始执行收集到的 ToolCall。
- 保留 D-424：finish_reason 后仍可接参数增量，无 [DONE] 时在 HTTP EOF 收尾；不能扩大成“无任何完成信号也成功”。保留 D-422：Responses 网关可省略 item.done，由 response.completed 收尾。
- 修改前已搜索全部 slot_for、ProtocolState::finish、SseParser/client 和 runner Completed/execute_tool_calls 调用。没有公共方法签名或序列化字段改动。

## crates/kanzei-llm/src/request.rs

### 职责
定义请求、消息、工具声明及各 builder 共用的思考档位转换。

### 判断
PASS

### 确切问题
- 未发现有实际影响的问题。

### 修改
- 无。保留 Off 与 None、原始 Hosted 内容、工具结果身份，以及字符数统计的现有含义。

### 影响范围
- 四种协议 builder、runner 请求装配；无兼容性变化。

### 验证
- 全文检查字段与辅助函数，对照 builder 和 assembly caller；既有参数与序列化相关测试通过。
- 不把当前硬编码模型表的静态检查描述为所有在线供应商参数的实时验收。

## crates/kanzei-llm/src/protocol/openai.rs

### 职责
构造 Chat Completions 请求，累积文本、思考、工具参数与用量，输出统一事件。

### 判断
P1

### 确切问题
- 已分配到替代槽的调用在续帧重复发送相同 id 时，会被再次分配新槽；同一调用被拆成残缺和重复的 ToolCall。缺 index 的交错续帧也会丢失原槽归属。
- finish() 在未收到任何完成信号时仍物化 pending calls，并合成成功 StepFinish，把异常结束变成可执行工具的成功步骤。

### 修改
- 已知 call id 优先回到原槽；新 id 的既有 index 冲突/缺失兼容规则继续保留。
- EOF 补收尾必须已有 finish_reason；显式 [DONE] 与原有 finish_reason 后参数增量兼容规则不变。

### 影响范围
- 兼容端点的工具参数归属与结束语义；公开 API、请求 JSON 和持久化格式不变。

### 验证
- 修复前两个测试失败：同槽/缺索引时 A、B、A、B 续帧出现重复残缺调用；无完成信号 EOF 仍放出 ToolCall。
- 修复后两个独立调用各出现一次且参数完整；未确认完成的 pending call 不放出。
- 既有 D-424 同槽、缺索引、finish_reason 后续参数、无 [DONE] 正常收尾回归全部保留通过。
- 修复前证据：output/protocol-before-20261003.log。

## crates/kanzei-llm/src/client.rs（结束消费切片）

### 职责
把 HTTP 分片交给协议状态机，并向 runner 暴露本轮成功或失败。

### 判断
P1

### 确切问题
- 四种路由在 HTTP 正常 EOF、但未收到模型终态时可无错误结束。Responses/Anthropic 可能已放出工具条目，runner 会把无错误 EOF 当成功，继续执行这些调用。

### 修改
- 跟踪从 step/finish 发出的 StepFinish；整个响应消费完仍无终态则返回 Protocol 错误。
- 沿用现有错误策略：该错误不冒充成功，也不擅自新增请求重放。

### 影响范围
- 全部四种协议与所有 LlmClient caller。真实终态响应不变；此前被误接受的空/未完成响应改为明确失败。

### 验证
- 本地真实 HTTP 覆盖四种路由，每种含空响应、未完成响应和正常终态对照，共 12 种输入。
- 失败路径最后一个事件为 Protocol 错误且无 StepFinish；正常对照恰有一次 StepFinish。
- runner 集成测试确认已经发出的 Responses ToolCall 仍不能在未完成整轮中执行。
- 本轮仅审完结束消费切片；超时、认证、代理、连接生命周期仍待后续全文审查。

## crates/kanzei/tests/integration/incomplete_stream.rs

### 职责
通过真实 HTTP → LlmClient → runner 验证未完成整轮不能执行已收集的工具。

### 判断
PASS

### 确切问题
- 未发现有实际影响的问题；此文件是本轮新增的上述 P1 回归证据。

### 修改
- 模拟收到完整 response.output_item.done 工具条目后正常关闭 HTTP，但不发送模型终态。

### 影响范围
- 仅隔离临时项目与本地 TCP 服务，不使用在线凭据。

### 验证
- runner 返回包含 completion event 的错误；ToolStart 为零，StepEnd 为零。
- 服务读取完整请求再返回，测试等待有界，不用 mock 执行器绕过真实 runner。

## crates/kanzei/tests/integration/main.rs

### 职责
登记单一 CLI 集成测试 target 的测试模块。

### 判断
PASS

### 确切问题
- 未发现有实际影响的问题。

### 修改
- 登记 incomplete_stream 回归模块。

### 影响范围
- 测试入口；没有生产行为变化。

### 验证
- 新回归被实际执行，未因文件未登记成为零测试假通过。

# Module Summary

## 已修复
- P0：无。
- P1：调用身份与内部槽漂移；无模型终态的 HTTP EOF 被误判成功，可能执行未完成步骤中的工具。
- P2：无。

## PASS 文件
- request.rs；新增集成测试与其登记入口。PASS 不代表四种协议文件已全部审完。

## 仍需人工判断
- 无。

## 依赖影响
- 无新的依赖、公共签名或持久化格式变化。
- 原生协议结束事件是单一完成凭据；网络正常关闭不再独立构成成功。

## 剩余风险
- 没有运行需要账号和额度的 live probes。
- 其他协议全文、proxy/auth 和 client 完整生命周期还未完成；core/runner/drive.rs 本次只跟踪了消费/分发边界。

## 下一步

继续 Responses 与 Anthropic 的全文审查，核对参数累计、权威 done 数据、去重、reasoning/usage 状态；之后处理 proxy/auth，再完成 client 与 core 持久化层。

## 验证

- `cargo test --workspace --no-fail-fast --quiet`：2224 通过、0 失败、5 项原有忽略；包含 63 个 LLM 测试与 40 个 CLI 集成测试。
- `cargo clippy --workspace --all-targets -- -D warnings`：通过。
- `cargo fmt --all -- --check`、`git diff --check`：通过。
- 全量日志：output/protocol-review-workspace-20261003.log、output/protocol-review-clippy-20261003.log。
- runner 专项日志：output/incomplete-turn-test-20261003.log。
- 本轮实际修改两个生产文件，新增/登记一个集成测试；未修改数据库 schema、工具实现或网络重试上限。
