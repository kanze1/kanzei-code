# 文件页编辑:可编辑、可拖拽伸缩的文件浏览

- 身份: live_design
- 状态: 设计基线(2026-09-26 起草并随 ui2/files 分支实施,本文描述的是已落地的实现)
- 日期: 2026-09-26
- 上游文档: [ui_surface_stack.md](ui_surface_stack.md)(§4.6 分隔条与 `ui_layout` 持久化、00-surface 弹层唯一写法)、[ui_color_semantics.md](ui_color_semantics.md)(琥珀 = 需要注意 / 配置未保存)、[cc_codex_alignment_impl_maps.md](cc_codex_alignment_impl_maps.md)(§4 先读后写账本 R-367、回退检查点 R-366)
- 关联需求: 无(用户 2026-09-26 第二轮 UI 问题清单第 6 条;tracker 条目待登记)
- 关联缺陷: 无
- 关联决策: 无
- 一句话: 用户原话「文件浏览要带编辑功能，而且也是做成可拖拽伸缩的」。文件页从只读预览变成可编辑:按内容指纹比较并交换地保存,磁盘被别人(多半是代理)改过就出冲突横幅让人选,覆盖前留证;外部改动轮询、切项目暂存草稿、新建文件、从链接定位到行;托管文档等按真实路径只读并说明原因。文件树与编辑器之间的分隔条沿用 00-frame 的 installSplit,宽度经 `ui_layout` 跨重启保存。

## 1. 症状与根因

| # | 根因 | 位置(改前) |
|---|---|---|
| A | Monaco 写死 `readOnly: true`,前端没有保存/脏状态/快捷键;后端只有读命令 | `17-files.js` 编辑器创建处;`main.rs` 只注册 `files_snapshot/file_preview/files_annotate` |
| B | 切文件先改树高亮再加载,旧 model 直接 `dispose()`——一旦可编辑,未保存修改会被静默丢掉 | `17-files.js` openFilePreview |
| C | 读通道不保真:`from_utf8_lossy` 把 GBK 等非 UTF-8 替换成 U+FFFD,BOM 以 U+FEFF 塞进内容;Monaco `getValue()` 默认丢 BOM、混合换行按多数派统一(实测)。直接开放编辑再写回会损坏字节 | `files_view.rs` file_preview |
| D | 托管文档与内部文件没有只读策略:树里能看到 `.kanzei/project/*.md`、`.kanzei/memory/*`,手改会在任一 bash 窗口被托管围栏隔离并回滚 | `kanzei-tools/src/managed.rs` MANAGED_ROOTS |
| E | 用户在主树的手改会被 worktree 线的跨树围栏当成他线越界(没有写日志解释) | `kanzei-tools/src/cross_tree.rs` covered_by_log |
| F | 「打开文件并定位」只打开不定位:调用方传了 `line`,文件页不用;需求页锚点解析出行号却只传路径 | `19-research.js` openStructuredPath、`11-docs-list.js` 锚点 |
| G | 焦点在树/头部时 Ctrl+S 冒泡到 window 且没人 `preventDefault`(WebView2 默认行为可能弹「另存为」) | 全仓无 Ctrl+S 绑定 |
| H | 切项目是同步的(`activate_execution_root` → `reset_files_scope`),没法等确认框 | `09-sessions.js` |

「可拖拽伸缩」在 UI2-0926 第一波已经由 00-frame.js 的 `installSplit` 给文件树装上(`03-layout.js`,`--kz-split-files`),本次只补:上限按文件页自身宽度给编辑器留 360px(侧栏开合、后台任务侧栏停靠都会改变文件页宽度;原来是窗口宽度一半,侧栏开着时编辑器会被挤得很窄)、专属读屏名「调整文件树宽度」、分隔条 `aria-controls` 指向窗格。没有另写拖拽,也没有另开偏好字段。没有独立的标注面板(标注在树行内),所以只有这一条分隔条。

## 2. 后端(crates/kanzei-app/src/files_edit.rs)

- **唯一的路径规范化入口 `resolve_in_root(root, rel)`**,`file_preview`/`file_stat`/`file_write` 都走它:
  - 词法先拒(所有平台):空、NUL 与控制字符、`/` 或 `\` 开头(绝对路径、UNC、`\\?\`)、`..`。只在 Windows 上另拒(那里非法或有歧义,别的平台上是合法文件名,改造前 file_preview 能打开,不能一刀切):段内冒号(盘符与 ADS)、`<>"|?*`、保留设备名(`con`/`nul.txt`/`com1`…)、段尾点或空格。`\` 与 `/` 在所有平台都当分隔符(非 Windows 上文件名里含 `\` 的文件打不开,接受),`.` 段与重复分隔忽略。
  - **开头的斜杠是有意的行为变化**:旧 file_preview 把首尾的 `/`、`\` 剥掉再当相对路径,项目外的绝对路径(`/etc/x`、线路工作树里的完整路径)会被静默改读成项目里的同名文件(或报「不存在」);现在一律拒绝并说明「必须是相对项目根的路径」。项目根下的绝对路径由前端 `toProjectRel` 先转成相对路径(工具结果、路径 chip 都经它),04-structured 的路径 chip 本来就只对当前项目下的路径给相对路径、其余给完整路径,与此一致。
  - 再按真实路径判包含:存在的目标 `canonicalize` 后必须在项目根的 canonical 路径下;不存在的沿祖先找最近存在的目录判包含,再拼剩余段;悬空链接拒绝。返回的 `rel` 取自真实路径(真实大小写),目录链接指向根内受限目录时只读策略照样生效。
  - 纵深:真实路径包含判定同时靠 `starts_with` 与 `strip_prefix` 两层;Windows 上 `..` 还同时被「段尾点」规则挡——变异时要同时去掉才会越界(见 §7)。
- **写入策略 `write_policy(rel)`**(小写比较):任一段 `.git` → `git`;在**任意一个** `.kanzei` 段之后(不只项目根这一层——树里嵌着的另一个 kanzei 项目 `sub/.kanzei/project/*.md` 有它自己的托管围栏,改了同样被回滚):前缀在 `kanzei_tools::MANAGED_ROOTS`(改为 pub 并再导出,单源;测试钉住它们都以 `.kanzei/` 开头)→ `managed`;`state.db*`、`artifacts/`、`.write-log/`、`quarantine/`、`summaries/`、`worktrees/`、`file-annotations.json`、`*.lock`、`*.tmp` → `internal`。其余可写,含 `.kanzei/kanzei.toml` 与 `.kanzei/research/**`。
- **文本探测 `detect_text(bytes)`**:BOM(EF BB BF);换行按 Monaco 建模同一条多数派规则((CR + CRLF) × 2 > 总数 → CRLF);混合换行(两种以上或出现孤立 CR);UTF-8(截断预览切在多字节中间不算编码问题)。
- **`file_preview`**:原有 `content/binary/truncated/size` 语义不变(11-docs-list、19-research*、ui-workspace-smoke 依赖),只加字段:`hash`(整文件 FNV-1a 指纹,`kanzei_base::content_hash`,与标注 stamp、写日志、R-367 账本同族;截断时 null)、`bom`、`eol`、`mixedEol`、`encoding`、`mtimeMs`、`readonly`(二进制 > 截断 > 非 UTF-8 > 路径策略 > 只读属性,第一个命中的码或 null)。内容去掉 BOM。
- **`file_stat`**:不读内容,只给轮询粗筛用;不存在是 `exists:false` 不是错,越界是错。
- **`file_write(path, content, expectedHash, bom, evidence)`** 统一返回 `{status: "saved"|"conflict", hash, size, mtimeMs, exists, evidence}`;只读返回 `Err("READONLY:<code>")`,越界/超 4MB/IO 失败返回 Err:
  - 已存在:读字节,二进制/非 UTF-8/超限/只读属性拒绝;`expectedHash` 与磁盘指纹不等(或为 null 即「新建」)→ `conflict` 并回报磁盘指纹;相等才经 `kanzei_base::atomic_file::write_atomic_cas` 原子替换(rename 前再比一次)。
  - 不存在:`expectedHash` 非空 → `conflict {exists:false}`(打开后被删,不偷偷重建);为 null → 建父目录 + `create_new` 新建,撞上已存在也是冲突。
  - 写出 = (bom ? U+FEFF : "") + content;CRLF 由前端 model 的 EOL 保持。
  - `evidence = true`(只在「覆盖磁盘版本」时)先把被覆盖的磁盘版本写进 `.kanzei/quarantine/files-overwrite-<ms>/<rel>`,再替换;替换没发生就把证据撤掉。`files-overwrite` 已登记进 quarantine 清理内核的 KNOWN_KINDS(否则被当未知证据永久保留)。
  - 成功后记一条写日志(`process_id = "files-view"`、只留指纹、不带内容)。根下没有 `.kanzei` 目录(`resolve_root` 找不到项目、退回到打开的目录本身)就不记:那里没有围栏会读它,不凭空建出 `.kanzei/.write-log`——与代理 write 工具无 run 身份时不建 `.kanzei` 同一口径。覆盖留证照常建目录(安全比整洁重要)。
- 移动端桥是固定路由,不暴露写命令(保持)。

## 3. 与代理、回退、围栏的关系

| 机制 | 用户在文件页保存之后 |
|---|---|
| 代理的 edit / insert | 每次调用都现读磁盘再匹配锚点(`edit.rs` 读 `read_to_string`)。锚点没被用户改到 → 编辑叠在用户的修改之上;锚点被改掉 → 未命中并附上文件实际片段(等于重读)。Rust 测试 `用户保存后代理edit按磁盘现状匹配` 经真实开发档位 harness 取 edit 工具验证两种情况 |
| 代理的 write(整文件覆盖) | 唯一的盲写路径,**现在仍可能整文件盖掉用户刚保存的内容**(仓里还没有读账本,R-367 仍是 todo)。收口靠先读后写账本(R-367,见 impl_maps §4):那份地图已写明 write 在写盘前现读磁盘、按同一种内容指纹比对账本,Stale 即 FILE_CHANGED_SINCE_READ——比对的是**写入那一刻的磁盘指纹**,所以文件页保存(不经代理工具)也会被识别;只看写日志的实现识别不了。这里不另发事件、不改提示词(改 system prompt 会让整段对话的提示缓存失效)。**待登记的后续条目(本组按规则不能改 tracker,请集成方或用户挂在 R-367 下登记,登记后把条目号回填到这里)**:验收①代理 read 之后用户在文件页保存,代理下一次 write 返回 FILE_CHANGED_SINCE_READ、磁盘内容不变;②edit/insert 同一情形按锚点现读(已由本文件 Rust 测试证明,账本落地后仍须成立);③判定比对写入那一刻的磁盘指纹,不以写日志为准 |
| 跨树围栏(worktree 线跑 bash 时保护其它树含主根) | 写日志按「路径 + 指纹 + 窗口内」吸收有解释的变化,用户手改不被报成他线越界、不被隔离 |
| 托管围栏 | 托管文档在文件页只读(前端 readOnly + 后端拒写),不会出现「手改被回滚」 |
| 回退检查点(R-366) | 不进 file_checkpoints:检查点按用户消息记;kanzei 之外的改动按设计是外部改动,回退默认跳过、强制覆盖前留证,用户的手改默认保得住 |

## 4. 前端(ui/17-files-editor.js + 17-files.js)

17-files.js 只留树、度量与标注,继续导出 `openFilePreview`(11-docs-list / 19-research* 调用);编辑器状态在新模块,两边用 `initFilesEditor({ onActiveChange, onDirtyChange, onSaved, onCreated })` 回调解耦,不双向 import。测试接缝 `setMonacoLoader(fn)`(仿 04-markdown 的 setRenderMarkdown)。

**状态**:`filesDoc = { root, path, hash, bom, eol, mixedEol, encoding, readonly, blocked, size, mtimeMs, savedVersion, saving, conflict }`;脏 = `model.getAlternativeVersionId() !== savedVersion`(撤销回原点 = 干净)。`savedVersion` 取发起保存那一刻的版本,保存期间继续输入的部分仍算未保存。

**只读 ≠ 保存被拒**:`readonly` 只表示「打开时就只读」(后端 file_preview 给的码),这种文件永远不脏;`blocked` 是「保存被后端以 `READONLY:<code>` 拒绝」——打开之后磁盘上的文件变了性质(被转成 GBK、超过 4MB、变成二进制、加了只读属性)。两者必须分开:复核时实测过,把保存被拒记成 `readonly` 会让 `isFilesDirty()` 立刻为假,未保存标记、保存键、树上的琥珀点一起消失,编辑器变只读,之后点另一个文件不确认、切项目不存草稿——修改随 model 一起被释放。`blocked` 时修改仍算未保存、编辑器照样可编辑可复制,原因写在只读原因条(「保存被拒:<原因> · 你的修改还在编辑器里,可以复制出来…」);保存键照常可点(原因解除,例如去掉只读属性后,再保存就成功并清掉 `blocked`)。

| 事件 | 行为 |
|---|---|
| 打开(树、链接、新建) | 有未保存修改先 `confirmDialog`:保存 / 不保存 / 取消;取消留在原文件、树高亮不动;保存失败(冲突/出错)也不走。保存已被拒过(`blocked`)时不给「保存」,只给 不保存(危险色)/ 取消,并列出原因与「切换会丢掉这些修改;要留着就先取消,把内容复制出来」。然后 `file_preview` → 建或复用 model → `setEOL` 按后端探测 → 记干净点 → 按只读码设 `readOnly` 与 `readOnlyMessage`,`unusualLineTerminators: "off"`(原样保留 U+2028,不弹窗)→ 有 `line` 就滚到该行并选中 |
| 保存(Ctrl/Cmd+S、「保存」) | `file_write` 带打开时的指纹与 `bom`。`saved` → 换成写出内容的指纹、回到干净、清掉 `blocked`、关掉比较视图、静默重扫树;`conflict` → 记下磁盘指纹,出冲突横幅;`READONLY:<code>` → 记 `blocked`(见上),toast「保存被拒:<原因>」。冲突未决时普通保存不发请求,提示去横幅里选 |
| 冲突横幅(role=alert) | 比较:Monaco diff(原始 = 现读的磁盘版本,修改 = 当前 model,可继续编辑;窄时 Monaco 自动改上下内联;关掉中缝的还原按钮栏);用磁盘版本:重新读盘并替换(只改变化的区间,保留撤销,Ctrl+Z 能回到自己的修改);覆盖磁盘版本:以磁盘指纹交换 + 留证。文件被删时按钮变「放弃修改 / 重新创建」,重新创建不带指纹 |
| 外部改动轮询 | 文件页可见时每 2 秒 + 窗口回到前台:`file_stat` 大小/时间变了才 `file_preview` 比指纹;只是 touch → 记下新时间;有未保存修改 → 进冲突态,编辑器内容不动(在进重载之前判:重载的二进制分支会释放 model);干净 → 静默重载并在头部提示「已从磁盘更新」(role=status);被删且干净 → 关掉并说明。冲突未决、比较中、保存中都暂停。**静默重载只读一次盘**:轮询读到的新版本直接交给 `loadDoc({ preview, ifClean })`,不再读第二次(复核实测:第二次读盘的 IPC 期间打的字会被磁盘版本盖掉并标成干净,不出横幅);`ifClean` 在替换前复查脏状态,兜住余下的 await(等 Monaco),途中变脏就改进冲突态 |
| 重载的替换方式 | `replaceContent` 只替换变化的区间:先把磁盘文本的换行统一成 model 当前换行(Monaco 插入时也这么做),再取公共前后缀(边界不落在 CRLF 中间,退开 UTF-16 代理对),一次 `pushEditOperations`。代理改了别处时光标、选区随编辑平移(复核实测整篇替换会把 30:5 的光标挤到 29:19);换行本身的变化(磁盘从 LF 改成 CRLF)随后由 `setEOL` 跟上 |
| 重载读盘失败 | 编辑器里还有未保存修改就绝不释放 model,改成「已删除」冲突 |
| 切项目 | 未保存修改(含保存被拒的)暂存为草稿(内存,按 **文件所属项目根**(`doc.root`)+ 路径——`activate_execution_root` 先 `setCurrentProject` 再 `reset_files_scope`,用当前项目当键会把 A 的草稿注入 B 的同名文件);回到该文件时恢复为未保存(Ctrl+Z 回到磁盘版本);草稿之后磁盘又变了 → 直接进冲突态;回来时文件已不能写(只读属性/非 UTF-8/超限/二进制)→ 草稿照样恢复进编辑器(能看、能复制),按 `blocked` 处理而不是只读(否则草稿既恢复不出来、也永远清不掉) |
| 切语言 | 头部、横幅按钮、只读原因条的文字是渲染点 `t()` 写的(没有 `data-i18n-key`),监听 `kz:language` 立即重画,Monaco 的只读提示一并重设 |
| 读屏 | 头部随每次内容变化重画,但文字没变就不碰节点(`setText`):role=alert 的冲突横幅、role=status 的同步提示换一次文本节点读屏就重读一遍,不能每敲一个键念一次 |
| 新建文件(工具栏 ＋) | `inputDialog` 输入相对路径(默认当前文件所在目录)→ `file_write` 不带指纹写空文件(父目录自动建,已存在即冲突、不覆盖、直接打开)→ 展开祖先目录并打开 |
| 放弃修改 | 危险确认后重新读盘 |
| 焦点在树/头部时 Ctrl/Cmd+S | window 级兜底 `preventDefault` 并保存(只在文件页可见、无模态时);编辑器里的由 Monaco 命令处理,不冒泡 |

**头部**:[● 未保存] 路径(省略号) 大小 · UTF-8[ BOM] · CRLF/LF[ · 混合换行,保存时统一为 X] · 同步提示 …… [放弃修改][保存](保存键 `aria-keyshortcuts="Control+S"`,只读文件不显示)。只读原因条(role=note)说明为什么只读;托管文档给「打开需求页 / 打开记忆页」。树里未保存的文件名后一个琥珀点,读屏名带「未保存」,当前文件 `aria-selected="true"`。工具结果里的项目根下绝对路径与 `./` 前缀先转成相对路径再交后端判定。

**颜色**(ui_color_semantics §3):未保存 = 琥珀 `--warn`(与设置页 `.settings-dirty` 同色,「配置未保存」同一含义);冲突横幅 = `.settings-effective` 同款 `--alert-soft` 浅底 + 软边,不画彩色左竖条;只读原因条中性;「覆盖磁盘版本」是 `ghost danger`。没有新增颜色 token。弹层只走 00-surface(confirmDialog / inputDialog / toast)。

## 5. 只读原因

| 码 | 条件(后端判定) | 界面说法 |
|---|---|---|
| binary | 头 8KB 含 NUL | 二进制文件,占位显示大小 |
| truncated | 超过 4MB(只预览前 4MB,无整文件指纹) | 文件超过 4MB,只预览前 4MB |
| encoding | 去 BOM 后不是 UTF-8(如 GBK) | 保存会写坏原字节,只读 |
| managed | 真实路径在 MANAGED_ROOTS 下 | 只能经专用工具修改,直接改会被托管围栏隔离并回滚;给跳转 |
| git | 任一段 `.git` | Git 内部文件 |
| internal | kanzei 内部状态(见 §2) | kanzei 内部状态文件 |
| attr | 文件带只读属性 | 不能在这里保存 |
| unknown | 后端没返回指纹(旧 kzapp) | 无法安全保存,请更新 |

同一套码也用于「保存被拒」(`blocked`,见 §4):打开时可写、保存那一刻后端判出上表某一条,界面说「保存被拒:<原因>」,编辑器不变只读。

## 6. 分隔条

`03-layout.js` 的 `installSplit($("files-side"), { id: "files", side: "right", min: 200, max: () => 文件页宽 − 360(隐藏时退回窗口一半), title/ariaLabel: 「拖动调整文件树宽度 · 双击复位」/「调整文件树宽度」 })`。宽度写 `<html>` 上的 `--kz-split-files`,经 `ui_prefs.ui_layout.splits.files` 跨重启保存(D-404:本机 localStorage 重启即丢);方向键 ±8px、Home/双击复位。00-frame 的分隔条统一加 `aria-controls` 指向被调尺寸的窗格(侧栏、日志、文件树;记忆列表窗格没有 id 就不加)。无头 Edge 实测:从手柄中心拖 +100px,树宽 340 → 441,手柄跟到新边(差 2px 骑边),`ui_prefs_set` 收到 `{ui_layout:{splits:{files:441}}}`,← 两次 −16,Home 复位。

## 7. 门禁

- **Rust**(`cargo test -p kanzei-app files_edit`、`ipc_contract`;`-p kanzei-tools quarantine`):指纹一致才写入并返回写出字节指纹;打开后磁盘被改返回冲突且不写;被删返回 `exists:false`、不带指纹才重建;新建撞已存在不覆盖、自动建父目录;BOM + CRLF 原样往返字节不变;换行探测与 Monaco 同规则(8 例);路径词法拒绝(12 种所有平台都拒 + 12 种只在 Windows 上拒,四个入口都拒;非 Windows 上后 12 种必须放行);目录链接指出根外拒绝、指向根内托管目录按真实路径只读(`mklink /J` 实跑);受限路径只读且文件不变(含嵌套项目 `sub/.kanzei/project`、`sub/.kanzei/state.db`;`sub/.kanzei/kanzei.toml`、`sub/kanzei/project` 可写);MANAGED_ROOTS 全部以 `.kanzei/` 开头;非 UTF-8 / 超 4MB / 二进制只读;覆盖留证可取回且冲突时不留证;保存后写日志;根下没有 `.kanzei` 时保存不建 `.kanzei`、覆盖留证照常;file_stat 存在/删除/目录;用户保存后代理 edit 按磁盘现状匹配;quarantine 认识 `files-overwrite`。手工变异(改源码 → 跑对应测试 → 还原)全部变红:去掉包含判定两层、去掉写入策略、不补 BOM、不比指纹、不留证、写日志写到别处、去掉 `..` 与段尾点两条词法规则。
- **IPC 契约**:`scripts/ipc-contract.json` 新增 `file_preview`/`file_stat`/`file_write`(`KZ_UPDATE_IPC_CONTRACT=1 cargo test -p kanzei-app <命令>_形状`,三条要串行跑,并行会互相覆盖写回);ui-runtime-smoke 的文件编辑分区拿夹具与之逐键比对。
- **ui-runtime-smoke「分区:文件编辑」**:Monaco 桩(alternativeVersionId、EOL、撤销栈、命令表、按范围编辑与 `getPositionAt`)+ 内存磁盘(可模拟二进制、可强制只读码)。S0 夹具与契约、只读原因英文词条、绝对路径转相对;S1 可编辑、脏标记(头部 + 树)、Ctrl+S 带指纹与 BOM、CRLF 保持、撤销回原点干净、保存期间继续输入仍脏;S2 冲突横幅、冲突时不写盘/不改编辑器、轮询暂停、冲突未决 Ctrl+S 不发请求、敲键不重写 role=alert/status 文字、切语言横幅按钮立即重写、比较开合与 Esc、在比较视图里覆盖带冲突指纹与留证且保存后关掉比较、用磁盘版本可撤销、读盘失败保住未保存修改并「重新创建」;S3 切文件三选一;S4 托管只读;S5 touch 不重载、干净静默重载、脏进冲突、被删关闭;S5b 静默重载只读一次盘、只替换变化的那一行、等 Monaco 期间打字进冲突不盖掉、磁盘 LF→CRLF 后 model 跟上、脏文件被换成二进制进冲突保住 model(覆盖被拒记 blocked);S6 按真实顺序(先 setCurrentProject 再 reset_files_scope)切项目:另一个项目的同名文件不注入草稿、回原项目恢复、之后磁盘变了进冲突;S7 定位到行;S8 分隔条 aria、上限随文件页宽、进 `ui_layout`、Home 复位;S9 Ctrl+S 兜底、模态打开时让路;S11 保存被拒 ≠ 只读(仍脏、树点在、编辑器可编辑、切文件只给 不保存/取消、切项目存草稿、草稿恢复进已不能写的文件按 blocked、原因解除后能保存);S10 新建。变异守卫 25 条:`filesSaveCas / filesBomRoundtrip / filesTypingDuringSave / filesConflictBanner / filesOverwriteEvidence / filesDirtyGuard / filesReadonlyApply / filesExternalReloadDirty / filesDraftStash / filesLineReveal / filesCtrlSFallback / filesSplitMax / filesSplitControls / filesReloadKeepsDirty`,复核修复加 `filesSaveRejectBlocked / filesDraftIntoBlocked / filesWatchReuseRead / filesReloadRecheckDirty / filesMinimalReload / filesEolReload / filesDraftKeyRoot / filesCtrlSModal / filesSaveClosesCompare / filesHeadSetText / filesLanguageRerender`,逐条实跑变红。`filesExternalReloadDirty` 在加了重载复查之后一度变绿(两层防线互相兜底),靠 S5b「脏文件被换成二进制」把两层区分开:轮询里的判脏必须在进重载之前。
- **有意没加守卫**:`loadDoc` 里 `root === currentProject` 的过期判断。唯一改 `currentProject` 的入口 `activate_execution_root` 同步调 `reset_files_scope` → `resetFilesDoc` 递增 `openGeneration`,generation 判断已经挡住在途的旧项目读盘;这一条是纵深,要让它单独变红只能伪造一条「改了当前项目却不重置文件页」的调用序列,真实路径上不存在。
- **ui-a11y-smoke「分区:文件编辑」**:冲突横幅 role=alert、未保存/同步提示 role=status、只读原因 role=note、保存键 aria-keyshortcuts、比较 aria-pressed、新建键读屏名、分隔条经 installSplit 且 aria-controls、树行 aria-selected、未保存琥珀、冲突横幅浅底且无左竖条;自带 6 个反例,另实跑删掉 `role="alert"` 变红。
- **ui-preview**:`node scripts/ui-preview/shoot.mjs --scenes files --query state=open|dirty|conflict|compare|readonly|blocked|new[&tree=520]`,已在 1333×695@1.5 与 1600×900@1.25、暗/亮、中/英下看过截图。`blocked` = 有未保存修改时保存被拒(夹具把 file_write 换成抛 `READONLY:encoding`)。

## 8. 不做与风险

- 不做:重命名、删除、多标签、关窗拦截(界面卡死时拦截会让窗口关不掉;未保存修改关窗即丢,草稿只在内存里)。
- 原子替换(tmp + rename)会让硬链接、ADS、自定义 ACL 丢失;rename 失败时同目录留一个 `.tmp` 现场(错误信息里点名)。
- 混合换行的文件保存时统一成多数派(Monaco 模型限制),头部如实提示。
- 非 UTF-8 文件只读;需要编辑请先转码。
- R-367 账本落地前,代理的 write 仍可能整文件盖掉用户刚保存的内容(edit/insert 不受影响);冲突横幅只保护「用户这边」的未保存修改。
- 覆盖留证在 `.kanzei/quarantine/files-overwrite-*`,toast 给出路径;文件页不列出 quarantine(gitignore)。
- 轮询只在文件页可见且打开了文件时进行,`file_stat` 不读内容;变了才读一次整文件(≤ 4MB)。
- 焦点不在文件页时 Ctrl+S 仍是 WebView2 默认行为(本次只管文件页)。

## 9. 接缝(给集成)

- 新模块 `ui/17-files-editor.js` 已写进 index.html 脚本清单;`scripts/ui-esm-graph.json` 未重生成(按约定由集成方跑 `node scripts/gen-esm-graph.mjs`)。
- `00-frame.js` 的 `installSplit` 多了一行 `aria-controls`(对所有带 id 的窗格生效);`03-layout.js` 的文件树 installSplit 参数改了上限与文案。其它组若也改这两个文件,合并时保留这两处。
- `scripts/ipc-contract.json` 新增三条;若别组也跑了 `KZ_UPDATE_IPC_CONTRACT`,合并按键取并集。
- `kanzei_tools::MANAGED_ROOTS` 改为 pub 再导出;quarantine KNOWN_KINDS 多了 `files-overwrite`。
- `ui-runtime-smoke` 里 R-189 的 Monaco 主题断言改读 `17-files-editor.js`(编辑器创建搬了家)。
- **与兄弟分支的合并冲突(复核用 `git merge-tree` 试合)**:与 ui2/workdir 冲突在 `scripts/ui-preview/scenes.mjs`、`scripts/ui-runtime-smoke.mjs`;与 ui2/arch 冲突在 `scripts/ui-preview/shoot.mjs`(默认场景串)与 `scripts/ui-runtime-smoke.mjs`。都出在文件末尾的追加区,一律取并集:shoot.mjs 默认场景串同时保留 `files` 与 arch 组的新场景;runtime-smoke 的变异表条目与各「分区」块全部保留。合并后重跑本组 25 条 `KZ_SMOKE_MUTATE`(表的「恰好命中一处」自检能发现重复插入),再跑 `node scripts/gen-esm-graph.mjs` 收录 `17-files-editor.js`。
- **tracker 待登记**:§3 的 R-367 后续条目(文件页保存后代理 write 必须得到 FILE_CHANGED_SINCE_READ),本组不能改 tracker。

## 变更记录

- 2026-09-26:起草并实施(ui2/files 分支,UI2-0926 #6)。相对勘察计划(scratchpad result-files.json)的调整:不新建 03-splitter.js、不在 prefs 另开 layout 字段——分隔条复用第一波的 installSplit 与 `ui_layout`;未保存标记用专用 `.files-dirty-dot` 而不是给 `.kz-dot` 加新状态(kz-dot 是运行状态原语,带动效与门禁清单);比较视图关掉 Monaco diff 中缝的还原按钮栏(Monaco 未分层 CSS 给它画 1px 焦点蓝框,且与横幅重复);新增「重载读盘失败保住未保存修改」一支(复核时发现:用磁盘版本撞上文件刚被删会连 model 一起释放)。

- 2026-09-26 复核修复(UI2-0926 #6 文件编辑(复核修复)):保存被拒改记 `blocked` 不再冒充只读(数据丢失路径,major);静默重载只读一次盘 + 替换前复查脏状态(竞态);重载只替换变化的区间(光标不跳);头部文字没变不重写、切语言立即重画(读屏);草稿按 `doc.root` 记的真实顺序、LF→CRLF、模态时 Ctrl+S、保存后关比较补了守卫;后端 Windows 专属词法规则按平台生效、开头斜杠拒绝写明是有意变化、`.kanzei` 规则对嵌套项目生效、无 `.kanzei` 时不建写日志;R-367 后续条目的验收写进 §3 待登记。

## 验证证据

- `cargo fmt --all -- --check`、`cargo clippy -p kanzei-app -p kanzei-tools --all-targets -- -D warnings`、`cargo test -p kanzei-app -p kanzei-tools`(325 + 599 通过)。
- 复核修复后:`cargo fmt --all -- --check`、`cargo clippy -p kanzei-app --all-targets -- -D warnings`、`cargo test -p kanzei-app`(327 通过);前端门禁全部退出码 0;文件编辑分区 25 条 `KZ_SMOKE_MUTATE` 逐条实跑全部 exit 1;复核的无头 Edge 探针重跑:外部改动重载后光标仍在 30:5,保存被拒后「未保存」与树上的点都在、点别的文件弹确认、修改仍在 model 里,读盘延迟 600ms 时打的字不丢。
- 前端门禁全部退出码 0:ui-a11y-smoke、ui-i18n-smoke、ui-markdown-smoke、ui-lint-smoke(含浏览器冒烟)、ui-connectivity、parallel-lines-regression、ipc-event-smoke、check-design-freshness、ui-narrow-layout-smoke、ui-workspace-smoke、ui-runtime-smoke。
