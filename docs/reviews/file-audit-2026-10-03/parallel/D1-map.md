# D1 配置链只读依赖与修复边界

后续 lower 实现与真实验证已在 `kanzei/audit-d1` 隔离树完成，状态以 D1.md / D1-verification.json 为准。以下保留 root 最初只读证据，不能把其“未改源码/未执行”描述当成后续实现状态；实际 app writer 和删除/查询/UI 问题仍待 root M3。

观察 HEAD：`f0dd9d57b56de97dde1ea24cc60329ca1d04e972`。共享树含 root 的未提交修改；D1 未改生产源码、未运行 Cargo、未 stage/commit。完整原字节 SHA256 与 LF 归一 SHA256、公开符号及全仓库词法命中见 `output/parallel-D1/evidence.json`。词法命中不能替代调用关系，也不扩大全文覆盖。

## 全文范围与既有覆盖

已核对 `coverage.json.full_file_reviews`：下面八文件均没有旧全文记录。已按子模块→配置加载→权限写入顺序全文阅读，包括文件内测试；本图不修改 coverage，不把待执行回归记成通过。

| 文件 | 全文状态 | 结论 |
|---|---|---|
| crates/kanzei-harness/src/config/cadence.rs | 已全文 | 显式 verify_every_n 漏 overlay，D1-C2 |
| crates/kanzei-harness/src/config/embeddings.rs | 已全文 | 未见真实可达问题，静态 PASS |
| crates/kanzei-harness/src/config/web.rs | 已全文 | 未见真实可达问题，静态 PASS |
| crates/kanzei-harness/src/config/limits.rs | 已全文 | 未见真实可达问题，静态 PASS |
| crates/kanzei-harness/src/config/models.rs | 已全文 | 未见真实可达问题，静态 PASS；Anthropic 专属行为暂缓 |
| crates/kanzei-harness/src/config/permissions.rs | 已全文 | 未见真实可达问题，静态 PASS |
| crates/kanzei-harness/src/config.rs | 已全文，包括全部内置测试 | loader 调用缺字段的 cadence overlay；D1-C2 同一根因 |
| crates/kanzei-harness/src/permission_persist.rs | 已全文 | D1-C1 无共同写事务；D1-C3 拒绝合法规则数组 |

`settings.rs`、`model_config.rs`、mobile/CLI/运行器/UI 为实际调用链切片，不能算全文。base/atomic_file.rs 与 atomic_file/lock.rs 已有全文覆盖，本轮只核实所需 API，不重复计数。D0 文件冻结，不在 D1 改动范围。

## 路径、配置读者与 owner

用户所说 config.toml 在本仓库实际文件名为 **kanzei.toml**；源码没有第二套字面量 config.toml 配置真源。

- 全局路径：`kanzei_home()/kanzei.toml`；项目路径：主根目录 `/.kanzei/kanzei.toml`。不读取真实用户文件或凭据，只读路径构造与源码夹具。
- `KanzeiConfig::load/load_with_warnings` 先发现主根，`load_at_root/load_with_warnings_at_root` 接受明确主根；合并 global→project→fill_defaults。缺失文件允许，文件读取或类型解析错误传播，未知键为 warning。
- CLI run/config、app run/state/model_config/settings、tools 装配及 memory/embeddings 等经公共 loader 消费有效配置；resolve_model/resolve_model_chain 和 Limits/Web/Permissions 访问器只消费内存配置，不是持久 owner。全部公开方法/类型符号已全仓库搜，命中清单存在 evidence.json，含实现、测试与间接 import。
- settings_get 的 effective 与项目模型配置由 main_root/load_merged/load_at_root 路径读取；设置表单全局原文由 settings_read_document 读取。查询侧个别 fallback 吞错误不等于 loader 吞错误。
- 项目导出 `projects.rs` 只复制配置到导出目录；git_init 的 .gitignore 文本不是配置写者。

## 全部识别出的持久写者与锁

| 写入口 | 实际路径/调用方 | 当前提交方式与锁 |
|---|---|---|
| permission_persist::append_allow_rule:48 | project/.kanzei/kanzei.toml；桌面 commands/run::persist_always_allow、permission_rule_add；CLI cli/memory::persist_always_allow；mobile/approvals::remember_always_allow 经桌面 helper | read→typed 预检→DocumentMut 追加→fs::write:89；无共同文件锁/原子提交 |
| model_config::project_models_save:775 → save_project_models_file:733 | main_root 项目配置；UI16 项目模型弹窗 | 读改文档→第二次读比较→settings_write_document；最后复读与写之间无锁，仍有覆盖窗口 |
| settings_save:717 → settings_save_at_path_impl:690 | 仅全局配置；UI16 设置表单 | DocumentMut 逐键改→settings_write_document:273→fs::write:283；无文件事务锁 |
| settings_open:792 → settings_bootstrap_file:728 | 仅缺失的全局配置，铺注释模板 | 检查不存在后普通写入；检查后文件出现可被覆盖，属 D1-C1 同一事务边界 |
| settings::permission_rule_delete:826 | main_root 项目配置；UI16 设置列表、UI07 “已记住”撤销 | 最新 typed config 按旧 index 移除→整文件 serde→fs::write:840；无文件锁/期待规则身份 |
| app/files_edit::write_at:469 | 用户可编辑 .kanzei/kanzei.toml | FileLock + expected hash + write_atomic_cas，已具备协作写入保护 |
| tools/write、tools/edit 及 bash/外部编辑器 | 通用文件写者，可到相同配置路径 | tools 文本写采用文件锁/guarded 写；bash/外部程序不承诺协作锁，需保留冲突检测，不能声称锁解决所有外部竞态 |

mobile 的 runtimes/asks Mutex 是运行态 owner，桌面 AppState 与 CLI 不共享它；不是配置文件锁。model_config 的二次读取不是原子 CAS。

底层 owner 已存在：`kanzei-base::atomic_file::{FileLock, write_atomic, write_atomic_cas}`。锁路径依据规范化目标文件（不存在时规范化父目录），同线程重入，非 Send，限时等待；不能跨 await 持有。write_atomic 用同目录临时文件、flush/sync、rename，失败不先截断真源；write_atomic_cas 在共同锁内核验期望 hash。单独替换 fs::write 为原子写不能防止两个旧快照相互覆盖。新配置 lower primitive 应集中路径、读取、语义/文档校验、同路径锁、修改和原子提交；root app 入口复用，避免各写者自己造不同锁。

## 静态确认问题与最小边界

以下是 **4 个 P1 根因**，不是按表现累计。没有执行回归，均未修复。

### D1-C1 / P1：共同配置读改写事务缺失

实际路径：CLI “a”/桌面 always/mobile “允许并记住”→append_allow_rule，与 project_models_save 保存同一个项目文件。模型保存第二读完成后，权限追加读旧文件并成功提交规则；模型再提交其旧文档，规则消失；双方返回成功。两个 append 也可读到同一旧文件后各写一条，最后只保留其中一条。raw write 失败还可留下被截断的配置，影响下次所有 loader。settings/global/bootstrap/delete 的相同写边界一并协调；不单列每个表现。

最小修复：统一路径的 FileLock 包住完整读取、校验、修改、提交；使用已有原子写原语。模型保存必须在同一 owner 内重读并套 patch，不能在外面生成旧 doc 后仅锁最后一写。缺失模板需在锁内再次检查存在。保留 type error 拒绝、未知字段、精确授权资源和失败不批准的契约。

回归边界：两个独立 append 都保留；模型 patch 与权限追加都保留；解析/提交失败保持原字节并返回错误；缺失创建竞争不覆盖新出现文件。锁必须由所有这些专用写者共享，泛型写者已有协作锁时兼容其路径。

### D1-C2 / P1：显式 verify_every_n 被层叠加载忽略

`config.rs:375–381` 取 raw cadence 显式键，调用 `cadence.rs:90–110 overlay_cadence`；函数没有 verify_every_n 分支，而已知键清单包含它。全局或项目写 `[cadence] verify_every_n = 3`，loader 仍返回初始值 0。实际 `app/run/coordinator.rs:577–580` 取加载结果；`harness/auto_run.rs:430–431` 要求 >0 才触发，故显式开启无效。R-144 历史要求 N 可配置。

最小修复：仅增加 written.contains("verify_every_n") 对该字段 overlay；不改变缺省值，Default=0 与 serde 缺字段=3 的设计分歧不纳入此次判断/修改。回归：全局显式非零生效、项目显式值覆盖、项目缺该键保留全局、项目显式 0 确实关闭。

### D1-C3 / P1：合法 inline rules 数组无法“允许并记住”

config_reference 明确展示 `rules = [{ action, resource, effect }, ...]`，KanzeiConfig 的 Vec<Rule> 接受该格式。append_allow_rule typed 预检能通过，但接着 `rules.as_array_of_tables_mut()` 必定失败，因为这是 Value::Array 而非 ArrayOfTables。三个入口实际调用相同 helper：CLI/桌面返回 Deny+保存错误，mobile 报错保留 ask；合法配置因此不能完成授权记忆。

最小修复：lower primitive 的文档编辑同时支持 inline 数组和 ArrayOfTables，保留现有未知键与文档内容；错误类型仍拒绝。回归同一 helper 下两种合法格式均追加成功，重读 Rule 值准确且不扩大 resource；非法格式不改文件。

### D1-C4 / P1：权限删除未携带规则/项目身份

settings permission_rules_get 返回 index/action/resource/effect，但 delete 只接 project_dir+index。UI16 两个旧列表都见 A(index0),B(index1)：第一次删除 A，第二次仍以 A 的 index0 删除，服务器最新 index0 已是 B，返回成功。即使文件锁完备，该顺序场景依然存在。UI07 撤销虽先重读匹配 action/resource，随后仅传 index，同样有窗口。

UI16:241–247 loadPermissionRules 直接渲染异步返回，没有捕获项目/请求代际核验；201 的删除取 click 时 currentProject，232–234 确认框 await 后没有身份复验。实际项目切换 owner `09-sessions.js:913–927 activate_execution_root` 更改 currentProject；旧请求返回或旧 rule 闭包可组合新项目。该证据是同一删除身份根因的扩展条件，不新增计数。

最小修复：后端命令需要期望 action/resource/effect 或文件版本+规则身份，并在同一写事务内校验，不匹配明确冲突；UI16/07 两个调用方同步。列表请求绑定项目和代际，旧响应丢弃；delete 使用渲染时项目，在确认返回和重试前核验。回归旧索引错位拒绝、保留 B，两个项目规则不交叉，迟到旧响应不渲染。

### D1-C5 / P2：规则查询把读取/解析失败伪装成空列表

settings.rs:812–815 `.ok().and_then(...ok()).unwrap_or_default()` 将权限文件读取失败或非法 TOML 返回 Ok(empty)。UI16:249–250 的读取错误 toast/retry 不会被触发，用户看见“没有权限规则”，无法区别规则不存在与配置坏了。这是读失败 contract，与 C4 删除身份不同；不属于 P1 根因计数。

最小修复：仅 NotFound 返回空；其他 IO 错误及类型/语法错误传播明确路径错误。回归缺文件为空、坏 TOML/error 返回 Err。不把当前空表 UI 的样式当问题。

## 历史决策与否定判断

- D-083（defects-archive.md:509–517）：记住权限只在持久成功后批准，保留注释/未知字段。tier1_implementation_plan 同样要求精确资源；不得修为扩大授权或忽略写失败。
- D-082（defects-archive.md:498）：settings_save 原先丢表单外业务字段；当前以 DocumentMut 改管理键，未见该旧 bug 回归。models 未管理的 web_extract、limits/cadence 表单外键、未知字段保留。providers 完整清单删除未列出的子表是明确契约，不当作无意丢失。
- D-245 与 R-144：cadence 五旧字段与验收核查 N 分别有历史消费者；C2 仅按显式 N 不生效判定，未用默认争议放大结论。
- permission_rule_delete 的整文件 serde 会丢注释/未知键；仅丢注释不记 bug，本轮未证明被丢未知键有当前可达业务消费者，不另列问题。
- `output/parallel-M2/next-config-findings.md` 仅作为搜索提示；上面的调用/行为都已独立核实。未读取原有 问题.MD 或真实 kanzei.toml。

## 后续最小 scope 分工建议

- D1 lower：`harness/config/cadence.rs` 显式 overlay；`harness/permission_persist.rs` 加入共同配置文档事务及双格式 append；`harness/config.rs` 仅 reexport/真实 loader 与 append 回归（如 root 决定公共 API 放此处）。不改变持久格式或新增依赖。
- root M3：`app/settings.rs`（global/bootstrap/query/delete）、`app/model_config.rs`（事务内 patch）、`ui/16-settings.js` 与 `ui/07-events.js`（身份契约）。app mobile/CLI 既有 append helper 调用不应各自造新的锁。
- base 原语不需重写。统一 lower primitive API/owner 等 root 确认；当前所有源码冻结、只读结束。

## 初次只读验证状态（历史）

已完成：八文件全文与内置测试阅读；coverage 无重叠核对；全部公开符号词法检索及实际入口切片；writer/primitive owner 检查；两种 SHA256 记录。未执行：Rust 编译、Cargo 测试、并发回归、UI 回归。未增加 coverage、未标已修复、没有将别人的测试执行记为 D1 通过。

## lower 后续验证（2026-10-03）

独立 D1 profile 隔离全部应用/用户/Cargo/Rustup 配置路径；仅复用 registry 与 toolchains 缓存目录。共享 root target 独占使用，验证开始与负例恢复后分别刷新本树420个 Rust 源码的 mtime。

- 四个定向回归各1 passed；完整 harness 在负例前、恢复后各207 passed，doc-tests 0。
- 三个等价旧逻辑负例各运行单个真实回归，编译成功后断言失败（exit101）；finally 原字节恢复及三个源码 SHA256 一致。
- `cargo check --workspace --all-targets`、`cargo clippy --workspace --all-targets -- -D warnings`、`cargo fmt --all -- --check`、`git diff --check` 全部 exit0。
- 首次锁探针断言使用错误返回语义，已改正并保留失败记录、完整重跑。没有把该失败或编译错误当成负例命中。
- lower C1 验证不代表 app settings/model_config 已共享该 owner；整链修复与 C4/C5 仍待 root M3。公共 coverage 未改，本次没有执行整仓 Rust 测试或 UI 回归。
