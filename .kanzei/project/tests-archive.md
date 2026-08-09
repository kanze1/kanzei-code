# Test Runs Archive


## T-1786220501 D-200 设置页静态文案 i18n 登记 + 前端冒烟四连 [passed]
- 命令: node scripts/ui-i18n-smoke.mjs; node scripts/ui-runtime-smoke.mjs; node scripts/ui-a11y-smoke.mjs; node scripts/ui-markdown-smoke.mjs
- 摘要: D-200 修复后四条前端冒烟全绿:i18n(33 资源 key/70 HTML 文案/1 动态契约)、runtime(123 invoke 初始化+6 视图切换 0 错误)、a11y、markdown。

## T-1786221328 R-076 防空转硬化与外部阻塞刹车 + 全量验证 [passed]
- 命令: node scripts/ui-runtime-smoke.mjs; node scripts/ui-i18n-smoke.mjs; node scripts/ui-a11y-smoke.mjs; node scripts/ui-markdown-smoke.mjs; cargo test --workspace
- 摘要: R-076 鞭挞状态机:runtime 冒烟新增五组断言(实质进展计数/写日记第一轮推进第二轮刹车/真实改动不误判/外部阻塞刹车/恢复后可继续),171 次 invoke 0 错误;i18n 35 key、a11y、markdown 全绿;cargo workspace 13 crate 全绿(kanzei-app 39 项含 docs_snapshot 阻塞暴露测试)。

## T-1786223647 R-070 refs 来源契约硬校验 + 截断可见 + 记忆页展示 [passed]
- 命令: cargo test -p kanzei-tools memory::; cargo test --workspace; node scripts/ui-i18n-smoke.mjs; node scripts/ui-runtime-smoke.mjs
- 摘要: R-070 refs 来源契约:memory 模块 26 项(含 5 个新测试:refs frontmatter 往返、validate_source_refs 接受/拒绝、add/note 携带 refs、memory_add 硬校验、memory_note 硬校验);workspace 13 crate 全绿;前端 i18n 36 key(新增「引用来源」)、runtime 214 invoke 0 错误。

## T-1786226007 R-100 冗余机械门禁:就地提醒 + 三类计数进入度量 [passed]
- 命令: cargo test -p kanzei-core --lib runner; cargo test --workspace; node scripts/ui-i18n-smoke.mjs; node scripts/ui-runtime-smoke.mjs
- 摘要: R-100 冗余机械门禁:runner 26 项(新增 4 个:重复 git status 就地提醒、全量测试工作树未变提醒、task 引用已知缺陷路径提醒、提醒按类别计数);workspace 13 crate 全绿(kanzei-core 68);前端 i18n 37 key(新增「冗余提醒」)、runtime 214 invoke 0 错误。

## T-1786238330 D-211 侧栏拖拽修复——ui-runtime-smoke [passed]
- 命令: node --check crates/kanzei-app/ui/main.js; node scripts/ui-runtime-smoke.mjs
- 摘要: D-211 修复后冒烟全绿(222 次 invoke):新增"侧栏解锁→锁提示消失→draggable=true→dragstart/dragover/dragend→docs_update reorder"链路断言;反向验证(临时恢复旧限制)断言 2 处立即命中,证明断言真实有效。

## T-1786244824 R-152 本地 License、workspace 与 UI 门禁 [passed]
- 命令: cargo metadata --no-deps --format-version 1; cargo test --workspace; node --check crates/kanzei-app/ui/*.js; node scripts/ui-runtime-smoke.mjs; node scripts/ui-a11y-smoke.mjs; node scripts/ui-i18n-smoke.mjs; node scripts/ui-markdown-smoke.mjs
- 摘要: 6 个 crate license 元数据均为 PolyForm-Noncommercial-1.0.0；workspace 及四条 UI 冒烟全部通过。

## T-1786244948 R-152 verify.ps1 全绿生成 commit 证据 [passed]
- 命令: scripts/verify.ps1
- 摘要: 提交 580f310 上 test、ui_syntax、ui_runtime、ui_a11y、ui_i18n、ui_markdown 全部通过，生成绑定全 SHA 的 dist/verification.json。

## T-1786244998 R-152 package.ps1 commit 漂移拦截 [passed]
- 命令: scripts/package.ps1 -Ack 5
- 摘要: 在 verify 产证后提交 83905c3，重跑 package.ps1 在 cargo build 前因证据绑定旧 commit 而中止。

## T-1786245754 D-218 test_record fixture 项目标记修复回归 [passed]
- 命令: cargo test -p kanzei-tools test_record::tests --lib; cargo test --workspace
- 摘要: 修复 CI 干净 checkout 的项目根 fixture 后，test_record 6 项定向测试与 workspace 全量全部通过。

## T-1786247356 R-152 License metadata 与 verify 脏树门禁 [passed]
- 命令: cargo metadata --no-deps --format-version 1；当前脏树运行 scripts/verify.ps1
- 摘要: 6 个 workspace crate 的 license 全为 PolyForm-Noncommercial-1.0.0；verify 在源码变更未提交时拒绝并报告“工作树不干净，证据无法绑定 commit”。

## T-1786247442 R-152 verify.ps1 全绿生成绑定 commit 证据 [passed]
- 命令: scripts/verify.ps1
- 摘要: 全量 cargo test --workspace、ui/*.js node --check、ui-runtime/a11y/i18n/markdown 四条冒烟全部通过；生成 dist/verification.json，commit=c0ea88db9f89546d69d430065bc0e46da67143af，all_pass=true。

## T-1786248314 R-153 批0a update 测试迁移 [passed]
- 命令: cargo test -p kanzei-app
- 摘要: R-153 批0a（update 测试迁移）：新增 update_tests_update 模块并从 main.rs 移出更新/安装/CLI/服务测试；43 项测试全部通过。批0其余 state/process/conversation/permission 模块尚未迁移。

## T-1786248454 R-153 批0a回归复测（托管文件保护拦截） [failed]
- 命令: cargo test -p kanzei-app
- 摘要: Rust 测试本身 42/42 全部通过；命令结束时仓库托管文件保护检测到测试运行期间触碰 .kanzei/.kanzei/memory 并回滚，工具因此将本次运行标为失败。未产生代码修改。

## T-1786248673 R-153 批0b process 测试迁移回归 [passed]
- 命令: cargo test -p kanzei-app process_tests
- 摘要: process_tests 5 项全部通过；临时目录使用 PID+纳秒唯一值，并在删除目录前显式 drop SQLite store，Windows 并行测试不再发生 error 32。

## T-1786248761 R-153 批0c conversation 测试迁移回归 [passed]
- 命令: cargo test -p kanzei-app conversation_tests
- 摘要: 新增 conversation_tests 模块，5 项会话消息/历史恢复测试全部通过。旧 update_tests 中对应原测试尚待删除，批0仍未完成。

## T-1786248852 R-153 批0d permission 测试迁移回归 [passed]
- 命令: cargo test -p kanzei-app permission_tests
- 摘要: permission_tests 3 项全部通过；新增权限对话框 payload 与 Always Allow 持久化测试模块可编译运行。

## T-1786249009 R-153 批0e state 测试迁移回归 [passed]
- 命令: cargo test -p kanzei-app state_tests
- 摘要: state_tests 10 项全部通过；新增 state 测试模块可编译运行。此前两次编译错误已修正（PathBuf 比较与临时数组借用）。

## T-1786249406 R-153 批0旧测试副本清理回归 [passed]
- 命令: cargo test -p kanzei-app
- 摘要: 批0旧副本清理阶段回归：60 项 kanzei-app Rust 测试全部通过；当前仍存在 update_tests 中 state/process/conversation 的旧副本，重复测试待继续删除。

## T-1786249621 R-153 批0旧测试副本继续清理回归 [passed]
- 命令: cargo test -p kanzei-app
- 摘要: 继续清理旧测试副本后的完整 kanzei-app 回归：53 项通过、0 失败。仍保留 state 与停止收尾旧测试，且 update_tests 有未使用导入警告，批0尚未完成。

## T-1786249805 R-153 批0重复测试隔离回归 [passed]
- 命令: cargo test -p kanzei-app
- 摘要: update_tests 旧副本已用 cfg(any()) 隔离，新的五个测试模块独立运行；42 项全部通过。旧代码块尚未物理删除，后续需做纯文本清理。

## T-1786249904 R-153 批0 state 旧副本物理删除回归 [passed]
- 命令: cargo test -p kanzei-app
- 摘要: 物理删除 state 旧测试中的两项后，kanzei-app 42 项全部通过。剩余旧 state 函数仍被 cfg(any()) 隔离，尚需继续物理删除。

## T-1786250031 R-153 批0继续删除 state 旧测试回归 [passed]
- 命令: cargo test -p kanzei-app
- 摘要: 继续物理删除一个 state 旧测试函数后，kanzei-app 42 项全部通过。剩余 state 旧副本与 update_tests 废弃模块仍待清理。

## T-1786250180 R-153 批0删除 defect_review 旧测试回归 [passed]
- 命令: cargo test -p kanzei-app
- 摘要: 物理删除 defect_review_snapshot 旧测试后，kanzei-app 42 项全部通过。剩余 4 项 state 旧测试与 2 项 process 停止旧测试待删除。

## T-1786250305 R-153 批0删除 defect_review 空报告旧测试回归 [passed]
- 命令: cargo test -p kanzei-app
- 摘要: 物理删除 defect_review_rejects_empty_model_report 旧测试后，kanzei-app 42 项全部通过。剩余 defect_review 空状态、docs_snapshot、export 及 process 停止旧测试。

## T-1786250439 R-153 批0删除 defect_review 空状态旧测试回归 [passed]
- 命令: cargo test -p kanzei-app
- 摘要: 物理删除 defect_review_empty_state_returns_without_model_call 旧测试后，kanzei-app 42 项全部通过。剩余 docs_snapshot、export 与 process 停止旧测试。

## T-1786250751 R-153 批0删除 export 旧测试回归 [passed]
- 命令: cargo test -p kanzei-app
- 摘要: 物理删除 export_project_data 旧测试后，kanzei-app 42 项全部通过。剩余 process 停止收尾旧测试与废弃 update_tests 模块。

## T-1786250897 R-153 批0删除首个 process 停止旧测试回归 [passed]
- 命令: cargo test -p kanzei-app
- 摘要: 物理删除 stopping_after_promote 旧测试后，kanzei-app 42 项全部通过。剩余 process 停止轨迹旧测试与废弃 update_tests 模块。

## T-1786251056 R-153 批0最终旧测试清理回归 [passed]
- 命令: cargo test -p kanzei-app
- 摘要: 批0最终旧测试清理回归：删除最后 process 停止旧测试及整个废弃 update_tests 模块后，42 项全部通过。

## T-1786251226 R-153 批1 agent_container 与 fast_model 拆解回归 [failed]
- 命令: cargo test -p kanzei-app
- 摘要: 批1尝试仅通过 pub use 将 tauri command 暴露到新模块失败：tauri 命令宏生成的辅助符号仍定义在 main 模块，导致重复定义与模块内找不到宏符号。尚未提交，需回退这次错误尝试后按完整函数迁移实施。

## T-1786251667 R-153 批1 kanzei-app 定向测试 [failed]
- 命令: cargo test -p kanzei-app
- 摘要: 编译失败：update_tests_update.rs 仍从 super 根导入已迁移至 fast_model 模块的 ollama_service_up/pull_progress_text；已记录 D-221，修复后重跑。

## T-1786251753 R-153 批1 kanzei-app 定向测试（修复后） [passed]
- 命令: cargo test -p kanzei-app
- 摘要: 42 项测试全部通过；覆盖模块编译、Tauri command 宏注册、原 update/permission/conversation/process/state/settings/assembly 测试。仅有既存 kanzei-core/tools 警告，无失败。

## T-1786252124 R-153 批2 update 模块边界回归 [failed]
- 命令: cargo test -p kanzei-app
- 摘要: 批2暂存模块入口方案失败：update.rs command wrapper 与 main.rs 原 tauri command 宏生成同名符号冲突；未提交，需改为完整剪切函数而非 wrapper。

## T-1786252264 R-153 批2 update 模块边界回归（修复后） [passed]
- 命令: cargo test -p kanzei-app
- 摘要: 42 项全部通过；修复 D-222 后 update command wrapper 的宏名冲突消失。注意：本次仅验证模块入口/转发边界，update 实现仍待完整剪切，不能作为 R-153 批2完成证据。

## T-1786252367 R-153 批2 update command 宏迁移回归 [passed]
- 命令: cargo test -p kanzei-app
- 摘要: 42 项全部通过；update_check/update_install 的 tauri command 宏已由 update.rs 承接并通过模块全路径注册。旧实现已改名为 impl 供转发，完整函数剪切仍未完成。

## T-1786252610 R-153 批2版本判断 helper 迁移回归 [passed]
- 命令: cargo test -p kanzei-app
- 摘要: 42 项全部通过；版本判断 helper 已从 main.rs 移入 update.rs，测试兼容导出有效。随后移除未使用的 timestamp_digits 根导出，待提交前复跑定向测试。

## T-1786252673 R-153 批2版本判断 helper 迁移回归（提交前） [passed]
- 命令: cargo test -p kanzei-app
- 摘要: 42 项全部通过；去除未使用导出后无 kanzei-app 新增警告，仅保留既有 kanzei-core/tools 警告。

## T-1786252959 R-153 批2更新 command 完整迁移回归 [passed]
- 命令: cargo test -p kanzei-app
- 摘要: 42 项全部通过；update_check/update_install 实现已移入 update.rs，命令仍由真实 invoke_handler 调用；仅保留既有 core/tools 警告。

## T-1786253319 R-153 批2 update 基础 helper 完整迁移回归 [passed]
- 命令: cargo test -p kanzei-app
- 摘要: update 基础 helper 迁移回归通过：路径、安装包校验、残留清理、日志、镜像指纹/替换判断均由 update.rs 提供，main.rs 测试兼容导出保持有效。