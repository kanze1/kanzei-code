## 1.4a Rust/Cargo 工程的提交门禁与 clippy 分工

(本节只在存在 `crates/kanzei-app/Cargo.toml` 的 Kanzei 自举仓库注入。)

- **提交前代码门禁(D-264)**:提交 Rust 源码前,`compile_gate` / `fmt_gate` / `clippy_gate`(kanzei-tools/src/git.rs)会由结构化 git 工具**代码强制**跑一遍 `cargo check --workspace --all-targets`、`cargo fmt --all -- --check`、`cargo clippy --workspace -- -D warnings`,任一不过即拦下提交并点名违规文件。这条规则在 2026-08-11/12 被自举漏掉三次才确认必须强制,不要再尝试用 bash 绕开。注意 clippy 这步**不含测试目标**:测试代码能否编译由 `check --all-targets` 兜住,测试代码的 lint(`#[cfg(test)]` 模块、`tests/` 集成测试)提交门禁看不到。
- **clippy 本机分工**:提交门禁与 `scripts/verify.ps1` 跑轻量 `cargo clippy --workspace -- -D warnings`(不含测试目标);并行线收活门禁与合并后门禁(kanzei-app)跑 `cargo clippy --workspace --all-targets --quiet`,不带 `-D warnings`,只有 deny 级 lint 会失败。因此测试代码的 warn 级 lint 本地提交与 verify 都拦不住——**改到测试代码时自跑 `cargo clippy -p <crate> --all-targets -- -D warnings`**,不要把「提交成功」当成测试代码 lint 已过(D-758)。改任一处 clippy 命令须同步守护测试 `gate_checklists_align_across_git_and_local_verify`(kanzei-tools/src/git.rs)。
- **验证运行位置**:本仓库按用户要求关闭 GitHub Actions CI。开发和发版均在本机运行门禁，最终干净提交必须通过 `scripts/verify.ps1 -Full`；不要添加、启用或运行远端 CI。
