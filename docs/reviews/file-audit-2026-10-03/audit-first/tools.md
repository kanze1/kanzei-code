# 工具与代码操作：先审后改审计记录

基线 `55eaca24750ac431ebe7f577c2f4b7b8cbded6f5`。本人全文审计 50 文件；原分组中的架构规范 6 文件已交 root/其他审计者，未计为本人覆盖。未修改产品代码、未提交、未运行共享 Cargo。
本报告以该提交作为追溯基线；结束时 read.rs / symbols.rs 已含另一用户任务的缺失路径结果分类改动，已补读 diff。JSON 的 source_sha256_lf 对应结束时实际审阅内容，包含这两份并行 delta；其余 48 文件与基线 LF 内容一致。未声称共享 checkout 干净。


依赖顺序：kanzei-base（锁/原子写/hash/journal）→ tools 的快照、校验、路径与进程原语 → Git/文件/检索/浏览器工具 → CLI/app/core caller。源文件属于 ctx.cwd；项目状态与配置属于 ctx.project_root；Git index/HEAD 属于实际目标工作树；浏览器 page 的实际 owner 目前未按会话隔离。

本轮新建文件只在本报告目录及 output/audit-first/tools/。所有探针是自建 fixture；没有访问或改动问题.MD。

## 确认问题

### AF-T01 P1 — crates/kanzei/src/cli/quarantine.rs
- 触发：执行 kz quarantine --dry-run --apply --type bg；参数顺序与 --apply --dry-run 相反。
- 影响：互斥参数仅单向检查，dry-run 命令实际进入 apply 并删除隔离恢复材料。
- 证据：真实 target/release/kz.exe 在隔离 bg-100 目录输出 mode: apply、removed_dirs: 1、freed_bytes: 10；随后目录不存在。清理审计日志保留在 output/audit-first/tools/quarantine-probe/.kanzei/quarantine/cleanup-log.jsonl。
- 验证：当前 parse_args 原函数逐字提取并 rustc 编译：dry-run/apply 返回 apply=true，反序返回 Err，证据 contract-evidence.json/quarantine_parser_probe。现有 release CLI 只作删除症状补证，未建立其二进制提交来源；仅操作 output 下自建 fixture。

### AF-T02 P0 — crates/kanzei-tools/src/git.rs
- 触发：stage/commit_plan/finalize/test_record 对约 5000 个源文件计算 endorsement fingerprint。
- 影响：父进程写满 git stdin，git 写满 stdout；父进程等写完才 wait_with_output 排空 stdout，形成永久相互等待，命令无法完成。
- 证据：output/audit-first/tools/git-hash-evidence.json：原函数 all 与 paths 两种路径都超过 5 秒看门狗；相同 5000 路径以并行排空管道的控制组返回全部 5000 个 hash。探针逐字提取两个生产函数，rustc 编译，不跑 Cargo。
- 验证：std-only 原函数隔离进程 + 同规模并行读控制组；看门狗只清理探针进程树。plan.rs:104、finalize.rs:125、test_record/execution.rs:52 都同步直接调用；函数内没有超时，外层 async 取消不能抢占阻塞的 std::io::Write，符合用户永久 deadlock 的 P0 口径。

### AF-T03 P1 — crates/kanzei-tools/src/cross_tree.rs
- 触发：A 线 bash 执行期间，B 线通过正常 write 新建源码，B 写日志包含正确路径及终态内容。
- 影响：新建文件一律被设为 Fingerprint，日志匹配逻辑对该状态直接 false；A 的正常命令误报跨树失败，且小文件被误称超过 4 MiB。
- 证据：contract-evidence.json/cross_tree_probe：已有文件合法修改 modified_control=None；增加带合法凭据的 new.txt 后 legal_create=Some([cross-tree] DETECTED)。
- 验证：逐字复制 cross_tree.rs 生产模块；仅 worktree 枚举与日志读取边界桩化，真实文件快照与归因逻辑执行；匹配旧文件控制组通过。

### AF-T04 P2 — crates/kanzei-tools/src/cross_tree.rs
- 触发：跨树变化需留证，但 .kanzei/quarantine 被普通文件占用、无法创建目录（其他写入故障同路径）。
- 影响：create_dir_all 和 write 的错误全部丢弃，报告仍称已隔离且可取回；用户按照报告寻找恢复副本时实际不存在。
- 证据：contract-evidence.json/cross_tree_probe failed_quarantine：fixture 有改变后的 existing.txt，隔离目录被普通文件阻塞，仍输出 evidence quarantined / 隔离留证于；没有实际副本。
- 验证：原生产模块确定性失败路径探针；保留源文件并验证隔离目录障碍。

### AF-T05 P2 — crates/kanzei-tools/src/local_validation.rs
- 触发：write/edit 在独立 worktree 中写入 member/src/lib.rs，Cargo.toml 位于 member/，ctx.project_root 指向主树。
- 影响：nearest_manifest 第一层因不在主树内即停止，误用 Rust 2015；合法 async fn 被错误报告语法失败，UI/模型收到错误修复建议。写入本身保留。
- 证据：contract-evidence.json：main_root=None，code_root=Some(member/Cargo.toml)；同一合法源码 rustfmt --edition 2015 退出 1/E0670，2021 退出 0。write.rs:334、edit.rs:568/807 传入 project_root。
- 验证：原 nearest_manifest 提取 + 实际 rustfmt 正反对照；不变式是工作树源码的 edition 来自该工作树。

### AF-T06 P1 — crates/kanzei-tools/src/palette.rs
- 触发：plot 工具 palette 导入普通 Adobe Swatch Exchange .ase 文件（例如两个标准 RGB 色块）。
- 影响：错误头部偏移、块类型、名称位置及编码使标准 ASE 无法导入；既有 sample.ase 同样采用错误自造布局，掩盖协议不兼容。
- 证据：contract-evidence.json/ase_probe：标准 12-byte header + 0x0001 色块 + UTF-16BE name 解析 0 色失败；仓库 fixture 成功。独立实现格式说明 https://github.com/behreajj/AsepriteSwatchExchange；生产 caller plot_tool.rs:244。
- 验证：逐字提取 read_u16_be 至 parse_ase；标准双 RGB fixture 与仓库 fixture 对照。

### AF-T07 P2 — crates/kanzei-tools/src/grep.rs
- 触发：grep 同时设置 multiline=true、count=true，模式 alpha.*beta 跨越两行。
- 影响：count 的 Searcher 未启用 multiline，返回 no matches；同输入的普通检索命中，统计结果与检索结果矛盾。
- 证据：contract-evidence.json/grep_multiline_probe：count=false => file.txt:1: alpha\nbeta；count=true => (no matches for alpha.*beta)。
- 验证：原 run_grep/run_count/ContextSink 函数提取，链接已有 grep/ignore/globset rlib，rustc 独立编译执行；未共享 Cargo。

### AF-T08 P1 — crates/kanzei-tools/src/symbols.rs
- 触发：独立 worktree 的 agent 查询 symbols {crate:"demo"} 或 module 地图，主树和当前树源码不同。
- 影响：crate_dirs 从 ctx.project_root 建立，带 crate 查询直接扫主树；只带 module 时当前树文件无法匹配主树目录，可能得到空地图。返回的依赖/API 地图不代表当前工作树。
- 证据：contract-evidence.json/symbols_root_probe：相同成员名 main=>main_only、worktree=>branch_only；execute:107 固定选择 main 的映射，:121 按该目录收集源码。
- 验证：原 crate_ident_to_dir 提取 + 主树/分支差异 fixture；完整追踪 execute→collect_rs_files→render_repo_map。

### AF-T09 P1 — crates/kanzei-tools/src/browser_tool/headless.rs
- 触发：桌面 A/B 两个会话或工作线均走无头 fallback；A open 后 B open，A 随后省略 target 调 click/type/eval。
- 影响：全局 helper/page 无会话或工作树 owner，A 的后续操作落到 B 页面；共享 CURRENT_URLS 还把错误页面作为权限资源。
- 证据：browser-evidence.json：真实 Node helper+Edge 执行 A open line-A、B open line-B、A targetless eval，返回 line-B。Rust execute_headless 不把 ctx owner 传给 with_helper/run_action；父审计确认桌面路由无额外 owner 隔离。
- 验证：真实 helper 独立子进程/新 Edge 会话；shutdown 并关闭 stdin 收尾。与 root AF-R09 面板 owner 检查归为同一修复批次。

### AF-T10 P1 — crates/kanzei-tools/src/webfetch.rs
- 触发：webfetch 下载大于 3 MiB 的 PDF。
- 影响：读取达到上限后保存截断 PDF；PDF 分支不携带 truncated 信息，仍返回成功并让用户用 read pages 打开，缓存/来源标记也将其视为已抓取。
- 证据：pdf-evidence.json：真实 CLI/mock provider/local HTTP，返回 PDF saved to 成功，保存文件恰好 3145728 字节且无 EOF。原 valid-large.pdf 经 fitz 打开为 1 页，完整响应尾部 xref/EOF 均在截断点后。
- 验证：当前 fetch_web_response 逐字提取并链接已有 reqwest/tokio rlib：status=200 bytes=3145728 truncated=true eof=false；当前 save/PDF-success 分支逐行核对。release CLI 提供用户结果症状补证，未建立该二进制提交来源。隔离 HOME/USERPROFILE/KANZEI_HOME，本地有效单页 PDF，无真实模型/外部请求。

### AF-T11 P0 — crates/kanzei-tools/src/git/tool.rs
- 触发：A 线调用 git merge_ff，into 分支当前被另一个活跃工作树 B checkout；B 仍在运行内部 write/edit 或 Git 工具。
- 影响：工具声明的排他范围是 A 的 ctx.cwd，但 merge_ff 实际改 B 的 HEAD、index 和文件；A/B 两个内部 run 可同时写 B，绕过一树一写者语义。
- 证据：git.rs:1091+ 找目标分支的 checkout tree，:1125 在该 dir 执行 merge --ff-only；git/tool.rs:95 固定 write_worktree(ctx)。root 已核实 app/run/assembly.rs:596+ 整轮 lease 仅 ctx.cwd，core ToolConcurrency 只给同批排序，无额外目标锁。
- 验证：跨层真实调用/持锁时序静态证明；B 的 write/edit 文件锁未被 Git subprocess 遵守，Git index.lock 也不能替代整个工作树互斥。按用户 mutual exclusion 被破坏口径列 P0；未在用户工作树制造并发 Git 写入。

## crates/kanzei-tools/src/base.rs

### 职责
装配基础工具与代理默认依赖。

### 判断
PASS

### 确切问题
- 未发现有实际影响的问题。

### 修改
- 未改，审计阶段。

### 影响范围
- Harness materialize、CLI/app profile。 本轮无状态、文件格式或 API 兼容性变化。

### 验证
- 全文阅读代码与本文件测试；注册、schema、延迟工具选择和公共依赖方向。
- 完成直接调用合同静态复核；未以未运行测试宣称动态通过。

## crates/kanzei-tools/src/bash.rs

### 职责
执行前台/后台命令并对托管/其他树变化收口。

### 判断
P1

### 确切问题
- AF-T03：新建文件一律被设为 Fingerprint，日志匹配逻辑对该状态直接 false；A 的正常命令误报跨树失败，且小文件被误称超过 4 MiB。（根因位于 crates/kanzei-tools/src/cross_tree.rs，此处为调用合同。）

### 修改
- 未改，审计阶段。

### 影响范围
- agent bash、test_record/background。 涉及运行时行为，后续修复需同步 caller；本轮无兼容性变更。

### 验证
- 全文阅读代码与本文件测试；超时 kill-tree、输出并排读取、围栏结果到 ToolOutput；AF-T03 根因在 cross_tree。
- 关联证据：AF-T03 逐字复制 cross_tree.rs 生产模块；仅 worktree 枚举与日志读取边界桩化，真实文件快照与归因逻辑执行；匹配旧文件控制组通过。

## crates/kanzei-tools/src/browser_tool.rs

### 职责
定义浏览器统一输入、权限资源和双后端输出。

### 判断
P1

### 确切问题
- AF-T09：全局 helper/page 无会话或工作树 owner，A 的后续操作落到 B 页面；共享 CURRENT_URLS 还把错误页面作为权限资源。（根因位于 crates/kanzei-tools/src/browser_tool/headless.rs，此处为调用合同。）

### 修改
- 未改，审计阶段。

### 影响范围
- CLI BrowserTool、app DesktopBrowserTool/preview。 涉及运行时行为，后续修复需同步 caller；本轮无兼容性变更。

### 验证
- 全文阅读代码与本文件测试；目标解析、动作参数、资源 host、全局 current URL 合同。
- 关联证据：AF-T09 真实 helper 独立子进程/新 Edge 会话；shutdown 并关闭 stdin 收尾。与 root AF-R09 面板 owner 检查归为同一修复批次。

## crates/kanzei-tools/src/browser_tool/headless.rs

### 职责
拥有 Node helper 生命周期及浏览器 RPC 调用。

### 判断
P1

### 确切问题
- AF-T09：全局 helper/page 无会话或工作树 owner，A 的后续操作落到 B 页面；共享 CURRENT_URLS 还把错误页面作为权限资源。

### 修改
- 未改，审计阶段。

### 影响范围
- BrowserTool、app fallback。 涉及运行时行为，后续修复需同步 caller；本轮无兼容性变更。

### 验证
- 全文阅读代码与本文件测试；RPC 配对/超时、reaper、owner 与跨调用页面状态。
- 关联证据：AF-T09 真实 helper 独立子进程/新 Edge 会话；shutdown 并关闭 stdin 收尾。与 root AF-R09 面板 owner 检查归为同一修复批次。

## crates/kanzei-tools/src/cross_tree.rs

### 职责
比对其他工作树变化并吸收合法写入、留证报告。

### 判断
P1

### 确切问题
- AF-T03：新建文件一律被设为 Fingerprint，日志匹配逻辑对该状态直接 false；A 的正常命令误报跨树失败，且小文件被误称超过 4 MiB。
- AF-T04：create_dir_all 和 write 的错误全部丢弃，报告仍称已隔离且可取回；用户按照报告寻找恢复副本时实际不存在。

### 修改
- 未改，审计阶段。

### 影响范围
- bash/background。 涉及运行时行为，后续修复需同步 caller；本轮无兼容性变更。

### 验证
- 全文阅读代码与本文件测试；变更三态、新建/删除、写日志分树、IO 失败和归因。
- 关联证据：AF-T03 逐字复制 cross_tree.rs 生产模块；仅 worktree 枚举与日志读取边界桩化，真实文件快照与归因逻辑执行；匹配旧文件控制组通过。；AF-T04 原生产模块确定性失败路径探针；保留源文件并验证隔离目录障碍。

## crates/kanzei-tools/src/dev_urls.rs

### 职责
从前端服务输出提取可访问的开发 URL。

### 判断
PASS

### 确切问题
- 未发现有实际影响的问题。

### 修改
- 未改，审计阶段。

### 影响范围
- process 输出、browser/preview 引导。 本轮无状态、文件格式或 API 兼容性变化。

### 验证
- 全文阅读代码与本文件测试；ANSI、localhost、端口与去重边界。
- 完成直接调用合同静态复核；未以未运行测试宣称动态通过。

## crates/kanzei-tools/src/edit.rs

### 职责
基于锚点或行号执行受控文件编辑。

### 判断
P2

### 确切问题
- AF-T05：nearest_manifest 第一层因不在主树内即停止，误用 Rust 2015；合法 async fn 被错误报告语法失败，UI/模型收到错误修复建议。写入本身保留。（根因位于 crates/kanzei-tools/src/local_validation.rs，此处为调用合同。）

### 修改
- 未改，审计阶段。

### 影响范围
- EditTool/InsertTool、agent。 涉及运行时行为，后续修复需同步 caller；本轮无兼容性变更。

### 验证
- 全文阅读代码与本文件测试；唯一锚点、换行、CAS、读凭据、检查点与失败回报；AF-T05 为下层根因。
- 关联证据：AF-T05 原 nearest_manifest 提取 + 实际 rustfmt 正反对照；不变式是工作树源码的 edition 来自该工作树。

## crates/kanzei-tools/src/files.rs

### 职责
扫描代码树并存取文件标注。

### 判断
PASS

### 确切问题
- 未发现有实际影响的问题。

### 修改
- 未改，审计阶段。

### 影响范围
- files 工具、app files_view。 本轮无状态、文件格式或 API 兼容性变化。

### 验证
- 全文阅读代码与本文件测试；扫描缓存失效、路径、标注存取合同；跨调用方并发点交 root 复核。
- 完成直接调用合同静态复核；未以未运行测试宣称动态通过。

## crates/kanzei-tools/src/frontend.rs

### 职责
提供 CSS 规则定位和结构诊断。

### 判断
PASS

### 确切问题
- 未发现有实际影响的问题。

### 修改
- 未改，审计阶段。

### 影响范围
- frontend_locate/frontend_check。 本轮无状态、文件格式或 API 兼容性变化。

### 验证
- 全文阅读代码与本文件测试；当前代码树读取、条件块深度、既有轻量解析边界。
- 完成直接调用合同静态复核；未以未运行测试宣称动态通过。

## crates/kanzei-tools/src/git.rs

### 职责
拥有受控 Git 暂存、提交、快进及源码背书逻辑。

### 判断
P0

### 确切问题
- AF-T02：父进程写满 git stdin，git 写满 stdout；父进程等写完才 wait_with_output 排空 stdout，形成永久相互等待，命令无法完成。
- AF-T11：工具声明的排他范围是 A 的 ctx.cwd，但 merge_ff 实际改 B 的 HEAD、index 和文件；A/B 两个内部 run 可同时写 B，绕过一树一写者语义。（根因位于 crates/kanzei-tools/src/git/tool.rs，此处为调用合同。）

### 修改
- 未改，审计阶段。

### 影响范围
- GitTool、test_record、work reconcile。 涉及运行时行为，后续修复需同步 caller；本轮无兼容性变更。

### 验证
- 全文阅读代码与本文件测试；全文含测试；staged/index/source 指纹、恢复和目标树边界。
- 关联证据：AF-T02 std-only 原函数隔离进程 + 同规模并行读控制组；看门狗只清理探针进程树。plan.rs:104、finalize.rs:125、test_record/execution.rs:52 都同步直接调用；函数内没有超时，外层 async 取消不能抢占阻塞的 std::io::Write，符合用户永久 deadlock 的 P0 口径。；AF-T11 跨层真实调用/持锁时序静态证明；B 的 write/edit 文件锁未被 Git subprocess 遵守，Git index.lock 也不能替代整个工作树互斥。按用户 mutual exclusion 被破坏口径列 P0；未在用户工作树制造并发 Git 写入。

## crates/kanzei-tools/src/git/commands.rs

### 职责
封装有界 Git 子进程调用。

### 判断
PASS

### 确切问题
- 未发现有实际影响的问题。

### 修改
- 未改，审计阶段。

### 影响范围
- Git 工具/索引/工作树子模块。 本轮无状态、文件格式或 API 兼容性变化。

### 验证
- 全文阅读代码与本文件测试；kill_on_drop、命令超时、stderr 和 status 错误传播。
- 完成直接调用合同静态复核；未以未运行测试宣称动态通过。

## crates/kanzei-tools/src/git/finalize.rs

### 职责
按计划执行测试、暂存、提交收口。

### 判断
P0

### 确切问题
- AF-T02：父进程写满 git stdin，git 写满 stdout；父进程等写完才 wait_with_output 排空 stdout，形成永久相互等待，命令无法完成。（根因位于 crates/kanzei-tools/src/git.rs，此处为调用合同。）

### 修改
- 未改，审计阶段。

### 影响范围
- GitTool finalize。 涉及运行时行为，后续修复需同步 caller；本轮无兼容性变更。

### 验证
- 全文阅读代码与本文件测试；动作顺序、失败即停、计划到提交的状态依赖；AF-T02 为根因。
- 关联证据：AF-T02 std-only 原函数隔离进程 + 同规模并行读控制组；看门狗只清理探针进程树。plan.rs:104、finalize.rs:125、test_record/execution.rs:52 都同步直接调用；函数内没有超时，外层 async 取消不能抢占阻塞的 std::io::Write，符合用户永久 deadlock 的 P0 口径。

## crates/kanzei-tools/src/git/index.rs

### 职责
操作 Git index 并保存 staging 凭据。

### 判断
PASS

### 确切问题
- 未发现有实际影响的问题。

### 修改
- 未改，审计阶段。

### 影响范围
- git stage/commit/finalize。 本轮无状态、文件格式或 API 兼容性变化。

### 验证
- 全文阅读代码与本文件测试；staged hash、路径集合、原子凭据写入。
- 完成直接调用合同静态复核；未以未运行测试宣称动态通过。

## crates/kanzei-tools/src/git/plan.rs

### 职责
生成 stage/commit/finalize 的预执行计划。

### 判断
P0

### 确切问题
- AF-T02：父进程写满 git stdin，git 写满 stdout；父进程等写完才 wait_with_output 排空 stdout，形成永久相互等待，命令无法完成。（根因位于 crates/kanzei-tools/src/git.rs，此处为调用合同。）

### 修改
- 未改，审计阶段。

### 影响范围
- GitTool 与 finalize。 涉及运行时行为，后续修复需同步 caller；本轮无兼容性变更。

### 验证
- 全文阅读代码与本文件测试；源文件集合、测试证据/覆盖面、下层指纹合同；AF-T02 为根因。
- 关联证据：AF-T02 std-only 原函数隔离进程 + 同规模并行读控制组；看门狗只清理探针进程树。plan.rs:104、finalize.rs:125、test_record/execution.rs:52 都同步直接调用；函数内没有超时，外层 async 取消不能抢占阻塞的 std::io::Write，符合用户永久 deadlock 的 P0 口径。

## crates/kanzei-tools/src/git/tool.rs

### 职责
定义 Git 工具 action/schema 与并发资源声明。

### 判断
P0

### 确切问题
- AF-T11：工具声明的排他范围是 A 的 ctx.cwd，但 merge_ff 实际改 B 的 HEAD、index 和文件；A/B 两个内部 run 可同时写 B，绕过一树一写者语义。

### 修改
- 未改，审计阶段。

### 影响范围
- DevProfile、core tool dispatch。 涉及运行时行为，后续修复需同步 caller；本轮无兼容性变更。

### 验证
- 全文阅读代码与本文件测试；全部 action 到实现路由及真实写入范围。
- 关联证据：AF-T11 跨层真实调用/持锁时序静态证明；B 的 write/edit 文件锁未被 Git subprocess 遵守，Git index.lock 也不能替代整个工作树互斥。按用户 mutual exclusion 被破坏口径列 P0；未在用户工作树制造并发 Git 写入。

## crates/kanzei-tools/src/git/worktree.rs

### 职责
在 GitTool 层桥接工作树管理命令。

### 判断
PASS

### 确切问题
- 未发现有实际影响的问题。

### 修改
- 未改，审计阶段。

### 影响范围
- GitTool worktree action。 本轮无状态、文件格式或 API 兼容性变化。

### 验证
- 全文阅读代码与本文件测试；命令错误传播与 branch/path 身份规范化。
- 完成直接调用合同静态复核；未以未运行测试宣称动态通过。

## crates/kanzei-tools/src/glob.rs

### 职责
按 glob 查询当前代码树文件。

### 判断
PASS

### 确切问题
- 未发现有实际影响的问题。

### 修改
- 未改，审计阶段。

### 影响范围
- Base/Subagent 工具注册、agent read。 本轮无状态、文件格式或 API 兼容性变化。

### 验证
- 全文阅读代码与本文件测试；cwd 相对路径、忽略规则、返回上限和非法模式。
- 完成直接调用合同静态复核；未以未运行测试宣称动态通过。

## crates/kanzei-tools/src/grep.rs

### 职责
提供有界内容检索及全量计数。

### 判断
P2

### 确切问题
- AF-T07：count 的 Searcher 未启用 multiline，返回 no matches；同输入的普通检索命中，统计结果与检索结果矛盾。

### 修改
- 未改，审计阶段。

### 影响范围
- Base/Subagent 工具与定位工作流。 涉及运行时行为，后续修复需同步 caller；本轮无兼容性变更。

### 验证
- 全文阅读代码与本文件测试；glob/display 基准、context/limit、multiline 与 count 一致性。
- 关联证据：AF-T07 原 run_grep/run_count/ContextSink 函数提取，链接已有 grep/ignore/globset rlib，rustc 独立编译执行；未共享 Cargo。

## crates/kanzei-tools/src/lib.rs

### 职责
暴露工具模块与复用底层 hash/atomic/journal 原语。

### 判断
PASS

### 确切问题
- 未发现有实际影响的问题。

### 修改
- 未改，审计阶段。

### 影响范围
- 全部工具、app、CLI。 本轮无状态、文件格式或 API 兼容性变化。

### 验证
- 全文阅读代码与本文件测试；公共 re-export、错误映射、write-log 持锁及主根/代码树边界。
- 完成直接调用合同静态复核；未以未运行测试宣称动态通过。

## crates/kanzei-tools/src/local_validation.rs

### 职责
写后执行有界单文件校验并区分代码/环境错误。

### 判断
P2

### 确切问题
- AF-T05：nearest_manifest 第一层因不在主树内即停止，误用 Rust 2015；合法 async fn 被错误报告语法失败，UI/模型收到错误修复建议。写入本身保留。

### 修改
- 未改，审计阶段。

### 影响范围
- write、edit、insert。 涉及运行时行为，后续修复需同步 caller；本轮无兼容性变更。

### 验证
- 全文阅读代码与本文件测试；75ms 同路径去抖、命令超时、Rust edition 来源和状态报告。
- 关联证据：AF-T05 原 nearest_manifest 提取 + 实际 rustfmt 正反对照；不变式是工作树源码的 edition 来自该工作树。

## crates/kanzei-tools/src/managed.rs

### 职责
拥有托管文档快照、合法写日志吸收和恢复。

### 判断
PASS

### 确切问题
- 未发现有实际影响的问题。

### 修改
- 未改，审计阶段。

### 影响范围
- bash/background fence。 本轮无状态、文件格式或 API 兼容性变化。

### 验证
- 全文阅读代码与本文件测试；共享收口锁、合法终态恢复、缺正文/坏日志和隔离失败明确停止。
- 完成直接调用合同静态复核；未以未运行测试宣称动态通过。

## crates/kanzei-tools/src/palette.rs

### 职责
保存配色规范、推荐/评分及导入解析。

### 判断
P1

### 确切问题
- AF-T06：错误头部偏移、块类型、名称位置及编码使标准 ASE 无法导入；既有 sample.ase 同样采用错误自造布局，掩盖协议不兼容。

### 修改
- 未改，审计阶段。

### 影响范围
- plot_tool、app palette。 涉及运行时行为，后续修复需同步 caller；本轮无兼容性变更。

### 验证
- 全文阅读代码与本文件测试；用户板互斥、分类采样、输入校验和 ASE 二进制协议。
- 关联证据：AF-T06 逐字提取 read_u16_be 至 parse_ase；标准双 RGB fixture 与仓库 fixture 对照。

## crates/kanzei-tools/src/preview_server.rs

### 职责
服务本地预览文件和沙箱 HTML 片段。

### 判断
PASS

### 确切问题
- 未发现有实际影响的问题。

### 修改
- 未改，审计阶段。

### 影响范围
- browser target、app preview。 本轮无状态、文件格式或 API 兼容性变化。

### 验证
- 全文阅读代码与本文件测试；根路径 canonical containment、token/root ID、CSP、HTTP header 和文件响应；未做权限型链接 fixture。
- 完成直接调用合同静态复核；未以未运行测试宣称动态通过。

## crates/kanzei-tools/src/project_state.rs

### 职责
缓存项目 Git/语言/入口事实。

### 判断
PASS

### 确切问题
- 未发现有实际影响的问题。

### 修改
- 未改，审计阶段。

### 影响范围
- 工具装配、agent/context、app。 本轮无状态、文件格式或 API 兼容性变化。

### 验证
- 全文阅读代码与本文件测试；缓存 generation、TTL、HEAD/commondir/packedrefs、主根与工作树事实。
- 完成直接调用合同静态复核；未以未运行测试宣称动态通过。

## crates/kanzei-tools/src/project_state/git_init.rs

### 职责
初始化项目 Git 及忽略规则。

### 判断
PASS

### 确切问题
- 未发现有实际影响的问题。

### 修改
- 未改，审计阶段。

### 影响范围
- project_state 初始化路径。 本轮无状态、文件格式或 API 兼容性变化。

### 验证
- 全文阅读代码与本文件测试；已有仓库识别、首次提交、失败保留及 ignore 规则。
- 完成直接调用合同静态复核；未以未运行测试宣称动态通过。

## crates/kanzei-tools/src/project_state/toolchains.rs

### 职责
从项目清单探测工具链及入口事实。

### 判断
PASS

### 确切问题
- 未发现有实际影响的问题。

### 修改
- 未改，审计阶段。

### 影响范围
- project_state 刷新。 本轮无状态、文件格式或 API 兼容性变化。

### 验证
- 全文阅读代码与本文件测试；文件事实与安装状态区分、不依赖外部命令猜版本。
- 完成直接调用合同静态复核；未以未运行测试宣称动态通过。

## crates/kanzei-tools/src/quarantine.rs

### 职责
扫描和清理允许类型的隔离目录并记审计日志。

### 判断
P1

### 确切问题
- AF-T01：互斥参数仅单向检查，dry-run 命令实际进入 apply 并删除隔离恢复材料。（根因位于 crates/kanzei/src/cli/quarantine.rs，此处为调用合同。）

### 修改
- 未改，审计阶段。

### 影响范围
- CLI quarantine。 涉及运行时行为，后续修复需同步 caller；本轮无兼容性变更。

### 验证
- 全文阅读代码与本文件测试；未知目录保留、类型/年龄过滤、dry-run 不写、失败计数。
- 关联证据：AF-T01 当前 parse_args 原函数逐字提取并 rustc 编译：dry-run/apply 返回 apply=true，反序返回 Err，证据 contract-evidence.json/quarantine_parser_probe。现有 release CLI 只作删除症状补证，未建立其二进制提交来源；仅操作 output 下自建 fixture。

## crates/kanzei-tools/src/question.rs

### 职责
提供交互问题的结构化工具输入和结果。

### 判断
PASS

### 确切问题
- 未发现有实际影响的问题。

### 修改
- 未改，审计阶段。

### 影响范围
- runner/AppQuestion、CLI question 路由。 本轮无状态、文件格式或 API 兼容性变化。

### 验证
- 全文阅读代码与本文件测试；必需输入、选项与问题结果合同。
- 完成直接调用合同静态复核；未以未运行测试宣称动态通过。

## crates/kanzei-tools/src/read_receipt_tests.rs

### 职责
验证 read ledger 与写前陈旧读取检测。

### 判断
PASS

### 确切问题
- 未发现有实际影响的问题。

### 修改
- 未改，审计阶段。

### 影响范围
- read/write/edit 测试。 本轮无状态、文件格式或 API 兼容性变化。

### 验证
- 全文阅读代码与本文件测试；确认回归断言检查内容指纹而非仅结果文本。
- 完成直接调用合同静态复核；未以未运行测试宣称动态通过。

## crates/kanzei-tools/src/read.rs

### 职责
读取文本/图像/PDF 并建立读凭据。

### 判断
PASS

### 确切问题
- 未发现有实际影响的问题。

### 修改
- 未改，审计阶段。

### 影响范围
- ReadTool、读后 edit/write。 本轮无状态、文件格式或 API 兼容性变化。

### 验证
- 全文阅读代码与本文件测试；范围裁剪、UTF-8、PDF 页参数、前后 hash 与 ledger 更新。
- 完成直接调用合同静态复核；未以未运行测试宣称动态通过。

## crates/kanzei-tools/src/shell.rs

### 职责
统一 shell 启动、输出编码、PATH 与 Windows 进程树收尾。

### 判断
PASS

### 确切问题
- 未发现有实际影响的问题。

### 修改
- 未改，审计阶段。

### 影响范围
- bash、background、verification/工具子进程。 本轮无状态、文件格式或 API 兼容性变化。

### 验证
- 全文阅读代码与本文件测试；Windows kill-tree 活性检查、编码、PATH 合并；非 Windows 分支按现有限制说明。
- 完成直接调用合同静态复核；未以未运行测试宣称动态通过。

## crates/kanzei-tools/src/symbols.rs

### 职责
生成源码符号、定义/引用及 crate/module 地图。

### 判断
P1

### 确切问题
- AF-T08：crate_dirs 从 ctx.project_root 建立，带 crate 查询直接扫主树；只带 module 时当前树文件无法匹配主树目录，可能得到空地图。返回的依赖/API 地图不代表当前工作树。

### 修改
- 未改，审计阶段。

### 影响范围
- agent symbols、依赖图审查。 涉及运行时行为，后续修复需同步 caller；本轮无兼容性变更。

### 验证
- 全文阅读代码与本文件测试；词边界、再导出、目录扫描与当前工作树来源。
- 关联证据：AF-T08 原 crate_ident_to_dir 提取 + 主树/分支差异 fixture；完整追踪 execute→collect_rs_files→render_repo_map。

## crates/kanzei-tools/src/team/workspace.rs

### 职责
为子代理建立隔离工作区并以快照差异接收结果。

### 判断
PASS

### 确切问题
- 未发现有实际影响的问题。

### 修改
- 未改，审计阶段。

### 影响范围
- team 调度与 adopt。 本轮无状态、文件格式或 API 兼容性变化。

### 验证
- 全文阅读代码与本文件测试；临时 index、commit-tree、结果 ref 保活、apply check 和父树 index 保留。
- 完成直接调用合同静态复核；未以未运行测试宣称动态通过。

## crates/kanzei-tools/src/verification/mod.rs

### 职责
拥有验证 job 持久状态及 WorkUnit 结果接入。

### 判断
PASS

### 确切问题
- 未发现有实际影响的问题。

### 修改
- 未改，审计阶段。

### 影响范围
- bash verification、CLI worker、work/app monitor。 本轮无状态、文件格式或 API 兼容性变化。

### 验证
- 全文阅读代码与本文件测试；状态切换、session 来源、旧 job 不覆盖新 WorkUnit。
- 完成直接调用合同静态复核；未以未运行测试宣称动态通过。

## crates/kanzei-tools/src/verification/process.rs

### 职责
运行冻结快照的验证命令并管理进程结果。

### 判断
PASS

### 确切问题
- 未发现有实际影响的问题。

### 修改
- 未改，审计阶段。

### 影响范围
- verification worker。 本轮无状态、文件格式或 API 兼容性变化。

### 验证
- 全文阅读代码与本文件测试；超时、退出码、输出上限及进程生命周期。
- 完成直接调用合同静态复核；未以未运行测试宣称动态通过。

## crates/kanzei-tools/src/verification/snapshot.rs

### 职责
为异步验证冻结源文件快照和源码指纹。

### 判断
PASS

### 确切问题
- 未发现有实际影响的问题。

### 修改
- 未改，审计阶段。

### 影响范围
- verification submit。 本轮无状态、文件格式或 API 兼容性变化。

### 验证
- 全文阅读代码与本文件测试；Git 跟踪/未跟踪来源、路径过滤、快照失败停止。
- 完成直接调用合同静态复核；未以未运行测试宣称动态通过。

## crates/kanzei-tools/src/verification/tests.rs

### 职责
验证冻结源、异步完成及 job 状态合同。

### 判断
PASS

### 确切问题
- 未发现有实际影响的问题。

### 修改
- 未改，审计阶段。

### 影响范围
- kanzei-tools verification 单测。 本轮无状态、文件格式或 API 兼容性变化。

### 验证
- 全文阅读代码与本文件测试；全文核对测试断言与实际状态 owner；本轮未运行 Cargo。
- 完成直接调用合同静态复核；未以未运行测试宣称动态通过。

## crates/kanzei-tools/src/verification/worker.rs

### 职责
持 job/resource lease 驱动独立验证工作。

### 判断
PASS

### 确切问题
- 未发现有实际影响的问题。

### 修改
- 未改，审计阶段。

### 影响范围
- CLI 验证 worker、verification monitor。 本轮无状态、文件格式或 API 兼容性变化。

### 验证
- 全文阅读代码与本文件测试；单 job 执行、结果完成与资源释放顺序。
- 完成直接调用合同静态复核；未以未运行测试宣称动态通过。

## crates/kanzei-tools/src/web_refs.rs

### 职责
持久保存会话范围网页引用和已抓取证据。

### 判断
PASS

### 确切问题
- 未发现有实际影响的问题。

### 修改
- 未改，审计阶段。

### 影响范围
- websearch remember、webfetch resolve/mark_fetched。 本轮无状态、文件格式或 API 兼容性变化。

### 验证
- 全文阅读代码与本文件测试；session 分区、锁内更新、裁剪与来源标记。
- 完成直接调用合同静态复核；未以未运行测试宣称动态通过。

## crates/kanzei-tools/src/webfetch.rs

### 职责
抓取、保存、缓存网页/PDF 并返回来源。

### 判断
P1

### 确切问题
- AF-T10：读取达到上限后保存截断 PDF；PDF 分支不携带 truncated 信息，仍返回成功并让用户用 read pages 打开，缓存/来源标记也将其视为已抓取。

### 修改
- 未改，审计阶段。

### 影响范围
- agent webfetch、web_refs、read。 涉及运行时行为，后续修复需同步 caller；本轮无兼容性变更。

### 验证
- 全文阅读代码与本文件测试；重定向、响应上限、PDF 二进制完整性和来源/缓存。
- 关联证据：AF-T10 当前 fetch_web_response 逐字提取并链接已有 reqwest/tokio rlib：status=200 bytes=3145728 truncated=true eof=false；当前 save/PDF-success 分支逐行核对。release CLI 提供用户结果症状补证，未建立该二进制提交来源。隔离 HOME/USERPROFILE/KANZEI_HOME，本地有效单页 PDF，无真实模型/外部请求。

## crates/kanzei-tools/src/websearch.rs

### 职责
执行批量联网搜索及后端回退、引用登记。

### 判断
PASS

### 确切问题
- 未发现有实际影响的问题。

### 修改
- 未改，审计阶段。

### 影响范围
- Base/Research profile、webfetch refs。 本轮无状态、文件格式或 API 兼容性变化。

### 验证
- 全文阅读代码与本文件测试；参数校验、超时/重试/冷却、结果域名过滤和预算调用；未进行真实付费搜索。
- 完成直接调用合同静态复核；未以未运行测试宣称动态通过。

## crates/kanzei-tools/src/worktree.rs

### 职责
维护 Git worktree 目标、创建凭据及失败清理。

### 判断
PASS

### 确切问题
- 未发现有实际影响的问题。

### 修改
- 未改，审计阶段。

### 影响范围
- app workspace、Git/CLI/team。 本轮无状态、文件格式或 API 兼容性变化。

### 验证
- 全文阅读代码与本文件测试；branch CAS、receipt 回滚、canonical key、Git 真实注册清单。
- 完成直接调用合同静态复核；未以未运行测试宣称动态通过。

## crates/kanzei-tools/src/write.rs

### 职责
执行带检查点、读凭据和写日志的新建/覆写。

### 判断
P2

### 确切问题
- AF-T05：nearest_manifest 第一层因不在主树内即停止，误用 Rust 2015；合法 async fn 被错误报告语法失败，UI/模型收到错误修复建议。写入本身保留。（根因位于 crates/kanzei-tools/src/local_validation.rs，此处为调用合同。）

### 修改
- 未改，审计阶段。

### 影响范围
- WriteTool、编辑/恢复调用。 涉及运行时行为，后续修复需同步 caller；本轮无兼容性变更。

### 验证
- 全文阅读代码与本文件测试；锁内 CAS、checkpoint/journal 失败、物理树归属；AF-T05 来自校验下层。
- 关联证据：AF-T05 原 nearest_manifest 提取 + 实际 rustfmt 正反对照；不变式是工作树源码的 edition 来自该工作树。

## crates/kanzei/src/cli/artifacts.rs

### 职责
提供产物列举、查看、整理的 CLI 入口。

### 判断
PASS

### 确切问题
- 未发现有实际影响的问题。

### 修改
- 未改，审计阶段。

### 影响范围
- CLI 用户→artifact store。 本轮无状态、文件格式或 API 兼容性变化。

### 验证
- 全文阅读代码与本文件测试；路径解析、过滤、JSON/text 输出与失败传播。
- 完成直接调用合同静态复核；未以未运行测试宣称动态通过。

## crates/kanzei/src/cli/quarantine.rs

### 职责
解析并执行隔离区清理命令。

### 判断
P1

### 确切问题
- AF-T01：互斥参数仅单向检查，dry-run 命令实际进入 apply 并删除隔离恢复材料。

### 修改
- 未改，审计阶段。

### 影响范围
- CLI 用户→tools quarantine。 涉及运行时行为，后续修复需同步 caller；本轮无兼容性变更。

### 验证
- 全文阅读代码与本文件测试；参数互斥、dry-run/apply 状态和真实 CLI 失败路径。
- 关联证据：AF-T01 当前 parse_args 原函数逐字提取并 rustc 编译：dry-run/apply 返回 apply=true，反序返回 Err，证据 contract-evidence.json/quarantine_parser_probe。现有 release CLI 只作删除症状补证，未建立其二进制提交来源；仅操作 output 下自建 fixture。

## crates/kanzei/src/cli/worktree.rs

### 职责
提供工作树管理 CLI 路由。

### 判断
PASS

### 确切问题
- 未发现有实际影响的问题。

### 修改
- 未改，审计阶段。

### 影响范围
- CLI→tools worktree。 本轮无状态、文件格式或 API 兼容性变化。

### 验证
- 全文阅读代码与本文件测试；参数与底层路径/branch contract 对齐。
- 完成直接调用合同静态复核；未以未运行测试宣称动态通过。

## crates/kanzei/tests/integration/always_allow_bash.rs

### 职责
验证非交互权限与 CLI 工具结果持久化。

### 判断
PASS

### 确切问题
- 未发现有实际影响的问题。

### 修改
- 未改，审计阶段。

### 影响范围
- CLI/core/harness 集成。 本轮无状态、文件格式或 API 兼容性变化。

### 验证
- 全文阅读代码与本文件测试；私有 HOME、显式 allow、拒绝后的 paired results 与恢复。
- 完成直接调用合同静态复核；未以未运行测试宣称动态通过。

## crates/kanzei/tests/integration/bash_action_literal.rs

### 职责
固定 bash action 与权限规范化的跨 crate 合同。

### 判断
PASS

### 确切问题
- 未发现有实际影响的问题。

### 修改
- 未改，审计阶段。

### 影响范围
- harness permission 与 tools BashTool。 本轮无状态、文件格式或 API 兼容性变化。

### 验证
- 全文阅读代码与本文件测试；按行为识别工具，不按被测名称自证；命令资源不作路径折叠。
- 完成直接调用合同静态复核；未以未运行测试宣称动态通过。

## crates/kanzei/tests/integration/tool_search_deferred_loading.rs

### 职责
验证延迟工具发现、加载与上下文恢复。

### 判断
PASS

### 确切问题
- 未发现有实际影响的问题。

### 修改
- 未改，审计阶段。

### 影响范围
- 真实 CLI/mock provider/core。 本轮无状态、文件格式或 API 兼容性变化。

### 验证
- 全文阅读代码与本文件测试；schema 去重、overflow 后加载保留、既有调用种子和账单记录。
- 完成直接调用合同静态复核；未以未运行测试宣称动态通过。

## crates/kanzei/tests/integration/worktree_main_root.rs

### 职责
验证 worktree 运行时主根资产路由。

### 判断
PASS

### 确切问题
- 未发现有实际影响的问题。

### 修改
- 未改，审计阶段。

### 影响范围
- CLI 集成测试。 本轮无状态、文件格式或 API 兼容性变化。

### 验证
- 全文阅读代码与本文件测试；配置、会话/状态根和 cwd 的区分；全文断言复核。
- 完成直接调用合同静态复核；未以未运行测试宣称动态通过。

# Module Summary

## 已修复
- P0：0。
- P1：0。
- P2：0。
- 当前是先审后改阶段，以上 11 个确认问题均待统一修复；不把已发现写成已修复。

## PASS 文件
- crates/kanzei-tools/src/base.rs
- crates/kanzei-tools/src/dev_urls.rs
- crates/kanzei-tools/src/files.rs
- crates/kanzei-tools/src/frontend.rs
- crates/kanzei-tools/src/git/commands.rs
- crates/kanzei-tools/src/git/index.rs
- crates/kanzei-tools/src/git/worktree.rs
- crates/kanzei-tools/src/glob.rs
- crates/kanzei-tools/src/lib.rs
- crates/kanzei-tools/src/managed.rs
- crates/kanzei-tools/src/preview_server.rs
- crates/kanzei-tools/src/project_state.rs
- crates/kanzei-tools/src/project_state/git_init.rs
- crates/kanzei-tools/src/project_state/toolchains.rs
- crates/kanzei-tools/src/question.rs
- crates/kanzei-tools/src/read_receipt_tests.rs
- crates/kanzei-tools/src/read.rs
- crates/kanzei-tools/src/shell.rs
- crates/kanzei-tools/src/team/workspace.rs
- crates/kanzei-tools/src/verification/mod.rs
- crates/kanzei-tools/src/verification/process.rs
- crates/kanzei-tools/src/verification/snapshot.rs
- crates/kanzei-tools/src/verification/tests.rs
- crates/kanzei-tools/src/verification/worker.rs
- crates/kanzei-tools/src/web_refs.rs
- crates/kanzei-tools/src/websearch.rs
- crates/kanzei-tools/src/worktree.rs
- crates/kanzei/src/cli/artifacts.rs
- crates/kanzei/src/cli/worktree.rs
- crates/kanzei/tests/integration/always_allow_bash.rs
- crates/kanzei/tests/integration/bash_action_literal.rs
- crates/kanzei/tests/integration/tool_search_deferred_loading.rs
- crates/kanzei/tests/integration/worktree_main_root.rs

## 仍需人工判断
- 无阻塞产品决策。缺陷可在统一批次按底层 owner 修复。

## 依赖影响
- 未修改 cross-module contract。修复批次应收敛：源码根路径（local_validation/symbols）；跨树并发范围（Git/Bash）；浏览器 owner（本报告 AF-T09 与 root AF-R09）；二进制格式与截断失败语义。

## 剩余风险
- 未运行工作区 cargo check/test；本阶段不修改产品，不宣称全工作区绿。
- Git 目标树 lease 问题使用真实调用/锁范围静态时序证据，没有对用户活跃树做并发破坏性实验。
- browser 探针直接驱动生产 helper；Rust owner 缺失与 app route 的连通性经双方静态核实，没有伪称完整桌面双线 E2E。

## 探针与历史依据
- output/audit-first/tools/probe_git_hash.py、git-hash-evidence.json。
- output/audit-first/tools/probe_contracts.py、contract-evidence.json。
- output/audit-first/tools/probe_browser.py、browser-evidence.json。
- output/audit-first/tools/probe_pdf.py、pdf-evidence.json、valid-large.pdf（fitz 可读取 1 页）。
- 历史边界按源码记录 D-395/D-407：跨树只报告留证，不重新启用自动回滚；R-177/R-182：工作树源码与主根资产分离；R-268：托管围栏按写日志归因。
- ASE 格式独立实现说明：[AsepriteSwatchExchange](https://github.com/behreajj/AsepriteSwatchExchange)。
