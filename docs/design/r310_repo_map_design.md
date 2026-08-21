# Repo Map Design for R-310 Batch 3

- 身份: validated_design
- 最近核验依据: 8ed3256f

**Date:** 2026-08-20
**Status:** Accepted for R-310 B3; implementation landed in `crates/kanzei-tools/src/symbols.rs`.

## 目标与边界

为 Rust workspace 提供可机读的 `crate → module → public symbol` 查询，减少模型为了定位代码而反复试错。该能力只做结构地图，不做 embedding、语义检索，也不改变既有 `grep`/`glob` 语义。

## 方案对比与 token 成本

| 方案 | 形态 | 成本核算 | 决策 |
|---|---|---|---|
| A 全量注入 | 每轮把全 workspace 符号表放入上下文 | 约为全量输出文本的 `ceil(UTF-8 字节数 / 4)` token；当前 workspace 的 public 符号规模会随仓库增长，且每轮重复支付 | 拒绝：固定上下文成本高、与按需查询重复 |
| B 实时按需查询 | `symbols` 接收 `crate`/`module`，只返回目标模块的 public symbol | 上下文注入成本为 0；每次只支付工具请求与目标模块返回文本，代理估算为 `请求约 12 token + ceil(返回字节数 / 4)` | **采用** |
| C 持久增量索引 | 提交时维护静态索引文件 | 查询成本低，但新增索引写者、失效与提交同步门；索引维护成本高于本条收益 | 不采用；实时扫描天然随提交更新 |

成本口径是可复算的 UTF-8 字节/4 代理，不把工具响应冒充永久上下文注入。B 方案对高频小模块只支付局部响应，超出预算时继续缩小 `module` 查询范围。

## 实现契约

- `symbols` 新增 `crate`（Cargo package name 的 `-` 转 `_`）与 `module`（模块路径前缀）参数。
- 地图模式只输出 public symbol，报告固定包含 crate、module、文件相对路径、kind、name、line，便于机器和模型同时读取。
- 每次查询重新读取 workspace `Cargo.toml`、crate `Cargo.toml` 与 `.rs` 文件，不落静态缓存；因此提交、分支切换或文件修改后的下一次查询自动获得最新结果。
- 合法的单行或多行 `workspace.members` 都可解析；未知 crate 返回 `SYMBOLS_CRATE_NOT_FOUND` 并列出可用 crate。
- 未带 `crate`/`module` 的既有 `path`、`filter`、`public_only`、`callers`、`define` 路径保持原语义。

## 定向验证

- `crates/kanzei-tools/src/symbols.rs:17-43`：输入契约。
- `crates/kanzei-tools/src/symbols.rs:83-151`：crate 选择、错误提示与地图查询分流。
- `crates/kanzei-tools/src/symbols.rs:275-371`：实时分层报告与模块路径生成。
- `crates/kanzei-tools/src/symbols.rs:512-582`：workspace crate 目录解析，覆盖单行/多行 members。
- `crates/kanzei-tools/src/symbols.rs:1160-1221`：crate/module/public 查询与文件变更后的增量反映测试。
- `T-1786922726645`：`cargo fmt --all -- --check; cargo test -p kanzei-tools symbols`，13 passed、0 failed。

## B4 真实运行数据

同口径为每个 run 的 `failure_count / calls`，失败分类来自 B1 统一遥测：

| 阶段 | run artifact | calls | failures | 失手率 |
|---|---|---:|---:|---:|
| B1 基线（导航试错轨迹） | `.kanzei/artifacts/tool-failures/run_1787269956737526500.json` | 32 | 24 | 75.00% |
| B4 当前自举档复测（本运行） | `.kanzei/artifacts/tool-failures/run_1787270303537435000.json` | 57 | 8 | 14.04% |

当前复测相对基线下降 `60.96` 个百分点（相对下降约 `81.28%`）。该数据是本次真实工具运行的遥测，不是静态文案或替身服务；它覆盖了按需地图设计落地后的导航操作，但仍是单个自举档 run，后续可由 R-311/R-312 继续积累跨 run 样本。验证记录：`T-1786922726646`。
