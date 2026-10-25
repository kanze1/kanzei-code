# Contributing

工程规则见 [AGENTS](AGENTS.md)，架构与状态所有权见[系统合同](docs/design/system_contracts.md)。使用 `kanzei/` 分支组织可评审改动，保留无关工作树变化，只暂存本次明确修改的文件。

## 开发与验证

安装前端开发依赖使用 `npm ci`。生产栈为 Rust、Tauri 与原生 ESM；React 仅用于浏览器输入兼容测试夹具。

```powershell
./scripts/dev.ps1 check --workspace
./scripts/dev.ps1 test --workspace
./scripts/verify.ps1
```

`verify.ps1` 按当前变更和 `scripts/verification-checks.json` 选择检查，记录计划与各项结果；成功回执在 `dist/verification-targeted.json`。检查只在本地执行，不新增、启用或运行 GitHub Actions CI。

每个独立调度范围对应一个检查入口，小用例放入该领域的测试模块，不嵌套进无关 smoke。注册表必须使子模块改动选择其所属检查，包括副作用导入。

环境变更测试使用带完成回执的子进程；写者互斥锁不能隔离读者。外部前提放入注明原因的 ignored 测试，显式执行时缺前提必须失败：

```powershell
cargo test -p <crate> <test-filter> -- --ignored
```

原生桌面、真实后端、真实模型和模拟 IPC 分开留证；passed、ignored、未执行和失败分别报告。Rust 变更按实际调用范围执行 check、fmt、clippy 和测试；修复必须复现原问题并验证恢复的不变量。

结构化 Git 工具提交 Rust 源码时执行 `cargo check --workspace --all-targets`、`cargo fmt --all -- --check` 和 `cargo clippy --workspace -- -D warnings`。提交门禁和 `verify.ps1` 的 clippy 不包含测试目标；修改测试代码时，另运行 `cargo clippy -p <crate> --all-targets -- -D warnings`。相关命令由 `gate_checklists_align_across_git_and_local_verify` 守护，提交成功不代表测试代码的 lint 已通过。

本仓库的前端样式可能同时存在基础定义和响应式覆盖，修改时结合相关样式表与 `@media` 上下文理解影响范围。缺失或多余的花括号可能改变整段规则的生效范围；静态结构检查与实际界面观察分别提供源码和渲染证据，按改动选择所需验证。

## 发布

发布前在最终干净提交执行：

```powershell
./scripts/verify.ps1 -Full
./scripts/package.ps1 -Ack <脚本统计的实际提交数> -Publish
```

全量成功回执为 `dist/verification.json`，必须匹配最终提交。交付代码、公开 Release 和本地安装是三个结果，分别核对。安装版运行时可写入 `.pending`，下次启动接力替换；退出码 0 不能代替运行版本确认。

日常开发在 dev 推进，main 通过允许的前向合并接收发布状态；需要隔离时使用临时干净 worktree。保留公开提交、标签和分支历史，不以重置共享分支或删除历史引用做清理。

构建、安装器和运行回执不入库，生命周期由 `scripts/artifact-lifecycle.mjs` 管理。度量机器基线为 `scripts/metrics-baseline.json`，不再维护 Markdown 榜单副本。发布结果与验收登记到现有交付/测试台账及机器回执，不为每次运行新增说明文档。

## 文档

人工维护范围由[项目入口](README.md)列出。日常修改写回已有手册或系统合同；管理方案实施后把已成立的合同并入系统合同，剩余设计仍在同一份方案维护。

需求、缺陷、测试和决策使用对应领域工具维护，设计身份使用 architecture 工具维护。旧资料已退出当前文档集，恢复说明见项目入口；原始历史与未完成验收不会因精简被关闭或改写。
