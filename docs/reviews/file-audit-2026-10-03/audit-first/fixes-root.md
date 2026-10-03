# Git、解析与工作台修复记录

整合状态：已在 dev 分组提交，未发布；提交与独立快照验证见 [修复汇总](fixes-summary.md)。

审计基线 55eaca24。12 项确认问题：P0 2、P1 5、P2 5。原始审计记录保留；本记录追踪修复结果。

## crates/kanzei-tools/src/git.rs

### 职责

拥有Git子进程、源版本指纹及工作树合并操作。

### 判断

P0

### 确切问题

- AF-T02：stage/commit_plan/finalize/test_record 对约 5000 个源文件计算 endorsement fingerprint。 父进程写满 git stdin，git 写满 stdout；父进程等写完才 wait_with_output 排空 stdout，形成永久相互等待，命令无法完成。

- AF-T11：A 线调用 git merge_ff，into 分支当前被另一个活跃工作树 B checkout；B 仍在运行内部 write/edit 或 Git 工具。 工具声明的排他范围是 A 的 ctx.cwd，但 merge_ff 实际改 B 的 HEAD、index 和文件；A/B 两个内部 run 可同时写 B，绕过一树一写者语义。

### 修改

- 同一 Git 子进程并行供 stdin 与排空 stdout/stderr，等待写线程并收割子进程，错误向上传递。

- 在源树固定 source OID；跨树 merge 获取实际目标树 writer 租约，忙时撤销 waiter 返回错误；持租约重查分支并完成 merge。

### 影响范围

- 直接 caller / 关联文件：crates/kanzei-tools/src/git.rs, crates/kanzei-harness/src/orchestration.rs, crates/kanzei-harness/src/tool.rs, crates/kanzei-tools/src/git/tool.rs, crates/kanzei-app/src/run/assembly.rs, crates/kanzei/src/cli/run.rs

- 保留持久格式和业务语义；ToolCtx 新 coordinator 字段由应用与CLI入口显式传入，Git merge 的动态目标采用 Exclusive 调度。其他接口调用方式不变。

### 验证

- 5000源码路径两条公开指纹路径均结束且一致；缺失文件返回错误。

- 目标忙时无变化、无遗留 waiter；释放后从源 HEAD 正确合并；分叉失败释放目标租约。

## crates/kanzei-harness/src/orchestration.rs

### 职责

定义项目读写执行协调器合同。

### 判断

P0

### 确切问题

- AF-T11：A 线调用 git merge_ff，into 分支当前被另一个活跃工作树 B checkout；B 仍在运行内部 write/edit 或 Git 工具。 工具声明的排他范围是 A 的 ctx.cwd，但 merge_ff 实际改 B 的 HEAD、index 和文件；A/B 两个内部 run 可同时写 B，绕过一树一写者语义。

### 修改

- 在源树固定 source OID；跨树 merge 获取实际目标树 writer 租约，忙时撤销 waiter 返回错误；持租约重查分支并完成 merge。

### 影响范围

- 直接 caller / 关联文件：crates/kanzei-harness/src/orchestration.rs, crates/kanzei-harness/src/tool.rs, crates/kanzei-tools/src/git.rs, crates/kanzei-tools/src/git/tool.rs, crates/kanzei-app/src/run/assembly.rs, crates/kanzei/src/cli/run.rs

- 保留持久格式和业务语义；ToolCtx 新 coordinator 字段由应用与CLI入口显式传入，Git merge 的动态目标采用 Exclusive 调度。其他接口调用方式不变。

### 验证

- 目标忙时无变化、无遗留 waiter；释放后从源 HEAD 正确合并；分叉失败释放目标租约。

## crates/kanzei-harness/src/tool.rs

### 职责

承载工具执行身份和共享协调器。

### 判断

P0

### 确切问题

- AF-T11：A 线调用 git merge_ff，into 分支当前被另一个活跃工作树 B checkout；B 仍在运行内部 write/edit 或 Git 工具。 工具声明的排他范围是 A 的 ctx.cwd，但 merge_ff 实际改 B 的 HEAD、index 和文件；A/B 两个内部 run 可同时写 B，绕过一树一写者语义。

### 修改

- 在源树固定 source OID；跨树 merge 获取实际目标树 writer 租约，忙时撤销 waiter 返回错误；持租约重查分支并完成 merge。

### 影响范围

- 直接 caller / 关联文件：crates/kanzei-harness/src/orchestration.rs, crates/kanzei-harness/src/tool.rs, crates/kanzei-tools/src/git.rs, crates/kanzei-tools/src/git/tool.rs, crates/kanzei-app/src/run/assembly.rs, crates/kanzei/src/cli/run.rs

- 保留持久格式和业务语义；ToolCtx 新 coordinator 字段由应用与CLI入口显式传入，Git merge 的动态目标采用 Exclusive 调度。其他接口调用方式不变。

### 验证

- 目标忙时无变化、无遗留 waiter；释放后从源 HEAD 正确合并；分叉失败释放目标租约。

## crates/kanzei-tools/src/git/tool.rs

### 职责

解析Git工具输入并声明调度方式。

### 判断

P0

### 确切问题

- AF-T11：A 线调用 git merge_ff，into 分支当前被另一个活跃工作树 B checkout；B 仍在运行内部 write/edit 或 Git 工具。 工具声明的排他范围是 A 的 ctx.cwd，但 merge_ff 实际改 B 的 HEAD、index 和文件；A/B 两个内部 run 可同时写 B，绕过一树一写者语义。

### 修改

- 在源树固定 source OID；跨树 merge 获取实际目标树 writer 租约，忙时撤销 waiter 返回错误；持租约重查分支并完成 merge。

### 影响范围

- 直接 caller / 关联文件：crates/kanzei-harness/src/orchestration.rs, crates/kanzei-harness/src/tool.rs, crates/kanzei-tools/src/git.rs, crates/kanzei-tools/src/git/tool.rs, crates/kanzei-app/src/run/assembly.rs, crates/kanzei/src/cli/run.rs

- 保留持久格式和业务语义；ToolCtx 新 coordinator 字段由应用与CLI入口显式传入，Git merge 的动态目标采用 Exclusive 调度。其他接口调用方式不变。

### 验证

- 目标忙时无变化、无遗留 waiter；释放后从源 HEAD 正确合并；分叉失败释放目标租约。

## crates/kanzei-app/src/run/assembly.rs

### 职责

组装桌面会话运行上下文。

### 判断

P0

### 确切问题

- AF-T11：A 线调用 git merge_ff，into 分支当前被另一个活跃工作树 B checkout；B 仍在运行内部 write/edit 或 Git 工具。 工具声明的排他范围是 A 的 ctx.cwd，但 merge_ff 实际改 B 的 HEAD、index 和文件；A/B 两个内部 run 可同时写 B，绕过一树一写者语义。

### 修改

- 在源树固定 source OID；跨树 merge 获取实际目标树 writer 租约，忙时撤销 waiter 返回错误；持租约重查分支并完成 merge。

### 影响范围

- 直接 caller / 关联文件：crates/kanzei-harness/src/orchestration.rs, crates/kanzei-harness/src/tool.rs, crates/kanzei-tools/src/git.rs, crates/kanzei-tools/src/git/tool.rs, crates/kanzei-app/src/run/assembly.rs, crates/kanzei/src/cli/run.rs

- 保留持久格式和业务语义；ToolCtx 新 coordinator 字段由应用与CLI入口显式传入，Git merge 的动态目标采用 Exclusive 调度。其他接口调用方式不变。

### 验证

- 目标忙时无变化、无遗留 waiter；释放后从源 HEAD 正确合并；分叉失败释放目标租约。

## crates/kanzei/src/cli/run.rs

### 职责

组装CLI运行上下文。

### 判断

P0

### 确切问题

- AF-T11：A 线调用 git merge_ff，into 分支当前被另一个活跃工作树 B checkout；B 仍在运行内部 write/edit 或 Git 工具。 工具声明的排他范围是 A 的 ctx.cwd，但 merge_ff 实际改 B 的 HEAD、index 和文件；A/B 两个内部 run 可同时写 B，绕过一树一写者语义。

### 修改

- 在源树固定 source OID；跨树 merge 获取实际目标树 writer 租约，忙时撤销 waiter 返回错误；持租约重查分支并完成 merge。

### 影响范围

- 直接 caller / 关联文件：crates/kanzei-harness/src/orchestration.rs, crates/kanzei-harness/src/tool.rs, crates/kanzei-tools/src/git.rs, crates/kanzei-tools/src/git/tool.rs, crates/kanzei-app/src/run/assembly.rs, crates/kanzei/src/cli/run.rs

- 保留持久格式和业务语义；ToolCtx 新 coordinator 字段由应用与CLI入口显式传入，Git merge 的动态目标采用 Exclusive 调度。其他接口调用方式不变。

### 验证

- 目标忙时无变化、无遗留 waiter；释放后从源 HEAD 正确合并；分叉失败释放目标租约。

## crates/kanzei-app/ui/05-chat-render.js

### 职责

渲染对话消息、错误卡和重试入口。

### 判断

P1

### 确切问题

- AF-R01：A 对话失败后，在 B 发送另一条请求，再回 A 点原错误卡重试。 把 B 的提示和附件发进 A，重试不再对应失败请求。

### 修改

- 错误卡捕获原请求、附件与会话，重试使用该快照并验证当前会话。

### 影响范围

- 直接 caller / 关联文件：crates/kanzei-app/ui/05-chat-render.js

- 保留持久格式和业务语义；ToolCtx 新 coordinator 字段由应用与CLI入口显式传入，Git merge 的动态目标采用 Exclusive 调度。其他接口调用方式不变。

### 验证

- A请求失败后全局lastRequest切到B，重试仍发A；当前B拒绝旧卡执行。

## crates/kanzei-app/ui/28-async-workspace.js

### 职责

展示和操作指定项目会话的异步问题与日志。

### 判断

P2

### 确切问题

- AF-R02：从 A 打开到 B，B 列表尚未返回时点击仍显示的 A 终端操作。 请求携带 B 项目/会话和 A 终端 ID，合法操作被后端归属校验拒绝；未证明误停别的进程。

### 修改

- 换scope立即清旧内容；行操作闭包捕获原scope；迟到结果仅更新仍匹配的界面。

### 影响范围

- 直接 caller / 关联文件：crates/kanzei-app/ui/28-async-workspace.js

- 保留持久格式和业务语义；ToolCtx 新 coordinator 字段由应用与CLI入口显式传入，Git merge 的动态目标采用 Exclusive 调度。其他接口调用方式不变。

### 验证

- 旧A日志行在切B后仍提交A，不会改B数据。

## crates/kanzei-app/ui/06-activity.js

### 职责

呈现工具活动及双栏代码差异。

### 判断

P2

### 确切问题

- AF-R03：插入/删除后，同一上下文行 old_line=10、new_line=12，切换为分栏显示。 右列仍显示 10，用户按错误行号定位新代码。

### 修改

- 分栏diff按左右pane分别选old_line/new_line。

### 影响范围

- 直接 caller / 关联文件：crates/kanzei-app/ui/06-activity.js

- 保留持久格式和业务语义；ToolCtx 新 coordinator 字段由应用与CLI入口显式传入，Git merge 的动态目标采用 Exclusive 调度。其他接口调用方式不变。

### 验证

- 上下文行旧2/新5时，右栏显示5。

## crates/kanzei-app/ui/20-lines.js

### 职责

执行工作线检查并控制合并入口。

### 判断

P1

### 确切问题

- AF-R04：工作树检查先成功，再点击重新检查；第二次调用失败/抛异常后点击合并。 仍走旧绿色状态，无失败检查确认就调用 worktree_merge；检查运行中也未撤销旧结果。

### 修改

- 重跑检查先清旧通过状态；运行时禁用合并，合并处理器也拒绝执行；失败不保留绿色结论。

### 影响范围

- 直接 caller / 关联文件：crates/kanzei-app/ui/20-lines.js

- 保留持久格式和业务语义；ToolCtx 新 coordinator 字段由应用与CLI入口显式传入，Git merge 的动态目标采用 Exclusive 调度。其他接口调用方式不变。

### 验证

- 旧成功后开始重跑：按钮禁用，直接调用也无法绕过；检查失败后不能合并。

## crates/kanzei-core/src/runner/schema_check.rs

### 职责

从模型正文提取完整JSON并进行结构校验。

### 判断

P1

### 确切问题

- AF-R05：子代理输出 Results: [{"ok":true},{"ok":false}]，期望 schema 为 array。 提取为首个 object，合法数组被打回；object schema 还可能错误接受丢失其余数据的子对象。

### 修改

- 按正文顺序扫描顶层JSON对象或数组，跳过非JSON平衡前缀；不把外层数组缩成内部对象。

### 影响范围

- 直接 caller / 关联文件：crates/kanzei-core/src/runner/schema_check.rs

- 保留持久格式和业务语义；ToolCtx 新 coordinator 字段由应用与CLI入口显式传入，Git merge 的动态目标采用 Exclusive 调度。其他接口调用方式不变。

### 验证

- 对象数组、嵌套数组、字符串内括号、[note]前缀回归；core全量。

## crates/kanzei-core/src/runner/tool_failure_telemetry.rs

### 职责

将工具结果归类为有含义的故障诊断。

### 判断

P2

### 确切问题

- AF-R06：read 成功读取普通英文或 schema 内容，其中出现 required。 诊断 events 仍写入 missing_parameter，产生错误诊断记录。另一任务本轮已修改计数：success 不再增加 failure_count，原关闭失败率污染影响已缓解，不能继续算作当前影响。

### 修改

- 保留空搜索诊断；其他文本诊断只分析错误结果，成功文件正文不被解释为失败。

### 影响范围

- 直接 caller / 关联文件：crates/kanzei-core/src/runner/tool_failure_telemetry.rs

- 保留持久格式和业务语义；ToolCtx 新 coordinator 字段由应用与CLI入口显式传入，Git merge 的动态目标采用 Exclusive 调度。其他接口调用方式不变。

### 验证

- 成功正文含required/path not found/permission denied无误报；真实错误与空搜索仍被识别。

## crates/kanzei-app/ui/24-preview.js

### 职责

控制预览面板及向对话添加截图。

### 判断

P1

### 确切问题

- AF-R07：A 点击截图放进输入框，截图返回前切换到 B。 A 截图追加到 B 的附件数组；与普通 FileReader 已有的 target 身份守卫不一致。

### 修改

- 截图捕获时记录附件容器身份，完成时身份变化则丢弃迟到截图及错误提示。

### 影响范围

- 直接 caller / 关联文件：crates/kanzei-app/ui/24-preview.js

- 保留持久格式和业务语义；ToolCtx 新 coordinator 字段由应用与CLI入口显式传入，Git merge 的动态目标采用 Exclusive 调度。其他接口调用方式不变。

### 验证

- A截图等待中切到B不会给B加附件；当前会话截图仍成功。

## crates/kanzei-app/ui/19-arch.js

### 职责

解析并显示项目架构索引。

### 判断

P2

### 确切问题

- AF-R08：索引使用普通 [example.md](../../../docs/design/example.md)，不把标签包在反引号中。 解析器标记已入索引，却不加入分组，未入册兜底也排除，文档从树里消失。

### 修改

- 索引统计与渲染复用相同链接正则；支持反引号标签和无标题平铺索引。

### 影响范围

- 直接 caller / 关联文件：crates/kanzei-app/ui/19-arch.js

- 保留持久格式和业务语义；ToolCtx 新 coordinator 字段由应用与CLI入口显式传入，Git merge 的动态目标采用 Exclusive 调度。其他接口调用方式不变。

### 验证

- 平铺索引及带反引号链接均显示。

## crates/kanzei-tools/src/conventions.rs

### 职责

按CAS修改项目约定文本。

### 判断

P1

### 确切问题

- AF-R10：规范 patch 替换内容含 Windows 路径 C:\new\rules 或字面量 \n。 路径中的 \n、\r 被改成换行，实际保存内容错误；旧文本也可能无法匹配而拒绝合法 patch。

### 修改

- 移除serde解码后的第二次转义处理，保留用户JSON字符串里的字面反斜线。

### 影响范围

- 直接 caller / 关联文件：crates/kanzei-tools/src/conventions.rs

- 保留持久格式和业务语义；ToolCtx 新 coordinator 字段由应用与CLI入口显式传入，Git merge 的动态目标采用 Exclusive 调度。其他接口调用方式不变。

### 验证

- 通过真实tool/CAS修改Windows路径与反斜线n，实际存储内容完全匹配。

## crates/kanzei-tools/src/architecture.rs

### 职责

校验架构索引链接、覆盖范围和更新合同。

### 判断

P2

### 确切问题

- AF-R11：同一 example.md 分别以 ../../../docs/design/example.md 和 ../../../docs/design/./example.md 入索引。 校验显示零问题，允许同一实体重复入册；compliant 却归一为一份，内部身份口径不一致。

### 修改

- 重复索引、合规保留与目录归属使用同一project_rel身份；命名检查保留原始大小写。

### 影响范围

- 直接 caller / 关联文件：crates/kanzei-tools/src/architecture.rs

- 保留持久格式和业务语义；ToolCtx 新 coordinator 字段由应用与CLI入口显式传入，Git merge 的动态目标采用 Exclusive 调度。其他接口调用方式不变。

### 验证

- 重复别名拒绝；docs/./design等价更新真实CAS成功；原有BadName命名拒绝保留。11项架构测试通过。

# Module Summary

## 已修复

- P0：Git双管道死锁、实际目标树写互斥。

- P1：错误卡重试、合并检查旧状态、JSON顶层数组、截图归属、字符串二次转义。

- P2：异步工作台scope、diff行号、成功正文诊断、架构索引渲染、重复路径身份。

## PASS 文件

- 未修改原审计PASS文件；关联caller按最小合同传播。

## 仍需人工判断

- 无。

## 依赖影响

- harness coordinator → app/CLI ToolCtx → Git目标租约；解析和UI调用方保留原接口。

## 剩余风险

- 目标树互斥复用应用既有进程内 coordinator；没有新增跨进程工作树互斥机制。

- 共享文件里另任务的 workflow outcome 和 tracker 改动保留但不归本修复提交。
