# 工具与浏览器修复记录

整合状态：已在 dev 分组提交，未发布；提交与独立快照验证见 [修复汇总](fixes-summary.md)。

9项根因已实现：P1 6项、P2 3项。基线55eaca24。未提交、未发布。工具相关155项测试通过，真实Edge归属回归通过；App全量624通过，最后一处导航前guard增量回归1项通过。工具库最终821通过、0失败、3项原有忽略。

修改前已搜索公开接口与直接caller；没有持久格式迁移。保留共享目录其他任务的改动。

## crates/kanzei-tools/src/cross_tree.rs

### 职责
跨树文件变化对账与证据保存。

### 判断
P1（对应 AF-T03, AF-T04）

### 确切问题
- AF-T03：A 线 bash 执行期间，B 线通过正常 write 新建源码，B 写日志包含正确路径及终态内容。 新建文件一律被设为 Fingerprint，日志匹配逻辑对该状态直接 false；A 的正常命令误报跨树失败，且小文件被误称超过 4 MiB。
- AF-T04：跨树变化需留证，但 .kanzei/quarantine 被普通文件占用、无法创建目录（其他写入故障同路径）。 create_dir_all 和 write 的错误全部丢弃，报告仍称已隔离且可取回；用户按照报告寻找恢复副本时实际不存在。

### 修改
- 新增文件也读取限额内正文，与该物理树写入日志核对；合法新增不再误报越界。
- 隔离目录创建和写入逐项核对结果，只报告实际保存成功的副本与数量，失败明确点名且不回滚现状。

### 影响范围
- bash前后围栏、每物理树写入凭据；不恢复自动回滚。

### 验证
- 并行双线_b线窗口内自写有写日志_被吸收不误报
- quarantine_failure_is_reported_without_claiming_a_saved_copy
- 本组Rust回归通过；工具库最终821通过/0失败/3忽略。App原全量624通过，最后owner guard增量1项通过。

## crates/kanzei-tools/src/local_validation.rs

### 职责
写后局部语法校验计划。

### 判断
P2（对应 AF-T05）

### 确切问题
- AF-T05：write/edit 在独立 worktree 中写入 member/src/lib.rs，Cargo.toml 位于 member/，ctx.project_root 指向主树。 nearest_manifest 第一层因不在主树内即停止，误用 Rust 2015；合法 async fn 被错误报告语法失败，UI/模型收到错误修复建议。写入本身保留。

### 修改
- write/edit 把真实 cwd 传给写后校验，Cargo edition、命令 cwd 和源码相对路径使用同一代码根。

### 影响范围
- write/edit 的代码根参数；无持久格式变化。

### 验证
- worktree_write_validates_against_its_own_manifest
- 本组Rust回归通过；工具库最终821通过/0失败/3忽略。App原全量624通过，最后owner guard增量1项通过。

## crates/kanzei-tools/src/write.rs

### 职责
写入专用通道及写后校验。

### 判断
P2（对应 AF-T05）

### 确切问题
- AF-T05：write/edit 在独立 worktree 中写入 member/src/lib.rs，Cargo.toml 位于 member/，ctx.project_root 指向主树。 nearest_manifest 第一层因不在主树内即停止，误用 Rust 2015；合法 async fn 被错误报告语法失败，UI/模型收到错误修复建议。写入本身保留。

### 修改
- write/edit 把真实 cwd 传给写后校验，Cargo edition、命令 cwd 和源码相对路径使用同一代码根。

### 影响范围
- 把ctx.cwd传给校验；新增真实write入口回归。

### 验证
- worktree_write_validates_against_its_own_manifest
- 本组Rust回归通过；工具库最终821通过/0失败/3忽略。App原全量624通过，最后owner guard增量1项通过。

## crates/kanzei-tools/src/edit.rs

### 职责
精确编辑和多处编辑通道。

### 判断
P2（对应 AF-T05）

### 确切问题
- AF-T05：write/edit 在独立 worktree 中写入 member/src/lib.rs，Cargo.toml 位于 member/，ctx.project_root 指向主树。 nearest_manifest 第一层因不在主树内即停止，误用 Rust 2015；合法 async fn 被错误报告语法失败，UI/模型收到错误修复建议。写入本身保留。

### 修改
- write/edit 把真实 cwd 传给写后校验，Cargo edition、命令 cwd 和源码相对路径使用同一代码根。

### 影响范围
- 两个写后校验调用都传ctx.cwd。

### 验证
- worktree_write_validates_against_its_own_manifest
- 本组Rust回归通过；工具库最终821通过/0失败/3忽略。App原全量624通过，最后owner guard增量1项通过。

## crates/kanzei-tools/src/palette.rs

### 职责
色板格式解析与注册。

### 判断
P1（对应 AF-T06）

### 确切问题
- AF-T06：plot 工具 palette 导入普通 Adobe Swatch Exchange .ase 文件（例如两个标准 RGB 色块）。 错误头部偏移、块类型、名称位置及编码使标准 ASE 无法导入；既有 sample.ase 同样采用错误自造布局，掩盖协议不兼容。

### 修改
- 按 ASE 1.0 的12字节头、0x0001色块、UTF-16BE色名与平铺分组解析，限制读取在当前块内；替换原错误二进制fixture。

### 影响范围
- plot_tool的ASE导入；修正标准协议兼容。

### 验证
- ase_unicode_names_flat_groups_and_block_bounds; ase导入解析与非法诊断
- 本组Rust回归通过；工具库最终821通过/0失败/3忽略。App原全量624通过，最后owner guard增量1项通过。

## crates/kanzei-tools/assets/palettes/fixtures/sample.ase

### 职责
标准ASE测试输入。

### 判断
P1（对应 AF-T06）

### 确切问题
- AF-T06：plot 工具 palette 导入普通 Adobe Swatch Exchange .ase 文件（例如两个标准 RGB 色块）。 错误头部偏移、块类型、名称位置及编码使标准 ASE 无法导入；既有 sample.ase 同样采用错误自造布局，掩盖协议不兼容。

### 修改
- 按 ASE 1.0 的12字节头、0x0001色块、UTF-16BE色名与平铺分组解析，限制读取在当前块内；替换原错误二进制fixture。

### 影响范围
- palette与plot_tool共享fixture。

### 验证
- ase_unicode_names_flat_groups_and_block_bounds; ase导入解析与非法诊断
- 本组Rust回归通过；工具库最终821通过/0失败/3忽略。App原全量624通过，最后owner guard增量1项通过。

## crates/kanzei-tools/src/grep.rs

### 职责
源码搜索和计数。

### 判断
P2（对应 AF-T07）

### 确切问题
- AF-T07：grep 同时设置 multiline=true、count=true，模式 alpha.*beta 跨越两行。 count 的 Searcher 未启用 multiline，返回 no matches；同输入的普通检索命中，统计结果与检索结果矛盾。

### 修改
- count 搜索器与普通搜索共用 multiline 语义。

### 影响范围
- count+multiline实际命中；无API变化。

### 验证
- multiline_count_matches_normal_search
- 本组Rust回归通过；工具库最终821通过/0失败/3忽略。App原全量624通过，最后owner guard增量1项通过。

## crates/kanzei-tools/src/symbols.rs

### 职责
实时符号与依赖地图。

### 判断
P1（对应 AF-T08）

### 确切问题
- AF-T08：独立 worktree 的 agent 查询 symbols {crate:"demo"} 或 module 地图，主树和当前树源码不同。 crate_dirs 从 ctx.project_root 建立，带 crate 查询直接扫主树；只带 module 时当前树文件无法匹配主树目录，可能得到空地图。返回的依赖/API 地图不代表当前工作树。

### 修改
- crate、module、define 都以 cwd 的实时源码建立地图，保留另一任务 needs_correction 分类修改。

### 影响范围
- 工作树crate/module/define视图；保留并行结果分类。

### 验证
- map_and_definitions_use_current_worktree
- 本组Rust回归通过；工具库最终821通过/0失败/3忽略。App原全量624通过，最后owner guard增量1项通过。

## crates/kanzei-tools/src/webfetch.rs

### 职责
网页/PDF获取与缓存。

### 判断
P1（对应 AF-T10）

### 确切问题
- AF-T10：webfetch 下载大于 3 MiB 的 PDF。 读取达到上限后保存截断 PDF；PDF 分支不携带 truncated 信息，仍返回成功并让用户用 read pages 打开，缓存/来源标记也将其视为已抓取。

### 修改
- 超限PDF明确失败且不保存/缓存残缺正文；只有读到限额之外字节才标记截断，恰好3MiB允许完整保存。

### 影响范围
- 超限PDF失败语义；缓存与落盘保持一致。

### 验证
- pdf_limit_rejects_partial_download_but_accepts_exact_boundary
- 本组Rust回归通过；工具库最终821通过/0失败/3忽略。App原全量624通过，最后owner guard增量1项通过。

## crates/kanzei-tools/src/browser_tool.rs

### 职责
浏览器共同输入输出与权限资源。

### 判断
P1（对应 AF-T09）

### 确切问题
- AF-T09：桌面 A/B 两个会话或工作线均走无头 fallback；A open 后 B open，A 随后省略 target 调 click/type/eval。 全局 helper/page 无会话或工作树 owner，A 的后续操作落到 B 页面；共享 CURRENT_URLS 还把错误页面作为权限资源。

### 修改
- 无头浏览器按项目会话和子代理owner隔离页面、控制台与权限URL；子代理独立owner不改变会话/写租约身份，桌面子代理走无头；helper闲置清理与退出统一关闭所有owner。

### 影响范围
- current_url/set_current_url增加ctx；所有caller已搜索并跟进。

### 验证
- parent_and_parallel_children_have_distinct_browser_owners; scripts/browser-session-smoke.mjs
- 本组Rust回归通过；工具库最终821通过/0失败/3忽略。App原全量624通过，最后owner guard增量1项通过。

## crates/kanzei-tools/src/browser_tool/headless.rs

### 职责
helper RPC及生命周期。

### 判断
P1（对应 AF-T09）

### 确切问题
- AF-T09：桌面 A/B 两个会话或工作线均走无头 fallback；A open 后 B open，A 随后省略 target 调 click/type/eval。 全局 helper/page 无会话或工作树 owner，A 的后续操作落到 B 页面；共享 CURRENT_URLS 还把错误页面作为权限资源。

### 修改
- 无头浏览器按项目会话和子代理owner隔离页面、控制台与权限URL；子代理独立owner不改变会话/写租约身份，桌面子代理走无头；helper闲置清理与退出统一关闭所有owner。

### 影响范围
- 每个RPC带owner；URL回写在helper动作锁内。

### 验证
- parent_and_parallel_children_have_distinct_browser_owners; scripts/browser-session-smoke.mjs
- 本组Rust回归通过；工具库最终821通过/0失败/3忽略。App原全量624通过，最后owner guard增量1项通过。

## scripts/browser-helper.mjs

### 职责
无头页面的实际state owner。

### 判断
P1（对应 AF-T09）

### 确切问题
- AF-T09：桌面 A/B 两个会话或工作线均走无头 fallback；A open 后 B open，A 随后省略 target 调 click/type/eval。 全局 helper/page 无会话或工作树 owner，A 的后续操作落到 B 页面；共享 CURRENT_URLS 还把错误页面作为权限资源。

### 修改
- 无头浏览器按项目会话和子代理owner隔离页面、控制台与权限URL；子代理独立owner不改变会话/写租约身份，桌面子代理走无头；helper闲置清理与退出统一关闭所有owner。

### 影响范围
- 进程内按owner隔离browser/context/page/console；RPC要求owner。

### 验证
- parent_and_parallel_children_have_distinct_browser_owners; scripts/browser-session-smoke.mjs
- 本组Rust回归通过；工具库最终821通过/0失败/3忽略。App原全量624通过，最后owner guard增量1项通过。

## scripts/browser-session-smoke.mjs

### 职责
实际helper与Edge归属回归。

### 判断
P1（对应 AF-T09）

### 确切问题
- AF-T09：桌面 A/B 两个会话或工作线均走无头 fallback；A open 后 B open，A 随后省略 target 调 click/type/eval。 全局 helper/page 无会话或工作树 owner，A 的后续操作落到 B 页面；共享 CURRENT_URLS 还把错误页面作为权限资源。

### 修改
- 无头浏览器按项目会话和子代理owner隔离页面、控制台与权限URL；子代理独立owner不改变会话/写租约身份，桌面子代理走无头；helper闲置清理与退出统一关闭所有owner。

### 影响范围
- 主会话/双子代理交替、未打开、缺owner、shutdown。

### 验证
- parent_and_parallel_children_have_distinct_browser_owners; scripts/browser-session-smoke.mjs
- 本组Rust回归通过；工具库最终821通过/0失败/3忽略。App原全量624通过，最后owner guard增量1项通过。

## crates/kanzei-harness/src/tool.rs

### 职责
工具执行上下文。

### 判断
P1（对应 AF-T09）

### 确切问题
- AF-T09：桌面 A/B 两个会话或工作线均走无头 fallback；A open 后 B open，A 随后省略 target 调 click/type/eval。 全局 helper/page 无会话或工作树 owner，A 的后续操作落到 B 页面；共享 CURRENT_URLS 还把错误页面作为权限资源。

### 修改
- 无头浏览器按项目会话和子代理owner隔离页面、控制台与权限URL；子代理独立owner不改变会话/写租约身份，桌面子代理走无头；helper闲置清理与退出统一关闭所有owner。

### 影响范围
- 新增可选browser_owner和child派生；保留execution_coordinator/ToolOutcome并行修改。

### 验证
- parent_and_parallel_children_have_distinct_browser_owners; scripts/browser-session-smoke.mjs
- 本组Rust回归通过；工具库最终821通过/0失败/3忽略。App原全量624通过，最后owner guard增量1项通过。

## crates/kanzei-core/src/runner/subagent.rs

### 职责
子代理上下文装配。

### 判断
P1（对应 AF-T09）

### 确切问题
- AF-T09：桌面 A/B 两个会话或工作线均走无头 fallback；A open 后 B open，A 随后省略 target 调 click/type/eval。 全局 helper/page 无会话或工作树 owner，A 的后续操作落到 B 页面；共享 CURRENT_URLS 还把错误页面作为权限资源。

### 修改
- 无头浏览器按项目会话和子代理owner隔离页面、控制台与权限URL；子代理独立owner不改变会话/写租约身份，桌面子代理走无头；helper闲置清理与退出统一关闭所有owner。

### 影响范围
- clone后仅派生browser owner；保留inbox/read ledger隔离。

### 验证
- parent_and_parallel_children_have_distinct_browser_owners; scripts/browser-session-smoke.mjs
- 本组Rust回归通过；工具库最终821通过/0失败/3忽略。App原全量624通过，最后owner guard增量1项通过。

## crates/kanzei-app/src/harness_ext.rs

### 职责
桌面browser路由装配。

### 判断
P1（对应 AF-T09）

### 确切问题
- AF-T09：桌面 A/B 两个会话或工作线均走无头 fallback；A open 后 B open，A 随后省略 target 调 click/type/eval。 全局 helper/page 无会话或工作树 owner，A 的后续操作落到 B 页面；共享 CURRENT_URLS 还把错误页面作为权限资源。

### 修改
- 无头浏览器按项目会话和子代理owner隔离页面、控制台与权限URL；子代理独立owner不改变会话/写租约身份，桌面子代理走无头；helper闲置清理与退出统一关闭所有owner。

### 影响范围
- child直接无头、权限URL按相同owner读取。

### 验证
- parent_and_parallel_children_have_distinct_browser_owners; scripts/browser-session-smoke.mjs
- 本组Rust回归通过；工具库最终821通过/0失败/3忽略。App原全量624通过，最后owner guard增量1项通过。

## crates/kanzei-app/src/desktop_bridge.rs

### 职责
服务到桌面的browser桥。

### 判断
P1（对应 AF-T09）

### 确切问题
- AF-T09：桌面 A/B 两个会话或工作线均走无头 fallback；A open 后 B open，A 随后省略 target 调 click/type/eval。 全局 helper/page 无会话或工作树 owner，A 的后续操作落到 B 页面；共享 CURRENT_URLS 还把错误页面作为权限资源。

### 修改
- 无头浏览器按项目会话和子代理owner隔离页面、控制台与权限URL；子代理独立owner不改变会话/写租约身份，桌面子代理走无头；helper闲置清理与退出统一关闭所有owner。

### 影响范围
- 无头URL按ctx读取。

### 验证
- parent_and_parallel_children_have_distinct_browser_owners; scripts/browser-session-smoke.mjs
- 本组Rust回归通过；工具库最终821通过/0失败/3忽略。App原全量624通过，最后owner guard增量1项通过。

## crates/kanzei-app/src/preview/mod.rs

### 职责
面板元数据owner。

### 判断
P1（对应 AF-R09）

### 确切问题
- AF-R09：A 的 browser 动作已选中面板，等待另一个父/子代理动作释放 agent 锁；用户切换到 B，可见性命令将面板绑定 B。 A 后续获得锁仍操作当前 B 面板。服务桥只比 URL，同页切会话无法拦截。

### 修改
- 面板等待动作锁后复核原generation、owner epoch、process与页面URL；A→B→A同URL也拒绝旧动作。

### 影响范围
- 私有非序列化owner_epoch；可见绑定改变才递增。

### 验证
- queued_action_rejects_rebinding_and_recreated_pane_even_at_same_url
- 本组Rust回归通过；工具库最终821通过/0失败/3忽略。App原全量624通过，最后owner guard增量1项通过。

## crates/kanzei-app/src/preview/agent.rs

### 职责
模型驱动面板操作。

### 判断
P1（对应 AF-R09）

### 确切问题
- AF-R09：A 的 browser 动作已选中面板，等待另一个父/子代理动作释放 agent 锁；用户切换到 B，可见性命令将面板绑定 B。 A 后续获得锁仍操作当前 B 面板。服务桥只比 URL，同页切会话无法拦截。

### 修改
- 面板等待动作锁后复核原generation、owner epoch、process与页面URL；A→B→A同URL也拒绝旧动作。

### 影响范围
- 获取agent锁后身份复核、导航等待后复核；child不进入pane。

### 验证
- queued_action_rejects_rebinding_and_recreated_pane_even_at_same_url
- 本组Rust回归通过；工具库最终821通过/0失败/3忽略。App原全量624通过，最后owner guard增量1项通过。

# Module Summary

## 已修复
- P0: 无（Git两个P0由root负责）。
- P1: AF-T03、06、08、09、10、AF-R09。
- P2: AF-T04、05、07。

## PASS 文件
- 沿用只读审计tools.md的PASS清单，未为风格改动。

## 仍需人工判断
- 无。

## 依赖影响
- write/edit→local_validation统一代码根；browser URL接口带ToolCtx；ToolCtx增加独立子代理browser owner；helper RPC携带owner；面板owner epoch只在运行时维护。

## 剩余风险
- 最终工具库与面板owner增量验证均通过。可见WebView切线未做原生UI实机重放，当前以确定性owner状态回归验证。

## 文档与交叉复核

- docs/design/preview_pane.md §5 已更新：主会话与子代理owner隔离、子代理强制无头、排队面板generation/owner/page复核。
- 独立复核root的AF-T02/11修复：双管道并发排空、源OID解析、目标租约生命周期与忙即拒/cancel_waiter路径，未发现新增确定遗漏。
- 产品文件和当前LF归一SHA-256清单见 fixes-tools.json；共享文件含他人改动，提交前仍应按diff核对。

## 最终验证证据

- `output/audit-first/automation/fix-tools-tests-final.log`:821通过、0失败、3忽略，含本组155项相邻模块测试。
- `output/audit-first/conversation/fix-app-tests.log`:App全量624通过。
- `output/audit-first/automation/fix-preview-owner-tests.log`:最终导航前guard修改后，面板owner回归1项通过。
- `node scripts/browser-session-smoke.mjs`:实际Edge主会话/双子代理隔离通过；`node --check scripts/browser-helper.mjs`通过。
- 未提交、未发布；没有修改tracker或问题.MD。
