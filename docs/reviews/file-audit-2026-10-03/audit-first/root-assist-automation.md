# Root assist — automation

基线 `55eaca24750ac431ebe7f577c2f4b7b8cbded6f5`。仅审计，无产品变更，无 Cargo/真实 Git 测试执行。

依赖：core SessionStore/MemoryCoordinator → tools worktree primitives → app registry/lifecycle/workspace/gate → 本测试文件。全文 1–2464 行已读；未将只看测试名称或搜索算作全文覆盖。

## crates/kanzei-app/src/worktree_tests.rs

### 职责
验证工作树与对话绑定、跨进程竞争、回滚、合并及关线。

### 判断
PASS

### 确切问题
- 未发现有实际影响的问题

### 修改
- 未改，审计阶段；先统一根因与相邻 caller 合同，再进入修复。

### 影响范围
- process lifecycle/workspace/gate/tools worktree。当前未改变 API、持久格式或兼容性。

### 验证
- 全文（含现有测试）审阅；核对真实Git夹具、CAS凭据、同名竞争、数据库注册失败、主根身份、历史保留与资源清理。 本轮未执行此文件相关完整测试，不将静态 PASS 写成运行验收通过。

# Module Summary

## 已修复
- P0: 0
- P1: 0
- P2: 0

## PASS 文件
- `crates/kanzei-app/src/worktree_tests.rs`

## 仍需人工判断
- 无。

## 依赖影响
- 未改变 contract；核对一树一线、ID退休、主根身份、失败回滚不删除他人分支、跨进程凭据和运行中资源边界。

## 剩余风险
- 本轮仅静态审阅测试及必要 caller，没有重跑跨进程 Git/Windows 句柄/真实 gate 测试。
