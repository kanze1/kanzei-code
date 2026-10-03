# B4 背景停止与最后守卫地图

基线 `29e2e3d4e9c9881e73d941d94b14c5fce5c13331`，复用A树分支 kanzei/audit-b4-background-stop。root已授独占Cargo。最终 scope 为五个必要生产 owner；lifecycle.rs 全文，其余四文件必要切片。已确认两条真实旧生产根因并修复，未编辑范围外源码。

## 依赖与生命周期

background.rs GuardCompletion已有Pending→共享Join→wait/completed；registration在registry发布前已设置guard/exit Pending，随后发布两个真实Join。exit worker回收child/输出reader并发mailbox；guard每300ms reconcile，观察到stopped后最后一次reconcile结束。observer只枚举is_running进程，合法tool_scope开闭按baseline锁吸收。

generic lifecycle stop只kill→mark_terminated→reconcile一次回bool；AlreadyStopped立刻false。stop_for_project验证project后调用stop，只核is_running。ProcessTool stop据bool/running输出stopped/finished；app tool_process_stop→stop_for_project；runtime_service关闭时generic stop。kill_process/project走同样一次reconcile，对应app run stop与team scoped cleanup：D2另显式wait_child_cleanup等待exit+guard后fresh restore，generic没有。

## 验证计划

使用实际注册进程、既有final_guard_hook屏障和managed tool_scope，直接证明旧stop已回执而最后guard仍能回滚之后的合法写。使用真实隔离项目/TEMP和自己PID，保留旧断言失败日志；不能只凭is_finished提前返回推断bug，不加sleep补丁。确认后只用existing completion，无新锁；检查AlreadyStopped、restore错误、guard不自调用stop、取消等待者、多waiters与所有公开consumer。source/API变更先发root协调，关联具体caller合同。

## 初始hash与caller

ignored output/parallel-B4/source-baseline.json保存三owner raw/LF；callers.txt保存全repo直接caller索引。root初次OS32清理日志保持，单独复跑PASS不代表根因证明。所有回归/后续checks待真实运行，不先记通过。

## 真实根因与协调后范围

已实证旧生产：stop true回执→held final guard→真实defect tool_scope写并关闭→旧guard恢复1文件，把LEGAL_AFTER_STOP_RECEIPT回滚为原始内容（old-reproduction.log，编译成功，断言失败101）。另quarantine路径被实际文件阻碍：PID停止/两completion已结束，stop_for_project仍Ok(true)，未恢复的字节仍存在（old-restore-error.log，断言失败101）。前者P0执行者收尾/合法基线遗漏；后者P1错误吞没/错误成功合同，auto remove/discover删除保留项为该错误恢复表现，不单计各caller根因。

root批准5生产owner：background.rs / background/lifecycle.rs / background/registration.rs / background/persistent.rs / process.rs。只lifecycle.rs本包确实完整读过；其余按必要数据/完成等待/stop consumer/注册删除切片记录，不为coverage补全文。A8的app/processes/lifecycle.rs只读，不编辑；其他app consumer只读。

## 全stop/registry writer caller清单

- stop_result(id)->Result<bool,String>：background.rs stop_for_project（project身份校验保留）、legacy stop(bool)包装；process.rs action=stop明确PROCESS_STOP_FAILED。app commands/run/tool_process.rs已Result返回原API，不改；runtime_service.rs关闭时legacy stop保签名但记录具体失败。
- stop_owned(&Arc<BackgroundProcess>)->Result<bool,String>私有：stop_result、kill_processes、persistent kill_registered_result。AlreadyStopped保false，但exit/guard共享Join均完成、清理成功收据确认后才返回；kill失败不等仍活worker，Err且磁盘项保留。Guard内部reconcile不调用stop/exit.wait，因此不self-await。所有wait均不持registry/baseline Mutex。
- kill_project/kill_process保usize→kill_processes；persistent仍跳过，只有实际kill且cleanup成功累计。app commands/run.rs异步回收与app processes/lifecycle.rs reset通过lib reexport调用；team/mod.rs按既有(owner,id,attempt)kill_child_processes后wait_child_cleanup，原子任务owner等待语义保留。
- kill_registered_result(root,id)->Result<bool,String>：process.rs action=kill，legacy kill_registered(bool)包装。找到已自然结束磁盘项仍found=true；有内存对象则共同stop_owned，无内存对象只kill实际pid，失败不删条目。legacy失败保日志，不冒充success。
- registration::register_with_mailbox在registry可见前设置exit/guard Pending，真实Join随后publish。exit先记录实际停止，persistent再wait guard+fresh restore成功才自动删磁盘；失败保原内存baseline/磁盘项，callback仍保实际exit结果及具体cleanup输出。reader原timeout-abort加实际Join，Join完成才代表owned stream/log句柄已结束。
- persistent::adopt_persistent：先构造process（exit/guard均Pending）→registry可见→启动并publish现有exit watcher Join→spawn/publishguard。watcher实际PID死后记录exit→等待guard/fresh restore，失败保持内存对象与磁盘项；成功才移除。guard不等待exit，exit标记先于wait，故无循环await。
- ProcessTool discover→mark_registry_failed同步入口：仅existing completed()确认两个task已结束、fresh restore成功才prune；Pending/error保具体输出；bool=false文案仅说本次未确认cleanup/未prune，不保证并发期间磁盘项一定存在。无内存ghost保既有pruned语义。load/save/remove_registry_entry的路径/格式/原有atomic写原语不扩。
- observer覆盖仍运行以及尚无恢复成功收据的旧process（含终结失败）；合法窗口开闭继续按同一baseline Mutex吸收。正常guard join Ok或失败补偿后的Recovered收据使该记录退出观察和重复恢复。

## 正负例与执行状态

background完整子集已实际36PASS/1既有ignored；包含stop回执后合法内容、restoreErr排障重试、取消第一等待者后第二AlreadyStopped仍join、停止中窗口基线、persistent stop/kill×registered/adopted四真实fixture的保留→discover→retry。registration真实Windows共享模式File在abort Drop屏障held时remove_file错误32，finish_reader直到实际句柄释放才完成的回归单测PASS。下一阶段等价旧production负例逐项断言失败+正常控制+finally五源原字节恢复，以及final全tools/checkClippyfmt尚在进行，未先记通过。

原挂起background/两单测日志保留；同步测试hook recv阻塞Tokio worker导致fixture唤醒不能推进，改为仅测试block_in_place后真实单测通过，不新增生产死锁结论。新增await时一次Mutex词法作用域Send编译失败保留并已用scope修复，不算产品bug。Root初次OS32日志仍由root保留，不能把单独旧测试PASS或本包新句柄控制推成对原初次原因的唯一解释。

## 完成收据与历史 prune 收口

当前补丁的二次恢复真实回滚也已实证：成功stop→合法tool_scope写→重复stop/reaper将内容回滚；pre-receipt-reproduction.log保存单测101真实文件断言失败。这是本次修复迭代，非新增历史根因。既有GuardCompletion.task新增显式Recovered；正常guard Join Ok直接成功。failed guard的fresh补偿先确认实际结束，在原baseline锁内再次复核→唯一reconcile_restore_locked→发布Recovered，避免两个等待者恢复之间夹入合法写后再回滚。stop/reaper、child wait/retry、mark_failed、两个自动删除共同消费finish_restore。

registration原keep127筛选仅is_running会丢失尚未清理的唯一baseline。提纯私有prune_finished，生产仍127历史目标；候选须exit已结束+guard成功/Recovered（无guard的NotRequired只表示原正常无需守卫）。回归使用真实已注册record的单项历史map调用同一helper，验证held/failed排除、成功后可回收，未制造128进程或扩registry格式。

最新定向40PASS/1既有ignored；其中8条新Rust回归（background父模块7条、registration 1条）及真实注册/接管四fixture。负例最终含原旧生产7条、补丁迭代3条、历史prune2条。中间对照pre_receipt_repaired只恢复helper而保留新failed observer实际PASS，完整日志保存negative-attempt-before-prune，不当作预期失败；最终对应旧observer+helper并使用独立真实失败补偿收据回归。最终tools全包798PASS/3既有ignored、0 doctests；tools/app all-target check、Clippy-D、fmt/diff均退出0。root后续统一workspace/native/UI，未用旧整仓结果认证。


## 最终证据与边界

12组完整结果的实际来源为 output/parallel-B4/checks-before-clippy-fix/negative-results.json 和 negative-runner-complete.log：12个真实assert失败101、12个合法单测PASS、每case五源exact原字节恢复。最终仅两源发生机械小diff（测试reconcile bridge cfg(test)、map_err→inspect_err、测试needless borrow以及恢复一行原fixture），不改变负例执行分支。final-small-delta.txt/negative-source-final-delta.json明确列前后SHA；旧SHA为事后逆向小diff重构，不能说成当时已预采。补跑收到root停止指令时已推进数case，当前case正常joined并finally恢复后通过阻断输出槽终止；PermissionError仅人为runner取消，非产品错误。完整12JSON提前另存，未用补跑prefix替代；被补跑覆写的top-level部分raw日志明确属于final机械小diff后的补充控制。

旧same_tool_windows_closed_out_of_order测试本来requirements.md路径在本基线合法（defect prefix整个project子树），原日志PASS。修新增夹具无效工具名requirement时文本替换连带误改旧fixture；已只恢复该旧行，原out-of-order定向实际1PASS。未删断言/未改窗口顺序/未算生产根因。新增fixture采用真实defect tool与合法archive路径。

完整tools首轮797PASS/1LaTeX初始化FAIL/3ignored，自己的B4 profile MiKTeX首次pdflatex失败日志保留；初始化同一真实单测1PASS后两次fulltools798PASS/3ignored。最终profile为自建B4，工具链D1隔离缓存、runtime AppData D2隔离缓存，TEMP/TMP为树外自建parallel-B4；实际MiKTeX日志落自建B4 Local缓存，不复制真实用户配置。全量验证与generated schemas原字节恢复均由 final-checks.ps1记录；check/Clippy/fmt/diff最终0。

下一包候选：next_id生成bg<ms>-<seq>而managed_process_id仅接受bg数字，expired-ID合同由root后续实际验证；本包只记录不修、不新增根因或覆盖。
