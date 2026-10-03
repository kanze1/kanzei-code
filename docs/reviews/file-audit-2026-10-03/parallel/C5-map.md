# C5依赖与状态地图

开始基线07c0d373，分支kanzei/audit-c5-terminal，复用audit-c0树。全文仅app/run/persistence.rs；core typed/inbox/session、app coordinator/commands和依赖声明为必要重开切片，不能增加全文覆盖。没有读取真实用户DB/凭据，没有push/release，没有改global coverage。

## 依赖方向与事实owner

SQLite/schema/events原语
→ SessionStore的input/status原语（inbox/session）
→ TypedSessionWriter事实不变量与同连接结果事务
→ app persistence持久收尾与压缩source/CAS
→ coordinator自动运行/成功finalize
→ commands运行队列与UI错误出口。

整个store模块共享SessionStore和同一SQLite事务owner。既有inbox移动输入调用typed事实，新typed结果边界调用inbox私有SQL；这个模块环以唯一DB transaction收口，不新增状态副本/持久schema/通用框架。LiveRun只是运行轨迹/可选episode，conversation cache是耐久投影的副本，均不能证明提交成功。

## 写前caller清单

| API/状态 | 实际生产caller | 合同 |
| --- | --- | --- |
| TypedSessionWriter.finish | 原persistence两分支；CLI run/finalize；scheduler run_steps及Stopped守卫 | 原签名/行为保持，A5只使用旧finish/is_terminal |
| 新finish_with_input_outcome | app persistence::commit_outcome唯一生产caller | bool只确认本次完整结果事务 |
| SessionStore.finish_input | CLI run/finalize；commands/run assembly fallback；coordinator steering；fixture | 原API原语义，同连接私有原语供新事务 |
| SessionStore.set_status | CLI、app assembly、scheduler、原persistence、各store调用 | 原签名/不存在会话错误保持 |
| persist_round_outcome | coordinator::run_task唯一caller | Option改crate内部Result<PersistedRoundOutcome>；stopped只确认本次提交；持久拒绝覆盖原provider成功 |
| finalize_round | coordinator::run_task唯一caller | 只有已提交成功轮才能进入压缩/kz:done |
| run_task Err | commands/run唯一外层caller | 未提交结果不能额外单写input；ordinary assembly错误继续旧fallback |
| RoundCompaction.persist_and_publish | finalize_round唯一生产caller；真实DB测试 | source/CAS成功之后才发布完成 |
| auto_run错误分类 | coordinator；分类从cause chain downcast LlmError | 已提交Failed必须保留原provider Err类型 |

已全文rg搜索finish/input/status/结果/错误分类和直接caller，原清单在output/C5/callers-before.txt，未依赖编译器发现caller。

## 根因1：跨域收尾状态分叉（P1）

真实provider及assistant提交成功；SQLite仅拒turn_completed。旧persistence不检查typed terminal结果，仍写idle/run.completed/episode completed/input completed和成功通知，coordinator仍依据原Ok进入auto-success/push/finalize/压缩/kz:done。

原finish、finish_input、set_status、事件各为独立提交。直接让app Err不能修复input/status拒绝后的部分成功，因此增加唯一实际caller的原子边界：

Immediate获取writer → terminal/interruption事实 → 同session input验证/结果 → session status → status_changed/run结果 → commit → writer invariant/draft/open_calls/terminal更新。

Completed拒绝缺input/错session/cancelled/既有terminal；Stopped兼容D-342 finalize_interrupt先取消同会话input，保留cancelled。Completed拒绝后尝试Failed同边界；两者都拒绝为真实Err，不伪造持久成功/失败。旧finish、input/status公开API语义保持；只提私有同Connection SQL原语，防嵌套事务和重复写规则。连接open原有Immediate设置正确，新边界显式Immediate不计新问题。

最外层commands原fallback会独立finish_input(false)，又破坏双拒绝时的全回滚。因此具体未提交上下文保留Display/source，生产fallback helper跳过此写；真正assembly-before-finalizer错误仍failed。coordinator失败abort弱flush并回收halt，写租约沿既有Drop Release，C4 execution owner仍由writer/callback Arc覆盖真实终态和输入收尾，不新增join/租约。

provider Err且Failed成功时，持久函数返回durable Ok(store)，coordinator使用原run_result Err（类型链仍支持503/限流分类）；持久拒绝则使用真实持久Err。通知/episode/记忆只在commit后，optional后处理不强塞事务。

## 根因2：压缩候选提前发布成功（P1）

finalize_round在await LLM产生candidate后，真实pending.persist(source/CAS)之前就发完成stage/kz:compacted。独立手机typed事实使source已变，提交拒绝但UI仍提前收到成功；真实surface SQL拒绝同样可达。

候选阶段只显示候选，persist_and_publish真实提交成功后发既有payload/完成stage；L0机械清理同样只有提交后才能说已清理。A1/A2 seq-beforeprojection+source/CAS及artifact共享发布guard保持，stale source不覆盖cache，不发布成功。

## 验证设计与当前状态

- core3条真实SQLite回归：terminal/input/status/status-event/result-event五个拒绝点完整rollback、内存不推进、Failed retry；cancelled合法Stopped/Completed拒绝；错会话/缺input/已有terminal拒绝。
- persistence12条新回归：本地HTTP真实runner/typed assistant成功后实际调用持久函数；四种拒绝、双拒绝、成功提交后通知/记忆回调、合法Stopped、provider错误分类；真实stale source/surface拒绝和成功压缩发布。
- commands1条assembly控制；双拒绝夹具调用相同生产fallback helper证明不单写。
- 等价旧边界负对照脚本已准备，正确字节SHA备份与恢复，恢复后刷新mtime并最终重新验证正确源码。
- C5独占Cargo槽后完成：正确代码core416/app568全过、all-targets check/Clippy/fmt/diff通过；旧收尾等价边界5故障断言失败+2控制通过，旧压缩时序1断言失败。逐字恢复SHA与全Rust mtime刷新后执行最终正确代码。首次app夹具temp重名已以测试AtomicU64隔离，原日志保留。以上均来自C5当前树，不混用root/C4/A5结果。

## 包范围与下一包

最终8源码/依赖文件（含Cargo.lock仅1行既有rusqlite绑定）+map/report/json。源码根因2个P1，不按多个协作文件重复计数。

C6已登记owned assembly启动/失败的真实分叉：set_status(running)后running通知/status事件/observer/写租约失败，typed user已提交但assembly fallback只改input failed，DB session仍running/typed open。assembly仅C4切片复核过，不能标全文PASS；C5不扩此源码范围。候选map在output/C5/C6-candidate-map.md，后包复用新原子事务收口，不在commands拼SQL。

## 停止交错的最终caller合同

state.stop持runtime.lifecycle取消token并finalize_interrupt；coordinator持同runtime Arc与本轮token。persist scoped lifecycle内重核token→实际Completed/Stopped/Failed及Failed重试；随后drop lifecycle才report/mobile/harvest/memory，不覆盖同步kdeconnect output。

Ok/Err返回后Stop先赢均产生真实Stopped与cancelled输入；完成先赢后Stop不覆写Completed/input completed。Err的source/types保持，PersistedRoundOutcome.stopped收据仅来源于本次成功提交；coordinator添加具体停止上下文，actual retry gate禁已取消token，commands分类→idle stopped/无terminal kz:error/不单写input。Failed与双拒绝仍保原真实失败语义。

新增真实Stop3项（实际HTTP200/503、真实stop、实际persist与direct caller helper）通过；旧Stop边界2故障失败+1控制过，旧UI统一failed1故障失败。最终正确core416/app568、all-targets check/Clippy/fmt/diff全部通过，3文件逐字恢复SHA齐。此补核归根因1，不扩全文数/bug数。
