# Test Runs

## T-1786248822 R-153 批0d permission 测试迁移回归 [running]
- 命令: cargo test -p kanzei-app permission_tests
- 摘要: 正在验证新增 permission_tests 模块。

## T-1786248951 R-153 批0e state 测试迁移回归 [running]
- 命令: cargo test -p kanzei-app state_tests
- 摘要: 正在验证新增 state_tests 模块。

## T-1786249114 R-153 批0旧测试副本清理回归 [running]
- 命令: cargo test -p kanzei-app
- 摘要: 正在验证批0旧测试副本清理后的 kanzei-app 全量单测。

## T-1786249557 R-153 批0旧测试副本继续清理回归 [running]
- 命令: cargo test -p kanzei-app
- 摘要: 正在验证删除 state/process/conversation/permission 旧测试副本后的 kanzei-app 测试。

## T-1786249737 R-153 批0重复测试隔离回归 [running]
- 命令: cargo test -p kanzei-app
- 摘要: 正在验证 update_tests 旧副本禁用后，仅新五个测试模块参与的 kanzei-app 回归。

## T-1786249861 R-153 批0 state 旧副本物理删除回归 [running]
- 命令: cargo test -p kanzei-app
- 摘要: 正在验证继续物理删除 state 旧测试函数后的 kanzei-app 回归。

## T-1786249984 R-153 批0继续删除 state 旧测试回归 [running]
- 命令: cargo test -p kanzei-app
- 摘要: 正在验证继续物理删除 state 旧测试后的 kanzei-app 回归。

## T-1786250102 R-153 批0删除 defect_review 旧测试回归 [running]
- 命令: cargo test -p kanzei-app
- 摘要: 正在验证物理删除 defect_review_snapshot 旧测试后的 kanzei-app 回归。

## T-1786250243 R-153 批0删除 defect_review 空报告旧测试回归 [running]
- 命令: cargo test -p kanzei-app
- 摘要: 正在验证物理删除 defect_review_rejects_empty_model_report 旧测试后的 kanzei-app 回归。

## T-1786250389 R-153 批0删除 defect_review 空状态旧测试回归 [running]
- 命令: cargo test -p kanzei-app
- 摘要: 正在验证物理删除 defect_review_empty_state_returns_without_model_call 旧测试后的 kanzei-app 回归。

## T-1786250504 R-153 批0删除 docs_snapshot 旧测试回归 [running]
- 命令: cargo test -p kanzei-app
- 摘要: 正在验证物理删除 docs_snapshot 旧测试后的 kanzei-app 回归。

## T-1786250694 R-153 批0删除 export 旧测试回归 [running]
- 命令: cargo test -p kanzei-app
- 摘要: 正在验证物理删除 export_project_data 旧测试后的 kanzei-app 回归。

## T-1786250847 R-153 批0删除首个 process 停止旧测试回归 [running]
- 命令: cargo test -p kanzei-app
- 摘要: 正在验证物理删除 stopping_after_promote 旧测试后的 kanzei-app 回归。

## T-1786250999 R-153 批0最终旧测试清理回归 [running]
- 命令: cargo test -p kanzei-app
- 摘要: 正在验证删除最后一个 process 停止旧测试及废弃 update_tests 模块后的批0回归。

## T-1786251209 R-153 批1 agent_container 与 fast_model 拆解回归 [running]
- 命令: cargo test -p kanzei-app
- 摘要: 正在验证 R-153 批1 agent_container/fast_model 域模块注册与现有行为回归。

## T-1786251634 R-153 批1 kanzei-app 定向测试 [running]
- 命令: cargo test -p kanzei-app
- 摘要: 验证 agent_container.rs 与 fast_model.rs 的完整 command 搬迁、宏注册及测试编译。

## T-1786251670 R-153 批1 kanzei-app 定向测试（修复后） [running]
- 命令: cargo test -p kanzei-app
- 摘要: D-221 已修复：update 测试改从 fast_model 模块导入辅助函数，并将跨测试模块辅助提升为 pub(crate)。

## T-1786252111 R-153 批2 update 模块边界回归 [running]
- 命令: cargo test -p kanzei-app
- 摘要: 验证批2新增 update 模块入口、启动调用和 command 全路径注册不破坏现有行为；当前实现仍保留旧函数体作为兼容转发目标。

## T-1786252149 R-153 批2 update 模块边界回归（修复后） [running]
- 命令: cargo test -p kanzei-app
- 摘要: D-222 修复：wrapper 改为 update_check_command/update_install_command 并用 tauri command rename 保持外部命令名；验证宏符号冲突消失。

## T-1786252297 R-153 批2 update command 宏迁移回归 [running]
- 命令: cargo test -p kanzei-app
- 摘要: 验证 update command 宏已从 main.rs 移除、模块 command wrapper 调用改名后的实现，避免重复宏符号。

## T-1786252559 R-153 批2版本判断 helper 迁移回归 [running]
- 命令: cargo test -p kanzei-app
- 摘要: 验证版本判断 helpers 已从 main.rs 迁移到 update.rs，测试兼容导出与 update command 入口保持行为。

## T-1786252614 R-153 批2版本判断 helper 迁移回归（提交前） [running]
- 命令: cargo test -p kanzei-app
- 摘要: 移除未使用的 timestamp_digits 根导出后，提交前复跑 kanzei-app 定向测试并确认无新增警告。

## T-1786252916 R-153 批2更新 command 完整迁移回归 [running]
- 命令: cargo test -p kanzei-app
- 摘要: 验证 update_check/update_install 生产实现已从 main.rs 完整剪切到 update.rs，保留既有 helper 调用与 command 名称。

## T-1786253308 R-153 批2 update 基础 helper 完整迁移回归 [running]
- 命令: cargo test -p kanzei-app
- 摘要: 验证 update 域路径、安装包校验、残留清理、日志和镜像判定 helpers 完整迁移到 update.rs，兼容 main.rs 既有测试导出。

## T-1786253446 R-153 批2 update 启动与 helper 迁移回归 [running]
- 命令: cargo test -p kanzei-app
- 摘要: 验证 update.rs 已承接启动接棒、WebView 清理、安装 helper、进程探测、CLI 同步和 pending 替换实现；当前 main.rs 旧实现尚待物理删除。

## T-1786253682 R-153 批2 update 旧副本清理回归 [running]
- 命令: cargo test -p kanzei-app
- 摘要: 验证 main.rs 已删除 update 启动/helper/CLI/pending 旧实现，测试通过 main.rs 的兼容导出访问 update 模块实现。

## T-1786253732 R-153 批2旧副本删除提交后回归 [running]
- 命令: cargo test -p kanzei-app
- 摘要: 验证 R-153 批2旧副本物理删除后的编译与测试行为；同时确认 main.rs 只保留 update 模块调用，旧 update 函数定义已无匹配。

## T-1786254023 R-153 批3 memory 模块接入回归 [running]
- 命令: cargo test -p kanzei-app
- 摘要: 验证批3 memory.rs 接入：13 个 memory command 经模块全路径注册，run_task 轮末整理改由 memory::consolidate_memory_inbox 调用；当前 main.rs 旧 memory 副本尚待物理删除。

## T-1786254227 R-153 批3 memory 旧副本清理回归 [running]
- 命令: cargo test -p kanzei-app
- 摘要: 验证批3 memory 旧副本物理删除：main.rs 保留 run_metrics，memory command 与 consolidate 仅由 memory.rs 提供，invoke_handler 和 run_task 调用保持真实。

## T-1786254656 R-153 批4 state 模块接入回归 [running]
- 命令: cargo test -p kanzei-app
- 摘要: 验证批4 state.rs 接入与 main.rs state 旧副本清理：AppState/运行时/UI probe/跨域辅助由 state 模块提供，main 保留 setup 与 invoke_handler 装配。

## T-1786255061 R-153 批5 prefs/projects 模块迁移回归 [running]
- 命令: cargo test -p kanzei-app
- 摘要: 验证批5 prefs/projects 模块接入：项目 command 全路径注册，AppPrefs 持久化与项目隔离逻辑迁移，workspace_snapshot 改用 projects 模块消费者。

## T-1786255398 R-153 批6 processes/mobile 模块接入回归 [running]
- 命令: cargo test -p kanzei-app
- 摘要: 验证批6 processes/mobile 模块接入：8 个 process/worktree command 与 2 个 mobile command 切换至模块全路径，mobile HTTP bridge 真实线程消费者保留。

## T-1786273837 R-158 Codex Fast mode 定向 Rust 测试 [running]
- 命令: cargo test -p kanzei-llm -p kanzei-core -p kanzei-app
- 摘要: 验证 Codex Fast mode 的请求字段、Runner 透传、桌面设置配置与既有构造点是否完整编译。

## T-1786274446 R-158 Luna 默认 Fast mode 编译检查 [running]
- 命令: cargo check -p kanzei-harness -p kanzei-llm -p kanzei-core
- 摘要: 验证 Luna 默认模型与 Codex Fast mode 的可选配置、merge/fill_defaults、请求协议和 Runner 透传。

## T-1786275923 R-153 批6 kanzei-app 定向测试 [running]
- 命令: cargo test -p kanzei-app
- 摘要: 批6代码搬迁后开始定向 Rust 测试；按 M-022 使用 test_record 作为唯一测试记录通道。

## T-1786276079 R-153 批6 kanzei-app 定向测试（原样搬迁修正后） [running]
- 命令: cargo test -p kanzei-app
- 摘要: 修正批6模块内容为从 main.rs 原样搬迁后的等价实现，重新执行定向验证记录。

## T-1786277657 R-153 批7 docs 域搬迁定向测试 [running]
- 命令: cargo test -p kanzei-app
- 摘要: 批7文档域搬迁阶段：docs.rs 已承接 docs_snapshot/docs_update/docs_open/docs_read，invoke_handler 已改用模块路径；settings 域仍待本批完成。

## T-1786277730 R-153 批7 settings 搬迁前基线定向测试 [running]
- 命令: cargo test -p kanzei-app
- 摘要: 继续 R-153 批7前置回归：确认已提交的 docs 域拆解在 settings 域搬迁前仍保持可验证基线。

## T-1786277788 R-153 批7 settings command 边界定向测试 [running]
- 命令: cargo test -p kanzei-app
- 摘要: 批7 settings command 边界接入：新增 settings.rs 作为真实 Tauri command consumer，invoke_handler 切换为 settings:: 全路径；底层行为暂沿用 main.rs 实现。

## T-1786277880 R-153 settings_get 物理搬迁定向测试 [running]
- 命令: cargo test -p kanzei-app
- 摘要: settings_get 已物理搬入 settings.rs，其他 settings command 暂保留委托边界；本阶段验证注册路径与设置读取实现。

## T-1786277939 R-153 permission settings 物理搬迁定向测试 [running]
- 命令: cargo test -p kanzei-app
- 摘要: project_permission_config、permission_rules_get、permission_rule_delete 已物理搬入 settings.rs，并继续由 settings:: command 注册。
