# M6 DocStore 依赖、caller 与 owner 地图

基线 `cb6cdc5267ac931b6a4b3b1f82111b995576cb01`，分支 `kanzei/audit-m6-docstore`。复用 A 树，未复制 root dirty。本包只新增 repository/archive/validation 三文件全文；docstore.rs、tracker.rs、actions.rs、scheduling.rs、actions/maintenance.rs 只读/改必要切片，不增加全文覆盖。三全文实现全部读完，无文件内测试；真实内置回归位于 docstore.rs，相关旧/新测试已逐项核对。公共 coverage/inventory/queue 不修改。

## 真源与依赖顺序

`docstore.rs` 聚合 reexport → model/parse/render 的既有 Entry/模板 → repository 的 path、load/save/lock → validation 的账本与 integrity → archive 的净化/先归档后活动 → tools Tracker execute/action/scheduling → app docs_snapshot。模型/解析/渲染只核必要定义与测试，不计全文。`memory/{ledger,store,tools}.rs` 的 MemoryStore 及 app memory/research_library 同名方法属于其他实体，不混入 DocStore。

DocStore 活动 path+DocKind 决定唯一 ID 账：活动 `<name>.md`、同目录 `<name>-archive.md`、`<name>-ids.md`。既有活动路径 kind lock 覆盖三者完整 RMW；req/defect Tracker 先 work-selection 再 kind lock。load shared 同线程可重入；save、archive、void、restore 自保护排他+write_atomic。沿用 cb6 已合 M5 primitive，不增 SQL/锁/schema/deps。next_id 实际生产caller均在 kind 事务内；归档缓存只是 mtime/len 解析投影。保持 D112 archive-first，崩溃可留两份而不留两处皆无。

## 真实公共 API 影响与全部直接生产caller

三处公开 Result 签名已由 root 审核协调：

```rust
pub fn voided_ids(&self) -> std::io::Result<BTreeMap<u32, String>>;
pub fn next_id(&self, entries: &[Entry]) -> std::io::Result<String>;
pub fn integrity_issues(&self, active: &[Entry]) -> std::io::Result<Vec<String>>;
```

| API | 全部 DocStore 生产直接caller（当前源码） | 处理 |
|---|---|---|
| voided_ids | repository.rs151；validation.rs134/174 | `?`，读错零分配/零恢复 |
| next_id | archive.rs110；tracker/actions.rs321 | `?`/ToolOutput::error，分配前阻断 |
| integrity_issues | archive.rs102；tracker.rs470/672；tracker/scheduling.rs42 | `?`/显式错误，区分 I/O 与 Ok 内容问题 |
| void_id | tracker/actions/maintenance.rs31 | 原 Result 输出错误 |
| restore_entry | tracker/actions.rs266 | 原 Result 输出错误 |
| archive_terminal | tracker/actions/maintenance.rs49；app/docs.rs273 | 原 Result 不变，调用方已有错误路径 |

已全仓 rg 搜索调用与定义，包含 tests、注释，按实体过滤；完整原始索引在 ignored `output/parallel-M6/public-callers.txt` 和 `remaining-public-callers.txt`。Result API 既有测试消费者全部显式 unwrap；并发 next_id+save 测试保持原事务。其他 public：open/open_topic/validate_topic 实际由 Tracker/work/doc commands 装配路径；lock/try_lock/load/save 的 owner 已核；archive_file/ledger_file 被 Tracker/archive/测试用于实路径；repair_reused、correct_archived_terminal、dedupe/reconcile/drop/fill 被 Tracker 对应维护/normalize 动作消费；transition_allowed 被 add/update/reopen 流转校验；raw_lines/delete_raw_line 被 raw_list/raw_delete 与回归消费。公开常量为 normalize/archive 净化定义与 UI/工具提示；此包不改这些 API/业务值。

## 可达根因与历史合同

- **P0 一条：编号 SSOT 读取失败当空。** 旧 void_id 读坏 UTF-8 账本两次都吞错，重建 header 覆盖含 D999 的原字节；真实 Tracker void_id 是 repair，旧 execute 豁免完整性门禁。next_id、integrity、restore 也把坏归档/账本当空，ID 复用/写门禁假干净是同根表现。root M5/M6-reproduction 已真实证明 void 覆盖旧账，日志/首次夹具编译错误均保留，本包不冒领重跑。D173 合法缺号需要账本证明，D112 完整性与顺序，R268/D569 写后凭据共同决定失败合同。
- **P2 一条：只有字段净化却未持久化。** archive_terminal 旧 cleaned 比 Entry 数量，去重字段/机制字段变化数量未变且无 terminal 时直接返回。真实 Tracker archive 与 app docs_snapshot 可到；root 真实单 Entry 两说明字段仍为2的断言 FAIL。保持 D316 任意净化变化落盘及 D328 同名不同值/Raw叙事保留。

## 最小修改与失败边界

validation 私有读取仅 NotFound=空，其他错误保留 ErrorKind+路径；void 锁内读一次原文本并纯解析，所有读错零写，合法未知行/幂等保留。repository next_id 成功读全部身份来源才分配；archive repair 用 `?`。Tracker 所有写动作先 I/O 门禁，repair 只豁免 Ok 内容 issues；append_progress 同样零准入。

Tracker private finalize_action_result 复用原收尾，不添加公开 hook。action 成功事实与 postcheck 错误输出分开：post-I/O 明确“已写入但复核失败/勿盲目重试”，仍先记录真实合法写凭据；Ok 内容 issues 仍仅 warning；write_log 自身失败保持原错误合同。回归确定性地先实际 save，再注入坏账本后调用该生产 private 收尾，检查文件字节/mtime、日志指纹和错误。archive cleaned 比完整 Entry 值，仍先归档 write_atomic 再 save 活动。

root 已批准 maintenance.rs archive 必要切片：lower archive_terminal 成功后，调用 TrackerTool 私有 finalize_archive_move，仍逐个核对 moved ID（包括最后条目）。I/O 与实际缺ID分开诊断；内部复核失败先调用同一私有 record_action_writes 记录真实 active/archive 指纹，再明确已写入但复核失败、禁止盲重试。正常 archive 只由统一 finalizer 记录一次，不复制写日志代码、不加公开测试 hook。

## 回归与证据边界

5个 lower 实际内置回归：坏账本原字节、NotFound/未知合法文本/幂等/恢复、坏归档/账本 UTF8+目录矩阵、repair_reused 零写、无 terminal 字段净化重开持久性。6个实际 Tracker 回归：raw_delete/普通及 repair/normalize/append_progress I/O 门禁、已写后复核失败成功凭据且不二写、Ok 内容 warning 与凭据、真实 archive 净化持久+幂等、内部归档I/O/缺最后ID失败实际两路径凭据、不重复正常两路径凭据。

当前 std-only39 PASS含5新增lower；真实Cargo11新增PASS。7等价旧production控制（保留新Result包装/私有支架，非完整旧源码）真实assert FAIL、7合法内容控制PASS，全8源码finally原字节+raw/LF SHA复原。恢复后memory189 PASS/0、tools790 PASS/0/3ignored；memory doctest0/0/1ignored、tools0/0/0。memory/tools/app all-targets check、Clippy -D warnings、fmt、diff全exit0。首次standalone接线失败/runner准备anchor错误保留；树内TEMP祖先Git旧fixture失败188/1、MiKTeX首轮新profile pdflatex.fmt缓存失败789/1/3原日志与原因保留，树外自建TEMP/工具初始化后对应旧test及同包全量成功。无真实用户配置读取。

全workspace/native/UI由root整合；只读必要caller不计全文，两个app gen/schemas构建换行产物按开始基线checkout原字节恢复不入包。

## 当前 hash

- `crates/kanzei-memory/src/docstore/repository.rs`（full，174行）：SHA256 `7aea99afb824b35750cbf041c46d7098cc7a7d637aefe2f6a16e9546df3b9f46`；SHA256_LF `1ea3783180e4fb2e50c8707f65b8d37e450ea62c9a015715dbb11323a6a8268e`；baseline LF `62b6d37e964a0fb91f11751fcd59ffa3bece685e852fcfe608d4f16f469572a8`。
- `crates/kanzei-memory/src/docstore/archive.rs`（full，595行）：SHA256 `c12b0f9d5a93a8af633bd52dd434f566abd3f6a5612182674ceccf76d4e4b437`；SHA256_LF `a3708108d5ef4e761c13edd667d1d3df8e61f0d121bcae42a51378555d123d65`；baseline LF `13d9ed19842889749246b43efff6c71ed276fd693d51bc6107b0eb99c797dbae`。
- `crates/kanzei-memory/src/docstore/validation.rs`（full，404行）：SHA256 `da036dee94b6766b54e138ba1d292964364c02c1ad6225be301e953845f9a3f9`；SHA256_LF `db2bda572ca4593e6cb8fd6aad71669c830cec5af5ab70a20d6d8d26b938af37`；baseline LF `e3b7be0fbf1ac7ba3f320ac2a1a9bd35a5c4f0007aa938aee5e2fc412048b03d`。
- `crates/kanzei-memory/src/docstore.rs`（necessary，1464行）：SHA256 `2ae561fbdb182da53efc7c168526737660affaf2754200988aebf659fc058c31`；SHA256_LF `2ae561fbdb182da53efc7c168526737660affaf2754200988aebf659fc058c31`；baseline LF `5c7c297ff9dc6fe81ddde9983747d2d4ee58b94676099f81320800aacb6451ee`。
- `crates/kanzei-tools/src/tracker.rs`（necessary，5578行）：SHA256 `80def7af05c7eadc598916631d0075fe7e26f8bb461b9333837860a1295a519b`；SHA256_LF `80def7af05c7eadc598916631d0075fe7e26f8bb461b9333837860a1295a519b`；baseline LF `47fbb6751463004e8ee72fe92a9ef970f82364cd49ea496ba8c02050ac309f9e`。
- `crates/kanzei-tools/src/tracker/actions.rs`（necessary，865行）：SHA256 `815dd63a2207255e2070c5da356e7ee78c64582aafba5297c0e3d09189ce8e00`；SHA256_LF `5d5e7b6f9033fd0dfafd3ae054c64a0bfe669a1f767e6586e1b89b50e957b208`；baseline LF `cb7d6813aea7981c242d3a9a2fbf6dc28377f2fe63c8fabb0e13b92833fe1204`。
- `crates/kanzei-tools/src/tracker/scheduling.rs`（necessary，1121行）：SHA256 `d7fc014a37cd534a5d5b1d877b4f11825b5b99b61eb06f1feb9bd58c5f26e369`；SHA256_LF `ecb1a6bfe3e1230f8b03209cf84fbfd82b50a16637c211dbbe83664bcf760fbd`；baseline LF `f43966e889c5963b265203dd94d2341d46c723cdfd4db1476e93ee5f6149a630`。
- `crates/kanzei-tools/src/tracker/actions/maintenance.rs`（necessary，212行）：SHA256 `7e683647e790e4b558579a320788bad8e650cacac7b3fa59533bf58d0e60a5c0`；SHA256_LF `7e683647e790e4b558579a320788bad8e650cacac7b3fa59533bf58d0e60a5c0`；baseline LF `c276856186cab03b84dc1c916f8db51ce396d0c00af868a18240492bd162adbf`。
