# 第二批：LLM 协议基础层逐文件审查

日期：2026-10-03。起点：e44b709a。主目录已切到 main，并从 dev 快进取得全部底层整合提交；不推送、不发布。

## 地图位置与调用链

依赖层级：base → harness / llm → core → memory → tools → CLI / app → UI。本轮位于 llm 的事件、错误及字节分帧基础层。

HTTP response.bytes_stream → client.rs 的 SseParser::feed → protocol::ProtocolState::step → LlmEvent → core/runner/drive.rs → 持久化与 UI。SSE parser 独占每个响应流的行缓存、事件类型与数据缓存，无共享静态状态；传输结束后的协议收尾由 ProtocolState::finish 拥有。

修改前搜索全部 SseParser 引用：生产调用仅 client.rs；另外有 native_search_probe.rs 的两处手工探测。四个协议实现消费 SseEvent，事件 schema、公开方法签名均未变化。错误分类还核对了 runner 的上下文压缩和限流重试分支。

标准依据：[WHATWG SSE 解析与解释规则](https://html.spec.whatwg.org/multipage/server-sent-events.html#parsing-an-event-stream)。合法行结束包括 LF、CRLF、CR；只剥除流开头一个 BOM；没有最终空行的未完成事件不得因 EOF 自动派发。现有 D-424 说明的协议层 finish 收尾保留，不把它和 SSE 半帧补齐混为一谈。

## crates/kanzei-llm/src/event.rs

### 职责
定义协议归一后的事件、结束原因与用量类型。

### 判断
PASS

### 确切问题
- 未发现有实际影响的问题。

### 修改
- 无。保持本地工具调用和托管工具事件分离，保留 raw_input、HostedItem 和现有序列化字段。

### 影响范围
- 协议状态机、core runner、持久化及 UI 事件转换；无兼容性变化。

### 验证
- 核对各协议构造事件与 runner 结束/重试消费；协议现有测试通过。

## crates/kanzei-llm/src/error.rs

### 职责
保留网络错误因果链，并区分限流、上下文超限、协议及配置错误。

### 判断
PASS

### 确切问题
- 未发现有实际影响的问题。

### 修改
- 无。保留限流类型优先于 token 文案分类的既有规则，避免误触发上下文压缩。

### 影响范围
- client HTTP 错误、三个 SSE 错误解释器、runner 重试；无接口变化。

### 验证
- 检查所有 classify 与 is_context_overflow/is_rate_limited 调用；现有因果链、限流与超限回归通过。

## crates/kanzei-llm/src/sse.rs

### 职责
将任意分片的响应字节流转换为完整 SSE 事件。

### 判断
P1

### 确切问题
- 仅按 LF 切行，合法 CR 流无法分帧；连续 CR 也会被整体剥除而丢失事件边界。
- 流开头的 UTF-8 BOM 被当作字段名的一部分，首个 data/event 字段被忽略。真实 client 路径可返回空文本，即使 HTTP 请求成功。

### 修改
- 以字节扫描 LF/CR，跨 chunk 记住 CR 后待跳过的 LF。
- 只在第一行剥除一个 BOM；整行收齐后解码 UTF-8，保留正文内 BOM。
- 不新增第三方依赖，不改变 SseParser::feed 或 SseEvent 的公开接口。

### 影响范围
- 所有四种协议共用的输入分帧；无持久化格式、重试政策或 UI 改动。

### 验证
- 修复前两项分帧回归失败；枚举所有二段切分位置，并逐字节喂入，覆盖 BOM/UTF-8/CRLF 跨 chunk。
- 保留中文、事件名称、多行正文和第二个事件；CRLF 不重复派发；未完成事件不提前派发；正文 BOM 不被删。
- 失败证据：output/sse-before-20261003.log。

## crates/kanzei-llm/src/protocol/mod.rs

### 职责
按 ProtocolKind 选择请求构造、响应状态机和端点路径。

### 判断
PASS

### 确切问题
- 未发现有实际影响的问题。

### 修改
- 无。保留 DeepSeek 与 OpenAI Responses 的请求方言和路由身份区分，保持已有 finish 边界。

### 影响范围
- Route → body/state/path 的分派；无公开 contract 变化。

### 验证
- 逐个核对四个枚举分支的 body、state、channel、path 配对，协议单测和直接 HTTP caller 回归通过。

## crates/kanzei-llm/src/lib.rs

### 职责
声明协议层模块并导出统一公共接口。

### 判断
PASS

### 确切问题
- 未发现有实际影响的问题。

### 修改
- 无。没有为了整理导出顺序改代码。

### 影响范围
- core、memory、tools、CLI、app；公共类型和再导出保持不变。

### 验证
- 对照 Cargo 依赖与模块定义；全工作区 all-targets Clippy 检查接口与类型兼容。

## crates/kanzei-llm/src/client.rs（调用链切片）

### 职责
把 HTTP 响应分片交给 SSE parser，再交给协议状态机。

### 判断
P1

### 确切问题
- 本文件是上述分帧缺陷的实际可达入口；本轮没有宣称完整审完 HTTP 超时、认证和重试生命周期。

### 修改
- 生产代码不变。新增本地真实 TCP/HTTP 回归，通过 LlmClient 读取带 BOM 的 LF、CRLF、CR 三类响应。

### 影响范围
- 测试模拟端点；不用真实凭据，不调用付费服务。

### 验证
- 修复前 HTTP 回归收到空字符串，断言失败；修复后收到完整中文，StepFinish 恰好一次。
- 失败证据：output/sse-http-before-20261003.log。

# Module Summary

## 已修复
- P0：无。
- P1：SSE 合法 BOM/换行处理错误，造成首条内容或整个事件丢失。
- P2：无。

## PASS 文件
- event.rs、error.rs、protocol/mod.rs、lib.rs，均无生产修改。

## 仍需人工判断
- 无。

## 依赖影响
- 仅 SSE parser 私有状态变化；client 只增加测试，其他 caller 无需补丁。

## 剩余风险
- 未做真实 provider 在线请求；三个需要账号/额度的既有 live probe 保持忽略。
- protocol 下具体状态机、request/proxy/auth 和 client 完整生命周期尚未全部审完；不能将本轮五个完整文件审查等同于 llm 模块完成。

## 下一步顺序

request 类型与构造 → 各协议状态机（工具参数累积、结束和错误）→ proxy/auth → client 生命周期 → core 的流事件消费和持久化。后续每轮继续更新本目录 coverage.json 与依赖地图，PASS 文件不做无意义修改。

## 验证记录

- `cargo test -p kanzei-llm`：60 通过；3 个原有 live probe 忽略；文档测试通过。
- `cargo clippy --workspace --all-targets -- -D warnings`：通过，覆盖所有直接与间接 caller 的类型检查。
- `cargo test -p kanzei --test integration --quiet`：39 通过、0 失败。
- `cargo fmt --all -- --check` 与 `git diff --check`：通过。
- 日志：output/llm-review-tests-20261003.log、output/llm-review-clippy-20261003.log、output/llm-review-integration-20261003.log。
- 本轮没有重复宣称全工作区 2217 项为当前改动的全量测试；当前证据是上述相关测试与全工作区静态检查。
