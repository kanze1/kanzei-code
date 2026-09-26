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
  - 词法先拒:空、NUL、`/` 或 `\` 开头(绝对路径、UNC、`\\?\`)、`..`、段内冒号(盘符与 ADS)、Windows 禁用字符、保留设备名(`con`/`nul.txt`/`com1`…)、段尾点或空格;`\` 与 `/` 都当分隔符,`.` 段与重复分隔忽略。
  - 再按真实路径判包含:存在的目标 `canonicalize` 后必须在项目根的 canonical 路径下;不存在的沿祖先找最近存在的目录判包含,再拼剩余段;悬空链接拒绝。返回的 `rel` 取自真实路径(真实大小写),目录链接指向根内受限目录时只读策略照样生效。
  - 纵深:`..` 同时被「段尾点」规则挡,真实路径包含判定同时靠 `starts_with` 与 `strip_prefix` 两层——变异时要同时去掉才会越界(见 §7)。
- **写入策略 `write_policy(rel)`**(小写比较):任一段 `.git` → `git`;前缀在 `kanzei_tools::MANAGED_ROOTS`(改为 pub 并再导出,单源)→ `managed`;`.kanzei/` 下的 `state.db*`、`artifacts/`、`.write-log/`、`quarantine/`、`summaries/`、`worktrees/`、`file-annotations.json`、`*.lock`、`*.tmp` → `internal`。其余可写,含 `.kanzei/kanzei.toml` 与 `.kanzei/research/**`。
- **文本探测 `detect_text(bytes)`**:BOM(EF BB BF);换行按 Monaco 建模同一条多数派规则((CR + CRLF) × 2 > 总数 → CRLF);混合换行(两种以上或出现孤立 CR);UTF-8(截断预览切在多字节中间不算编码问题)。
- **`file_preview`**:原有 `content/binary/truncated/size` 语义不变(11-docs-list、19-research*、ui-workspace-smoke 依赖),只加字段:`hash`(整文件 FNV-1a 指纹,`kanzei_base::content_hash`,与标注 stamp、写日志、R-367 账本同族;截断时 null)、`bom`、`eol`、`mixedEol`、`encoding`、`mtimeMs`、`readonly`(二进制 > 截断 > 非 UTF-8 > 路径策略 > 只读属性,第一个命中的码或 null)。内容去掉 BOM。
- **`file_stat`**:不读内容,只给轮询粗筛用;不存在是 `exists:false` 不是错,越界是错。
- **`file_write(path, content, expectedHash, bom, evidence)`** 统一返回 `{status: "saved"|"conflict", hash, size, mtimeMs, exists, evidence}`;只读返回 `Err("READONLY:<code>")`,越界/超 4MB/IO 失败返回 Err:
  - 已存在:读字节,二进制/非 UTF-8/超限/只读属性拒绝;`expectedHash` 与磁盘指纹不等(或为 null 即「新建」)→ `conflict` 并回报磁盘指纹;相等才经 `kanzei_base::atomic_file::write_atomic_cas` 原子替换(rename 前再比一次)。
  - 不存在:`expectedHash` 非空 → `conflict {exists:false}`(打开后被删,不偷偷重建);为 null → 建父目录 + `create_new` 新建,撞上已存在也是冲突。
  - 写出 = (bom ? U+FEFF : "") + content;CRLF 由前端 model 的 EOL 保持。
  - `evidence = true`(只在「覆盖磁盘版本」时)先把被覆盖的磁盘版本写进 `.kanzei/quarantine/files-overwrite-<ms>/<rel>`,再替换;替换没发生就把证据撤掉。`files-overwrite` 已登记进 quarantine 清理内核的 KNOWN_KINDS(否则被当未知证据永久保留)。
  - 成功后记一条写日志(`process_id = "files-view"`、只留指纹、不带内容)。
- 移动端桥是固定路由,不暴露写命令(保持)。

## 3. 与代理、回退、围栏的关系

| 机制 | 用户在文件页保存之后 |
|---|---|
| 代理的 edit / insert | 每次调用都现读磁盘再匹配锚点(`edit.rs` 读 `read_to_string`)。锚点没被用户改到 → 编辑叠在用户的修改之上;锚点被改掉 → 未命中并附上文件实际片段(等于重读)。Rust 测试 `用户保存后代理edit按磁盘现状匹配` 经真实开发档位 harness 取 edit 工具验证两种情况 |
| 代理的 write(整文件覆盖) | 唯一的盲写路径。先读后写账本(R-367,见 impl_maps §4)按同一种内容指纹判「读后被改」:用户保存改变了指纹,账本落地后代理下一次 write/edit 自动得到 FILE_CHANGED_SINCE_READ。这里不另发事件、不改提示词(改 system prompt 会让整段对话的提示缓存失效) |
| 跨树围栏(worktree 线跑 bash 时保护其它树含主根) | 写日志按「路径 + 指纹 + 窗口内」吸收有解释的变化,用户手改不被报成他线越界、不被隔离 |
| 托管围栏 | 托管文档在文件页只读(前端 readOnly + 后端拒写),不会出现「手改被回滚」 |
| 回退检查点(R-366) | 不进 file_checkpoints:检查点按用户消息记;kanzei 之外的改动按设计是外部改动,回退默认跳过、强制覆盖前留证,用户的手改默认保得住 |

## 4. 前端(ui/17-files-editor.js + 17-files.js)

17-files.js 只留树、度量与标注,继续导出 `openFilePreview`(11-docs-list / 19-research* 调用);编辑器状态在新模块,两边用 `initFilesEditor({ onActiveChange, onDirtyChange, onSaved, onCreated })` 回调解耦,不双向 import。测试接缝 `setMonacoLoader(fn)`(仿 04-markdown 的 setRenderMarkdown)。

**状态**:`filesDoc = { root, path, hash, bom, eol, mixedEol, encoding, readonly, size, mtimeMs, savedVersion, saving, conflict }`;脏 = `model.getAlternativeVersionId() !== savedVersion`(撤销回原点 = 干净)。`savedVersion` 取发起保存那一刻的版本,保存期间继续输入的部分仍算未保存。

| 事件 | 行为 |
|---|---|
| 打开(树、链接、新建) | 有未保存修改先 `confirmDialog`:保存 / 不保存 / 取消;取消留在原文件、树高亮不动;保存失败(冲突/出错)也不走。然后 `file_preview` → 建或复用 model → `setEOL` 按后端探测 → 记干净点 → 按只读码设 `readOnly` 与 `readOnlyMessage`,`unusualLineTerminators: "off"`(原样保留 U+2028,不弹窗)→ 有 `line` 就滚到该行并选中 |
| 保存(Ctrl/Cmd+S、「保存」) | `file_write` 带打开时的指纹与 `bom`。`saved` → 换成写出内容的指纹、回到干净、静默重扫树;`conflict` → 记下磁盘指纹,出冲突横幅。冲突未决时普通保存不发请求,提示去横幅里选 |
| 冲突横幅(role=alert) | 比较:Monaco diff(原始 = 现读的磁盘版本,修改 = 当前 model,可继续编辑;窄时 Monaco 自动改上下内联;关掉中缝的还原按钮栏);用磁盘版本:重新读盘并整体替换(保留撤销,Ctrl+Z 能回到自己的修改);覆盖磁盘版本:以磁盘指纹交换 + 留证。文件被删时按钮变「放弃修改 / 重新创建」,重新创建不带指纹 |
| 外部改动轮询 | 文件页可见时每 2 秒 + 窗口回到前台:`file_stat` 大小/时间变了才 `file_preview` 比指纹;只是 touch → 记下新时间;干净 → 静默重载并在头部提示「已从磁盘更新」(role=status);有未保存修改 → 进冲突态,编辑器内容不动;被删且干净 → 关掉并说明。冲突未决、比较中、保存中都暂停 |
| 重载读盘失败 | 编辑器里还有未保存修改就绝不释放 model,改成「已删除」冲突 |
| 切项目 | 未保存修改暂存为草稿(内存,按 项目根 + 路径);回到该文件时恢复为未保存(Ctrl+Z 回到磁盘版本);草稿之后磁盘又变了 → 直接进冲突态 |
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

## 6. 分隔条

`03-layout.js` 的 `installSplit($("files-side"), { id: "files", side: "right", min: 200, max: () => 文件页宽 − 360(隐藏时退回窗口一半), title/ariaLabel: 「拖动调整文件树宽度 · 双击复位」/「调整文件树宽度」 })`。宽度写 `<html>` 上的 `--kz-split-files`,经 `ui_prefs.ui_layout.splits.files` 跨重启保存(D-404:本机 localStorage 重启即丢);方向键 ±8px、Home/双击复位。00-frame 的分隔条统一加 `aria-controls` 指向被调尺寸的窗格(侧栏、日志、文件树;记忆列表窗格没有 id 就不加)。无头 Edge 实测:从手柄中心拖 +100px,树宽 340 → 441,手柄跟到新边(差 2px 骑边),`ui_prefs_set` 收到 `{ui_layout:{splits:{files:441}}}`,← 两次 −16,Home 复位。

## 7. 门禁

- **Rust**(`cargo test -p kanzei-app files_edit`、`ipc_contract`;`-p kanzei-tools quarantine`):指纹一致才写入并返回写出字节指纹;打开后磁盘被改返回冲突且不写;被删返回 `exists:false`、不带指纹才重建;新建撞已存在不覆盖、自动建父目录;BOM + CRLF 原样往返字节不变;换行探测与 Monaco 同规则(8 例);22 种路径词法拒绝(四个入口都拒);目录链接指出根外拒绝、指向根内托管目录按真实路径只读(`mklink /J` 实跑);受限路径只读且文件不变;非 UTF-8 / 超 4MB / 二进制只读;覆盖留证可取回且冲突时不留证;保存后写日志;file_stat 存在/删除/目录;用户保存后代理 edit 按磁盘现状匹配;quarantine 认识 `files-overwrite`。手工变异(改源码 → 跑对应测试 → 还原)全部变红:去掉包含判定两层、去掉写入策略、不补 BOM、不比指纹、不留证、写日志写到别处、去掉 `..` 与段尾点两条词法规则。
- **IPC 契约**:`scripts/ipc-contract.json` 新增 `file_preview`/`file_stat`/`file_write`(`KZ_UPDATE_IPC_CONTRACT=1 cargo test -p kanzei-app <命令>_形状`,三条要串行跑,并行会互相覆盖写回);ui-runtime-smoke 的文件编辑分区拿夹具与之逐键比对。
- **ui-runtime-smoke「分区:文件编辑」**:Monaco 桩(alternativeVersionId、EOL、撤销栈、命令表)+ 内存磁盘。S0 夹具与契约、只读原因英文词条、绝对路径转相对;S1 可编辑、脏标记(头部 + 树)、Ctrl+S 带指纹与 BOM、CRLF 保持、撤销回原点干净、保存期间继续输入仍脏;S2 冲突横幅、冲突时不写盘/不改编辑器、轮询暂停、冲突未决 Ctrl+S 不发请求、比较开合与 Esc、覆盖带冲突指纹与留证、用磁盘版本可撤销、读盘失败保住未保存修改并「重新创建」;S3 切文件三选一;S4 托管只读;S5 touch 不重载、干净静默重载、脏进冲突、被删关闭;S6 草稿暂存/恢复/之后磁盘变了进冲突;S7 定位到行;S8 分隔条 aria、上限随文件页宽、进 `ui_layout`、Home 复位;S9 Ctrl+S 兜底;S10 新建。变异守卫 14 条:`filesSaveCas / filesBomRoundtrip / filesTypingDuringSave / filesConflictBanner / filesOverwriteEvidence / filesDirtyGuard / filesReadonlyApply / filesExternalReloadDirty / filesDraftStash / filesLineReveal / filesCtrlSFallback / filesSplitMax / filesSplitControls / filesReloadKeepsDirty`,逐条实跑变红。
- **ui-a11y-smoke「分区:文件编辑」**:冲突横幅 role=alert、未保存/同步提示 role=status、只读原因 role=note、保存键 aria-keyshortcuts、比较 aria-pressed、新建键读屏名、分隔条经 installSplit 且 aria-controls、树行 aria-selected、未保存琥珀、冲突横幅浅底且无左竖条;自带 6 个反例,另实跑删掉 `role="alert"` 变红。
- **ui-preview**:`node scripts/ui-preview/shoot.mjs --scenes files --query state=open|dirty|conflict|compare|readonly|new[&tree=520]`,已在 1333×695@1.5 与 1600×900@1.25、暗/亮、中/英下看过截图。

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

## 变更记录

- 2026-09-26:起草并实施(ui2/files 分支,UI2-0926 #6)。相对勘察计划(scratchpad result-files.json)的调整:不新建 03-splitter.js、不在 prefs 另开 layout 字段——分隔条复用第一波的 installSplit 与 `ui_layout`;未保存标记用专用 `.files-dirty-dot` 而不是给 `.kz-dot` 加新状态(kz-dot 是运行状态原语,带动效与门禁清单);比较视图关掉 Monaco diff 中缝的还原按钮栏(Monaco 未分层 CSS 给它画 1px 焦点蓝框,且与横幅重复);新增「重载读盘失败保住未保存修改」一支(复核时发现:用磁盘版本撞上文件刚被删会连 model 一起释放)。

## 验证证据

- `cargo fmt --all -- --check`、`cargo clippy -p kanzei-app -p kanzei-tools --all-targets -- -D warnings`、`cargo test -p kanzei-app -p kanzei-tools`(325 + 599 通过)。
- 前端门禁全部退出码 0:ui-a11y-smoke、ui-i18n-smoke、ui-markdown-smoke、ui-lint-smoke(含浏览器冒烟)、ui-connectivity、parallel-lines-regression、ipc-event-smoke、check-design-freshness、ui-narrow-layout-smoke、ui-workspace-smoke、ui-runtime-smoke。
