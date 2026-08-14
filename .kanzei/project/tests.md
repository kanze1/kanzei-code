# Test Runs

## T-1786672324 R-241/D-209 typed session events 关闭门禁 [passed]
- 命令: cargo fmt --all -- --check; cargo test -p kanzei-core store::typed -- --nocapture; cargo test -p kanzei-app conversation::tests::shadow_get_returns_projection_and_comparison_without_switching_source -- --nocapture; cargo test -p kanzei --test always_allow_bash cli_declined_permission_persists_paired_tool_results -- --nocapture; cargo test -p kanzei --test cooperative_halt --test ctrl_c_finalize -- --nocapture; cargo test --workspace; cargo clippy --workspace --all-targets -- -D warnings
- 时长: 关闭前全门禁 93.4s（不含此前定向复跑）
- 摘要: typed/invariant/recovery 11 项、只读 shadow 1 项、真实 CLI 权限拒绝双写 1 项、D-342 停止/Ctrl+C 3 项及全 workspace 全绿；clippy 全 targets 零 warning。覆盖并发 sequence、原子拒绝、750ms 短草稿、legacy 幂等、assistant/tool 崩溃闭合、确定性投影、正常/停止/拒绝/工具错误/多工具部分完成 shadow。
- 关联: R-241 D-209 D-342
- 收尾: 1786672324
