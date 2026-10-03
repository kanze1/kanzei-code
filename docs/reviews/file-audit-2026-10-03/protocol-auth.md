# 协议、代理、凭据与历史审查

基线：6613f7fe。按依赖顺序继续；只记录读过的文件。

官方依据：[Anthropic thinking 协议](https://platform.claude.com/docs/en/docs/build-with-claude/extended-thinking)：工具回合须保留完整 thinking/redacted_thinking 块。历史依据：R-137（推理回放）、D-422（缺 item.done 网关）、D-061（凭据并发）。

## crates/kanzei-llm/src/protocol/anthropic.rs

### 职责
Anthropic 请求编码、流块状态与原样历史回放。

### 判断
P1

### 确切问题
- redacted_thinking 被当成空 thinking，data 丢失；下一次工具回合无法原样回传。

### 修改
- 独立保存 opaque 块，沿现有 Hosted 历史通道回放；不暴露为可读推理。

### 影响范围
- client → runner；复用现有序列化字段，无 schema 变更。

### 验证
- 隐藏块往返回归；原有 thinking、工具、引用与错误测试。

## crates/kanzei-llm/src/protocol/openai_responses.rs

### 职责
Responses 流事件到统一事件的转换。

### 判断
P1

### 确切问题
- D-422 已支持缺 output_item.done 的网关，但只结算工具，遗漏推理结束，runner 不会将该推理写入历史。

### 修改
- 按 output_index 维护未结束推理，终态按索引结算待处理内容与工具，从终态 output 取签名。

### 影响范围
- OpenAI/DeepSeek Responses → runner；公共类型不变。

### 验证
- 缺 item.done 的推理→工具→终态回归；既有 D-422、错误分类、签名与 hosted 测试。

## crates/kanzei-llm/src/protocol/deepseek_responses.rs

### 职责
DeepSeek Responses 请求历史和工具协议编码。

### 判断
PASS

### 确切问题
- 未发现有实际影响的问题。

### 修改
- 无。

### 影响范围
- protocol/mod → client；图文入口已由 client 拒绝，不改变协议。

### 验证
- 既有请求体、明文推理与不支持字段测试。

## crates/kanzei-llm/src/proxy.rs

### 职责
统一模型、网络工具与更新服务的 HTTP 代理选择。

### 判断
P1

### 确切问题
- Windows Env 分支将 NO_PROXY 产生的 None 当成未配置，继续启用系统代理，直连要求失效。

### 修改
- 在系统代理回退之前统一执行 bypass 判断。

### 影响范围
- client、auth、webfetch、websearch、app/models/settings/update；API 不变。

### 验证
- 注入系统回退验证通配、后缀、端口直连，以及环境代理优先级。

## crates/kanzei-llm/src/auth/store.rs

### 职责
凭据原子写回与磁盘版本采纳。

### 判断
P1

### 确切问题
- 内部并发提交的重读与替换不在同一互斥区。

### 修改
- 同一 FileLock 包住重读与提交，兼容外层同线程重入。

### 影响范围
- 唯一生产 caller auth/codex；外部 CLI 不遵守锁协议。

### 验证
- 存储回环、损坏旧 JSON 恢复、并发刷新路径。

## crates/kanzei-llm/src/auth/codex.rs

### 职责
订阅凭据读取、过期刷新与请求头生成。

### 判断
P1

### 确切问题
- 内部并发请求可同时刷新；用刷新完成时间比较会覆盖网络等待期间的外部新登录；缺 access_token 的 HTTP 成功响应仍推进 last_refresh。

### 修改
- 线程拥有的 OS 锁覆盖读取→刷新→提交；放在 blocking worker，避免跨 async 迁移持锁；提交比较刷新前快照；无 access_token 保持原快照并报错。

### 影响范围
- client 的路由组装方；保留 auth.json 格式，未读取或修改真实用户凭据。

### 验证
- 本地 HTTP 两调用刷新、外部登录插入、旧令牌及时间保持不变的回归。

## crates/kanzei-llm/src/auth/mod.rs

### 职责
导出凭据模块。

### 判断
PASS

### 确切问题
- 未发现有实际影响的问题。

### 修改
- 无。

### 影响范围
- 公开模块路径不变。

### 验证
- 调用方检查与工作区构建。

## crates/kanzei-llm/src/client.rs

### 职责
路由、HTTP 重试、SSE 消费和完成标记检查。

### 判断
PASS

### 确切问题
- 未发现新增有实际影响的问题；上一批 EOF 修复保留。

### 修改
- 无新增代码修改。

### 影响范围
- core、memory 与入口共享，重试只发生在安全边界。

### 验证
- 既有四协议 HTTP 完成矩阵、重试、BOM 与分帧测试。

## crates/kanzei-core/src/history.rs

### 职责
清除孤儿工具记录并保留其余消息顺序。

### 判断
PASS

### 确切问题
- 未发现有实际影响的问题。

### 修改
- 无。

### 影响范围
- runner 与会话恢复；Hosted 数据保持原样。

### 验证
- 孤儿/重复 ID 配对测试；新增隐藏块经历史过滤与 JSON 往返集成测试。

## crates/kanzei/tests/integration/incomplete_stream.rs

### 职责
真实 HTTP 到 runner 的终态与历史契约回归。

### 判断
PASS

### 确切问题
- 未发现有实际影响的问题。

### 修改
- 补隐藏推理保存、JSON 往返和零伪工具事件断言。

### 影响范围
- 测试范围，无生产兼容性变化。

### 验证
- CLI 集成测试。

## crates/kanzei-core/src/runner/drive.rs（调用链切片）

### 职责
消费协议事件、构造本轮历史与工具执行计划。

### 判断
P1

### 确切问题
- 原 HostedItem 分支把所有 opaque 类型都展示成 web_search；接入隐藏推理后会产生伪工具事件。

### 修改
- redacted_thinking 仅入历史，不产生 HostedTool UI 事件。

### 影响范围
- RunEvent 消费方与会话历史；公共 API 不变。

### 验证
- 真实 HTTP→runner→历史过滤→JSON 往返，断言隐藏块保留且工具事件为零。

# Module Summary

## 已修复
- P0: 无。
- P1: 隐藏推理丢失、缺块终态推理丢失、NO_PROXY 失效、凭据并发及错误响应写回。
- P2: 无。

## PASS 文件
- deepseek_responses.rs、auth/mod.rs、client.rs、core/history.rs、集成测试文件。

## 仍需人工判断
- 无。

## 依赖影响
- 内部凭据刷新改用现有 OS 锁；协议 opaque 状态沿现有 Hosted 序列化通道保存，没有数据库迁移。

## 剩余风险
- 官方 CLI 不参与内部锁协议，最后一次磁盘版本检查与 rename 之间的外部写入不能由内部锁排除。
- 未连真实供应商跑付费请求；用本地 HTTP 和历史兼容夹具验证。
- 这里只完成列出的全文/切片，不代表整个 core、memory、tools 或前端完成。
