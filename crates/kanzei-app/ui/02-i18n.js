const I18N_EN = {
  "agent 正在做这一条": "The agent is working on this item",
  "同轮最多并行执行的子代理数。默认 16(留空即默认);超出部分不排队,直接以「too many parallel subagent tasks」错误返回给模型。桌面端主对话已生效(R-173 起 task 工具注册不再受执行策略门控)。": "Max subagents that can run in parallel in one turn. Default 16 (leave empty for the default); extras are not queued — they get a \"too many parallel subagent tasks\" error back to the model. Effective in the desktop main conversation (task tool registration is no longer gated by the execution policy since R-173).",
  "架构浏览": "Architecture browser",
  "切换到架构浏览": "Switch to architecture browser",
  "设计文档与架构索引的可视化浏览:左侧为 docs/design 文档树(按索引状态分层),右侧为架构索引原文;点击文档在应用内 Markdown 查看器打开。": "Visual browse of design docs and the architecture index: left shows the docs/design tree (grouped by index status), right shows the index text; clicking a doc opens it in the in-app Markdown viewer.",
  "篇设计文档": " design docs",
  "行索引": " index lines",
  "未入册": "not indexed",
  "暂无设计文档": "No design docs",
  "架构索引": "Architecture index",
  "在查看器中打开架构索引": "Open the architecture index in the viewer",
  "设计文档树": "Design doc tree",
  "打开": "Open",
  "重新扫描架构索引": "Rescan architecture index",
  "记忆管理": "Memory management",
  "跳转到记忆页维护记忆条目(编辑/整理/重心设置走既有 memory 命令)": "Jump to the memory page to maintain entries (edit/consolidate/focus via existing memory commands)",
  "打开失败": "Failed to open",
  "架构索引读取失败": "Failed to load architecture index",
  "先选择一个项目": "Select a project first",
  "agent 下一个会拿这一条(按取活顺序)": "The agent will pick this item next (by work order)",
  // 侧栏「当前在做」焦点卡片:完整列表搬进单页视图后,侧栏只保留取活焦点这一条。
  "当前在做": "In progress now",
  "记需求": "Log item",
  "记缺陷": "Log defect",
  "打开完整需求与缺陷列表": "Open the full work item and defect lists",
  "查看完整列表": "View the full list",
  "在完整列表中查看": "Show in the full list",
  "当前没有在做的条目": "Nothing is in progress",
  "队列已清空或全部被阻塞": "The queue is empty, or everything is blocked",
  "下一个": "Next",
  "待办": "Backlog",
  "依据": "Basis",
  "本轮运行证据": "run evidence from this round",
  "取活顺序推断": "inferred from the work order",
  "优先级仅参考,不影响取活顺序": "Priority is reference only; it does not affect the work order",
  "需求与工作 / 缺陷 / 测试": "Work items / Defects / Tests",
  "完整列表与深度管理都在这里：筛选、排序、拖拽定开发顺序、字段编辑、批量操作、依赖视图与测试记录；侧栏只留当前在做。":
    "Full lists and deep management live here: filtering, sorting, drag to set the development order, field editing, bulk actions, the dependency view, and test runs. The sidebar keeps only what is in progress now.",
  "查看测试记录": "View test runs",
  "测试记录由 agent 跑测时写入,刷新会自动归档已完成项。":
    "Test runs are written by the agent while it tests; refreshing archives completed entries automatically.",
  "排序只改显示;拖拽(手动排序)才写回文件、改变取活顺序":
    "Sorting only changes the display; dragging under manual sort writes back to the file and changes the agent's work order",
  "文件树加载失败": "Failed to load file tree",
  "已标注": "annotated",
  "文件导览": "File explorer",
  "切换到文件导览": "Switch to file explorer",
  "切换排序:名称 / 行数": "Toggle sort: name / lines",
  "用 fast 模型为文件生成一句话用途标注(增量,只标新增或已变化的文件)": "Generate one-line purpose annotations with the fast model (incremental; only new or changed files)",
  "重新扫描": "Rescan",
  "重新扫描文件树": "Rescan file tree",
  "项目文件树": "Project file tree",
  "选择左侧文件查看内容 · 目录行显示聚合度量 · 「标注」用 fast 模型生成用途说明": "Select a file on the left to view it · directory rows show aggregates · \"Annotate\" generates purpose notes with the fast model",
  "个文件": "files",
  "行": "lines",
  "字": "chars",
  "标注": "Annotate",
  "标注中": "Annotating",
  "标注完成": "Annotation complete",
  "标注失败": "Annotation failed",
  "过大未计": "too large, not measured",
  "二进制文件": "Binary file",
  "已截断预览前 4MB": "preview truncated to first 4MB",
  "预览失败": "Preview failed",
  "按名称": "By name",
  "按行数": "By lines",
  "拖拽调序已锁": "Reordering locked",
  "分组视图": "grouped view",
  "排序": "sort",
  "标签": "tag",
  "解锁": "Unlock",
  "关闭分组、切回手动排序并清除全部筛选,恢复拖拽调序": "Turn off grouping, switch to manual sort and clear all filters to re-enable drag reordering",
  "关于 kanzei": "About kanzei",
  "kanzei 是文件优先的日常开发工具：让上下文、权限、记忆和工作轨迹可见、可回放、可验证。": "kanzei is a file-first daily development tool that makes context, permissions, memory, and work traces visible, replayable, and verifiable.",
  "从左侧选择项目，在对话框输入任务；遇到权限请求时选择允许、拒绝或总是允许。运行结果、错误和工具详情会留在当前会话中。": "Select a project on the left and enter a task in the conversation. For permission requests, choose allow, deny, or always allow. Results, errors, and tool details stay in the current session.",
  "项目": "Projects", "当前状态": "Current status", "空闲": "Idle", "排队输入": "Queued input",
  "测试记录": "Test runs", "目标": "Goals", "历史对话": "Chat history", "需求与工作": "Work items",
  "缺陷": "Defects", "研究": "Research", "来源": "Sources", "发现": "Findings", "开发规范": "Conventions",
  "自动审查缺陷": "Review defects", "使用只读子代理审查活动缺陷，不修改项目文件": "Review active defects with a read-only subagent without modifying project files",
  "正在审查缺陷…": "Reviewing defects…", "当前没有活动缺陷": "There are no active defects", "审查完成": "Review complete", "审查失败": "Review failed", "缺陷自动审查报告": "Automated defect review report",
  "对话": "Chat", "工作区": "Workspace", "设置": "Settings", "活动": "Activity", "继续": "Continue",
  "鞭挞": "Auto-run", "SOP": "SOP", "选择 SOP": "Choose SOP", "关闭 SOP 列表": "Close SOP list", "暂无可调用的 SOP": "No callable SOPs", "SOP 加载失败": "Failed to load SOPs", "SOP 已填入继续输入": "SOP inserted into the prompt", "SOP 内容为空": "This SOP has no executable content", "鞭挞已触发": "Auto-run triggered", "收到手动输入，鞭挞已停止": "Manual input received; auto-run stopped", "暂停鞭挞": "Pause auto-run", "继续鞭挞": "Resume auto-run", "本轮后停": "Stop after round",
  "自动放行": "Auto-allow", "总结": "Summarize", "复制上下文": "Copy context", "新对话": "New chat",
  "附件": "Attach", "停止": "Stop", "发送": "Send", "需求与工作 / 缺陷": "Work items / Defects",
  "模型角色": "Model roles", "网络与默认": "Network & defaults", "默认模式": "Default mode",
  "已记住的权限": "Saved permissions", "版本与更新": "Version & updates", "保存": "Save",
  "检查更新": "Check for updates", "下载并安装": "Download and install", "打开配置原文": "Open config", "工作资料导出": "Export work materials", "默认导出记忆、需求、缺陷和项目配置；可按需取消项目内容，导出结果会显示实际路径。": "Memory, requirements, defects, and project config are selected by default; the result path is shown.", "导出目录": "Export directory", "选择导出目录": "Choose an export directory", "选择目录": "Choose directory", "记忆": "Memory", "需求": "Requirements", "缺陷": "Defects", "项目配置": "Project config", "导出工作资料": "Export work materials", "导出完成": "Export completed",
  "测试全部连通性": "Test connectivity", "+ 添加 provider": "+ Add provider", "跟随环境变量": "Environment",
  "从已探测到的模型中选择;端点不提供列表时可手填": "Choose from detected models; type manually if the endpoint lists none",
  "重新向各 provider 探测可用模型": "Re-detect available models from all providers", "重新探测模型": "Re-detect models",
  "自动完成:安装 Ollama(winget)→ 启动服务 → 拉取 fast 模型": "One-click setup: install Ollama (winget) → start service → pull the fast model", "一键就绪子代理": "One-click subagent setup",
  "未保存 — 改动要点「保存」才会写入配置并生效": "Unsaved — changes only take effect after clicking Save",
  "直连": "Direct", "指定地址": "Custom", "dev 开发": "dev development", "research 研究": "research",
  "日志": "Logs", "当前计划": "Current plan", "回到最新": "Jump to latest", "继续文案": "Continue prompt",
  "输入任务开始 · 权限请求会弹窗询问 · Ctrl+Enter 发送 · Ctrl/Cmd+K 聚焦输入 · Ctrl/Cmd+Shift+N 新对话 · Ctrl/Cmd+Shift+C 停止":
    "Enter a task to begin · permission requests appear as dialogs · Ctrl+Enter send · Ctrl/Cmd+K focus input · Ctrl/Cmd+Shift+N new chat · Ctrl/Cmd+Shift+C stop",
  "暂无测试记录": "No test runs", "暂无排队输入": "No queued input", "暂无时间": "No time",
  "运行中": "Running", "等待模型响应": "Waiting for model response", "空闲": "Idle",
  "已复制": "Copied", "复制失败": "Copy failed", "暂无可复制的运行日志": "No runtime log to copy",
  "运行日志已复制": "Runtime log copied", "运行完成": "Run completed", "运行失败": "Run failed", "运行已停止": "Run stopped",
  "运行状态": "Run status", "工具执行中": "Tool running", "成功": "Succeeded", "失败": "Failed",
  "思考中": "Thinking", "生成中": "Generating", "等待模型": "Waiting for model", "一轮完成": "Round completed",
  "活动": "Activity", "隐藏右侧活动面板": "Hide activity panel", "显示右侧活动面板": "Show activity panel",
  "上下文压缩 · 点击查看纪要": "Context compaction · click to view summary", "上下文压缩纪要": "Context compaction summary",
  "对话小总结 · 点击查看": "Conversation summary · click to view", "展开或收起上下文压缩纪要": "Expand or collapse context compaction summary",
  "展开或收起对话总结": "Expand or collapse conversation summary",
  "需要你的回答": "Your answer is needed", "权限请求": "Permission request",
  "当前请求": "Current request", "还有": "remaining", "条待处理": "pending requests",
  "当前无其他待处理请求": "No other pending requests",
  "运行事件": "run event", "当前对话": "Current chat", "暂无": "None",
  "最近活动": "Recent activity", "排队": "Queued", "条": "items", "更新于": "Updated", "已归档": "archived",
  "展开已归档条目": "Expand archived items", "双击打开归档文件": "Double-click to open archive file",
  "外部阻塞": "Externally blocked", "阻塞": "Blocked", "可执行": "Ready", "阻塞原因": "Blocking reasons", "缺少阻塞原因": "Blocking reason missing", "解除条件": "Release condition", "下一步": "Next step", "等待项目外部条件、负责人或服务解除": "Waiting for an external condition, owner, or service", "待澄清": "Needs clarification",
  "复杂度": "Complexity", "未评估": "Not assessed", "未设": "Unset", "设置缺陷复杂度": "Set defect complexity", "设置需求复杂度": "Set requirement complexity", "复杂度已保存": "Complexity saved",
  "配置读取失败": "Failed to read configuration", "配置": "Config", "删除规则": "Delete rule", "已停止并撤销设备 token": "Stopped and revoked device token", "没有可测试的 provider": "No provider to test", "测试中": "Testing", "连通性检查完成": "Connectivity check complete", "可用": "available",
  "订阅登录态": "Subscription login", "环境变量名(可选)": "Environment variable name (optional)", "读取该环境变量作为 key": "Use this environment variable as the key", "或直接粘贴 key": "Or paste a key directly", "直填优先于环境变量;明文存 kanzei.toml": "Direct value takes precedence; stored in kanzei.toml", "已设": "Set", "缺失": "Missing", "测试": "Test", "连接": "connection", "不限": "Unlimited",
  "自动压缩": "automatic compaction", "上下文": "Context", "点击查看上下文成分": "Click to view context details",
  "连接中断": "Connection interrupted", "重放本轮": "Replaying round", "总结中": "Summarizing", "当前没有可总结的对话": "No conversation to summarize", "小总结已收纳到活动面板": "Summary added to activity panel",
  "自主推进": "Self-directed progress", "等待下一轮": "Waiting for next round", "鞭挞恢复": "Auto-run resumed", "秒后继续": "seconds until continuing",
  "已停止": "Stopped", "完成": "Completed", "用户拒绝后停止": "Stopped after user rejection", "本轮完成": "Round completed", "按你的拒绝停止": "Stopped after your rejection",
  "没有可复制的内容": "Nothing to copy", "当前没有可复制的对话": "No conversation to copy",
  "当前任务还在运行，自动鞭挞将在本轮完成后继续": "The current task is still running; auto-run will continue after this round", "先在左侧「项目」里添加并选择一个目录": "Add and select a directory under Projects first",
  "已撤销排队输入": "Queued input cancelled", "暂无测试记录": "No test runs", "撤销": "Cancel", "撤销这条排队输入": "Cancel this queued input", "跳转到": "Jump to",
  "记忆": "Memory", "检索全部记忆(FTS)": "Search all memory (FTS)", "整理 inbox": "Consolidate inbox",
  "展开或收起工具详情": "Toggle tool detail", "开发重心保存失败": "Failed to save work focus",
  "无结果(轮次中断)": "No result (round interrupted)",
  "展开筛选与排序": "Show filters and sort", "展开需求筛选与排序": "Show requirement filters",
  "展开筛选": "Show filters", "展开缺陷筛选": "Show defect filters",
  "按标签分组显示": "Group by tag", "切换需求分组显示": "Toggle requirement grouping",
  "切换缺陷分组显示": "Toggle defect grouping", "切换文档页分组显示": "Toggle documents grouping", "其他": "Other",
  "文件优先的分级记忆:所有条目都是 .kanzei/memory 下可手改的 markdown,此页与文件实时同步。":
    "File-first layered memory: every entry is a hand-editable markdown file under .kanzei/memory; this page mirrors the files.",
  "上下文账单": "Context bill", "最近一轮 system 注入的各来源字符数。": "Characters injected per source in the latest run.",
  "最近轮次": "Recent rounds", "全局记忆": "Global memory", "项目记忆": "Project memory",
  "条": "entries", "命中": "hits", "条待整理": "notes pending", "该分类暂无记忆": "No entries in this category",
  "记忆页加载失败": "Failed to load memory page", "记忆条目加载失败": "Failed to load entries",
  "记忆标题": "Title", "召回钩子": "Recall hook", "记忆正文": "Body", "来源": "Source", "引用来源": "Source refs", "冗余提醒": "redundancy tips",
  "正文摘要": "Summary", "展开全文": "Expand", "收起": "Collapse", "编辑正文": "Edit body", "无正文": "No body",
  "保存修改": "Save changes", "标题": "Title", "编辑标题": "Edit title", "编辑字段": "Edit field", "记忆已保存": "Memory saved", "记忆保存失败": "Failed to save memory", "找不到": "Not found:",
  "选择": "Select", "已选": "Selected", "改状态…": "Change status…", "混选类型,仅可改标签": "Mixed types — tags only",
  "先选择要改的状态或标签": "Pick a status or tag to apply first", "批量操作完成": "Bulk update done", "批量操作部分失败": "Bulk update partly failed",
  "内部调用": "inner calls", "只停这一条,不影响本轮其它工具": "Stop just this one; other tools in this round keep running",
  // R-173:编排派发的勘察/复核子代理在活动面板里的分区标签与超时终态。
  "勘察": "Scouting", "复核": "Review", "超时": "Timed out",
  "已停止该后台进程": "Background process stopped", "停止失败": "Failed to stop", "重跑": "Re-run",
  "把这次调用的参数填回输入框,确认后再执行": "Put this call's arguments back in the input box for you to confirm",
  "已填入输入框,确认后发送": "Filled into the input box — review, then send", "复制完整输出": "Copy full output",
  "导出": "Export", "把完整输出存成文件": "Save the full output to a file", "已导出": "Exported",
  "采纳": "adopted", "已采纳": "Adopted", "未拉取": "Not fetched", "条命中": "hits", "注入": "injected",
  "还没有召回记录:开跑时若无记忆命中,这里就是空的": "No recall records yet — if nothing matches at run start, this stays empty",
  "最近命中": "Last hit", "从未命中": "Never hit", "长期零命中": "Never recalled", "累计命中": "Total hits",
  "从磁盘删除该记忆文件,不可撤销": "Delete this memory file from disk — cannot be undone",
  "确认删除": "Delete", "此操作不可撤销": " — this cannot be undone.", "已删除": "Deleted", "删除失败": "Delete failed",
  "暂无待确认候选": "No candidates awaiting your decision", "采纳": "Adopt", "丢弃": "Discard",
  "交给记忆管理子代理提炼成条目": "Hand to the memory-manager subagent to distill into an entry",
  "已交给记忆管理子代理提炼": "Handed to the memory-manager subagent", "提炼失败": "Distillation failed",
  "直接移出候选箱,不再进入提炼范围": "Remove from the candidate box; it will not be distilled",
  "已丢弃": "Discarded", "丢弃失败": "Discard failed",
  "＋ 手填模型…": "+ Enter a model…", "填 provider:model,例如 deepseek:deepseek-chat": "Enter provider:model, e.g. deepseek:deepseek-chat",
  "格式应为 provider:model": "Format must be provider:model",
  "以下项被项目级配置覆盖,本页的改动不会生效": "Overridden by the project config — changes here will not take effect",
  "(未设 · 用内置默认)": "(unset · use built-in default)", "已重新探测": "Re-probed",
  "条被当前筛选隐藏": "hidden by the current filter", "清除筛选": "Clear filters",
  "本项目没有独立空间,正在使用上级目录的数据(与共用该上级的其它项目混在一起)": "This project has no space of its own and is reading a parent directory's data — shared with any other project under the same parent",
  "在此建立独立空间": "Create its own space here", "只在本目录创建 .kanzei,不搬动上级目录的既有条目": "Creates .kanzei here only; existing entries in the parent are left untouched",
  "已建立独立空间": "Own space created", "建立独立空间失败": "Failed to create its own space",
  "已为本项目建立独立空间": "Gave this project its own space", "以下项目仍与上级目录共用数据,切过去可一键分离": "These projects still share a parent directory's data — switch to one to separate it",
  "fast 指向外部 provider,不由本机托管": "fast points at an external provider — not managed locally",
  "子代理就绪": "Subagent ready", "Ollama 未安装": "Ollama is not installed", "Ollama 服务未运行": "Ollama service is not running",
  "模型未拉取": "Model not pulled", "子代理杂活(记忆整理/快速记录)暂不可用": "subagent chores (memory consolidation, quick capture) are unavailable",
  "子代理安装": "Subagent setup", "子代理安装失败": "Subagent setup failed",
  "运行画像加载失败": "Failed to load run metrics", "还没有轮次记录:跑一轮后这里会出现画像": "No rounds recorded yet — run once and metrics will appear here",
  "平均终端调用": "Avg terminal calls", "平均 git 查询组": "Avg git query groups", "edit 未命中率": "Edit miss rate",
  "平均步数": "Avg steps", "平均输出 token": "Avg output tokens", "近": "Last", "轮均值": "round average",
  "步": "steps", "终端": "terminal", "组": "groups", "未命中": "missed", "子代理": "subagents",
  "已完成": "Finished", "工具调用": "tool calls", "修复": "Fix-up", "token": "tokens",
  "子代理启动中": "Subagent starting", "收起子代理面板": "Collapse subagent panel", "打开子代理面板": "Open subagent panel",
  "子代理面板": "Subagent panel", "打开或收起子代理面板": "Open or collapse subagent panel",
  "清空已完成的子代理条目": "Clear finished subagent entries", "清空": "Clear",
  "只停这一条子代理,不影响本轮其它工具": "Stop only this subagent, other tools keep running",
  "已请求停止该子代理": "Stop requested for this subagent", "展开或收起子代理详情": "Expand or collapse subagent detail",
  "查看完整 transcript(工具调用序列 + 每次入参与输出)": "View full transcript (tool call sequence + each call's input and output)",
  "失败": "failed", "上下文": "context", "该轮早于度量落地,无画像": "This round predates metrics collection — no profile",
  "标记失效": "Mark stale", "恢复启用": "Reactivate", "没有命中的记忆": "No matching memory",
  "记忆检索失败": "Memory search failed", "inbox 尚有草稿未消化": "Inbox still has pending notes",
  "inbox 已整理完毕": "Inbox consolidated", "整理失败": "Consolidation failed",
  "暂无账单数据(跑一轮后生成)": "No bill yet (generated after a run)", "暂无轮次记录": "No rounds recorded",
  "暂无隔离工作树": "No isolated worktrees", "干净": "Clean", "项改动": "changed files", "差异": "Diff", "合并": "Merge", "放弃": "Discard",
  "工作树干净,没有未提交差异": "Worktree is clean; there are no uncommitted changes", "工作树差异已写入运行日志": "Worktree diff was written to the runtime log", "工作树操作完成，详细结果已写入运行日志": "Worktree operation completed; detailed results were written to the runtime log", "隔离工作树已创建": "Isolated worktree created", "放弃工作树": "Discard worktree", "未提交改动会阻止删除并保留现场": "Uncommitted changes will prevent deletion and be preserved",
  "历史消息恢复失败": "Failed to restore conversation history", "已恢复": "Restored", "历史消息": "historical messages", "组工具轨迹": "tool traces", "暂无历史对话": "No conversation history", "点击打开 · 勾选后点标题栏的删除图标批量删除": "Click to open · tick rows, then use the delete icon in the section header", "已打开历史对话": "Opened historical conversation", "先勾选要删除的历史对话": "Select conversations to delete first", "已删除": "Deleted", "份对话快照": " conversation snapshots", "历史对话加载失败": "Failed to load conversation history", "已开启新对话(历史已清空)": "New conversation started (history cleared)", "新对话:多轮历史已清空": "New conversation: multi-turn history cleared",
  "上下文占用过高,已自动压缩为纪要并延续对话": "Context was too large; it was compacted into a summary and the conversation continued", "自动压缩完成:多轮历史已替换为纪要": "Automatic compaction complete: multi-turn history replaced by a summary", "已手动停止": "Stopped manually", "已手动停止并取消": "Stopped manually and cancelled", "已取消": "cancelled", "上轮": "last round", "鞭挞停止": "Auto-run stopped", "处于暂停中,点顶栏「继续鞭挞」恢复": "paused; click \"Resume auto-run\" in the top bar to continue", "已自动取消勾选,再点鞭挞即可继续": "automatically unchecked; click Auto-run to continue", "已达连上限,点继续或重开鞭挞": "maximum consecutive rounds reached; click Continue or restart Auto-run", "上一轮没有实质动作,已追加一次具体推进指令(再无动作才会停)": "The previous round made no substantive progress; one concrete nudge was added (it stops if the next round is also inactive)", "连续两轮没有实质动作(可能目标已达成或确实无可推进项)": "Two consecutive rounds made no substantive progress (the goal may be complete or nothing can be advanced)", "连续两轮无动作,鞭挞停止": "No action for two consecutive rounds; Auto-run stopped", "无动作 · 追加推进指令": "No action · added nudge", "系统通知权限已拒绝，请在系统设置中允许后重试": "System notification permission was denied; allow it in system settings and try again", "系统通知权限未授予，完成提示将保留在应用内": "System notification permission was not granted; completion notices will remain in the app", "当前环境不支持系统通知，完成提示将保留在应用内": "System notifications are not supported here; completion notices will remain in the app", "系统通知权限请求失败": "Failed to request system notification permission", "运行中可插入或排队，按交付方式发送": "While running, send to steer or queue according to Delivery", "运行中请先完成或停止当前任务，再打开历史对话": "Finish or stop the current task before opening conversation history", "文件列表": "Files", "实际差异": "Diff", "未跟踪文件尚未包含在 git diff 中": "Untracked files are not included in git diff", "子代理启动中": "subagent starting", "历史子代理轨迹": "historical subagent trace", "历史轨迹": "historical trace", "回放": "replay", "文件": "file", "并排": "Split", "统一": "Unified", "展开或收起后台任务详情": "Expand or collapse background task details", "测试失败": "Test failed", "移除 provider": "Remove provider", "已删除权限规则": "Permission rule deleted", "删除权限规则": "Delete permission rule", "删除": "Delete", "移动端本机桥接已启动": "Local mobile bridge started", "先填写 agent id": "Enter an agent id first", "代理容器": "Agent container", "创建": "created", "升级": "upgraded", "回滚": "rolled back", "已保存": "Saved", "检查中…": "Checking…", "发现新版本": "New version found", "已是最新": "Already up to date", "检查失败": "Check failed", "下载中…(安装器就绪后会自动弹出)": "Downloading… (the installer will open when ready)", "工具": "Tool", "工具结果": "Tool result", "移动端本机桥接已停止": "Local mobile bridge stopped", "选择项目": "Select project", "移除(不删除文件)": "Remove (do not delete files)", "移除项目": "Remove project", "只解除登记,不会删除磁盘文件。": "Only unregister it; files on disk will not be deleted.", "重命名项目(只修改显示名)": "Rename project (display name only)", "重命名项目": "Rename project", "项目显示名": "Project display name", "新项目目录路径(不存在时会创建)": "New project directory (created if missing)", "项目显示名(可留空)": "Project display name (optional)", "已初始化并切换到新项目": "Initialized and switched to the new project", "项目初始化完成": "Project initialization complete", "创建进程失败": "Failed to create process", "更新进程能力失败": "Failed to update process capability", "进程模式保存失败": "Failed to save process mode", "进程思考强度保存失败": "Failed to save reasoning effort", "进程模型保存失败": "Failed to save process model", "进程列表刷新失败": "Failed to refresh process list", "待处理权限询问恢复失败": "Failed to restore pending permission requests", "已切换到进程": "Switched to process", "回答": "Answer", "权限": "Permission", "拒绝": "Deny", "总是允许": "Always allow", "自动放行失败": "Auto-allow failed", "权限应答失败": "Permission response failed", "已开启自动放行(本会话所有权限询问直接通过)": "Auto-allow enabled (all permission requests in this session pass automatically)", "已关闭自动放行": "Auto-allow disabled", "需求": "requirement", "缺陷": "defect", "自然语言描述": "Describe in natural language", "先写点描述": "Write a description first", "记录中": "Recording", "独立子代理后台进行": "independent subagent working in background", "已记录": "Recorded", "记录失败(内容已保留,可重试)": "Recording failed (content kept; retry available)", "提交": "Submit", "取消": "Cancel", "目标描述,回车创建(Esc 取消)": "Goal description, press Enter to create (Esc to cancel)", "未创建,点 ＋ 生成模板;agent 会自动遵守此文件": "Not created; click + to generate a template; the agent will follow it", "打开开发规范": "Open conventions", "个章节": " sections", "点击查看": "click to view", "规范文件已就绪": "Conventions file ready", "空": "Empty", "按 Enter 展开详情": "Press Enter to expand details", "点击展开": "click to expand", "点击循环调整优先级": "Click to cycle priority", "优先级已调整为": "Priority changed to", "转": "Move to", "记录状态/调整方向,回车保存": "Record status/adjustment, press Enter to save", "需求与缺陷已清空，自动推进已停止": "Requirements and defects are clear; Auto-run stopped", "需求与缺陷全部被阻塞，自动推进已停止": "All requirements and defects are blocked; Auto-run stopped", "自动推进停止:需求与缺陷已清空": "Auto-run stopped: requirements and defects are clear", "自动推进停止:需求与缺陷全部被阻塞": "Auto-run stopped: all requirements and defects are blocked", "检查需求/缺陷是否清空失败": "Failed to check whether requirements/defects are clear", "移除附件": "Remove attachment", "不支持的附件类型": "Unsupported attachment type", "鞭挞已暂停": "Auto-run paused", "鞭挞已恢复": "Auto-run resumed", "本轮结束后将停止鞭挞": "Auto-run will stop after this round", "已取消本轮后停": "Stop-after-round cancelled", "鞭挞上限已设为": "Auto-run limit set to", "鞭挞仅适用于自主推进模式，请先切换模式": "Auto-run only works in Self-directed progress mode; switch modes first", "鞭挞未开启:结伴开发模式不支持自动续跑": "Auto-run not enabled: paired development mode does not support continuation", "鞭挞已开启:每轮结束自动推进目标": "Auto-run enabled: advance the goal after each round", "鞭挞已关闭": "Auto-run disabled", "鞭挞启动,2 秒后开始…": "Auto-run starting in 2 seconds…", "当前模式不支持鞭挞，已自动关闭": "The current mode does not support Auto-run; it was disabled", "鞭挞已关闭：当前进程不是自主推进模式": "Auto-run disabled: the current process is not Self-directed progress", "复制": "Copy", "复制消息": "Copy message", "可压缩重试": "Retry after compaction", "可重试错误": "Retryable error", "致命错误": "Fatal error", "重试上一次请求": "Retry last request", "正在重试…": "Retrying…", "思考中…": "Thinking…", "拖动调整面板宽度": "Drag to adjust panel width", "调整面板宽度": "Adjust panel width", "展开或收起思考过程": "Expand or collapse reasoning", "切换差异并排或统一视图": "Toggle split or unified diff view", "已复制": "Copied", "继续文案已升级到新版(含【阻塞】刹车约定)": "Continue prompt upgraded to the new version (with the blocked brake rule)", "已请求停止(本地已复位)": "Stop requested (local state reset)", "模型:agent 默认": "Model: agent default", "暂无排队输入": "No queued input", "任务运行中,先停止再开新对话": "Stop the running task before starting a new conversation", "先选择一个项目": "Select a project first", "开始总结当前对话…": "Starting conversation summary…", "轮": "rounds", "耗时": "duration",
  "已请求停止": "Stop requested",
  "切换到对话": "Switch to chat",
  "切换到工作区": "Switch to workspace",
  "切换到需求与工作和缺陷": "Switch to requirements, work, and defects",
  "切换到记忆": "Switch to memory",
  "运行画像": "Run profile",
  "切换到运行画像": "Switch to run profile",
  "切换到设置": "Switch to settings",
  "初始化新项目目录": "Initialize a new project directory",
  "添加项目目录": "Add project directory",
  "删除勾选的对话": "Delete selected conversations",
  "刷新并自动归档已完成测试": "Refresh and archive completed tests",
  "刷新工作树差异": "Refresh worktree changes",
  "新建目标": "Create goal",
  "打开 goals.md(活跃目标会注入 agent 上下文,没有明确任务时它会自主推进)": "Open goals.md (active goals are injected into agent context and advanced when no task is given)",
  "打开 goals.md": "Open goals.md",
  "快速记需求:自然语言描述交给独立子代理结构化落库,不打断当前对话": "Quick requirement: send a natural-language description to an independent subagent without interrupting this chat",
  "快速记录需求": "Quickly record requirement",
  "打开 requirements.md 原文": "Open requirements.md source",
  "按状态筛选": "Filter by status",
  "按复杂度筛选": "Filter by complexity",
  "按优先级筛选": "Filter by priority",
  "按优先级筛选(仅参考,不影响取活顺序)": "Filter by priority (reference only; does not affect work order)",
  "点击循环调整优先级(仅参考,不影响取活)": "Click to cycle priority (reference only; does not affect work order)",
  "按标签筛选": "Filter by tag",
  "按阻塞状态筛选": "Filter by blocked state",
  "需求排序(手动=拖拽定开发顺序,agent 按此取活)": "Requirement order (Manual means drag to set the agent's work order)",
  "快速记缺陷:自然语言描述交给独立子代理结构化落库,不打断当前对话": "Quick defect: send a natural-language description to an independent subagent without interrupting this chat",
  "快速记录缺陷": "Quickly record defect",
  "打开 defects.md 原文": "Open defects.md source",
  "打开 research/report.md": "Open research/report.md",
  "创建规范模板": "Create conventions template",
  "打开 conventions.md(agent 会遵守此文件)": "Open conventions.md (the agent follows this file)",
  "打开 conventions.md": "Open conventions.md",
  "项目进程": "Project processes",
  "为当前项目新建独立进程": "Create an independent process for this project",
  "清空多轮对话历史,开一段新会话": "Clear multi-turn history and start a new chat",
  "显示/隐藏右侧活动面板": "Show or hide the activity panel",
  "折叠/展开左侧栏": "Collapse or expand the left sidebar",
  "每轮跑完自动发「继续推进目标」": "Automatically continue after each round",
  "打开低频操作菜单": "Open additional actions",
  "暂停/恢复自动鞭挞": "Pause or resume auto-run",
  "当前轮完成后停止鞭挞": "Stop auto-run after this round",
  "鞭挞上限(1-100)": "Auto-run limit (1–100)",
  "本次不再弹权限窗,全部自动放行(相当于 yolo)": "Automatically allow all permissions for this session",
  "创建隔离 Git 工作树线程": "Create isolated Git worktree thread",
  // 「勘察复核」= 阶段流水线总闸(2026-08-11 用户定调)。它不是「有没有子代理」的开关
  // ——关着的时候模型照样能自己派 task,所以文案与译名都不能再出现「启用子代理」的说法。
  "勘察复核": "Scout & review",
  "开启后本进程每个任务强制走:并行勘察 → 实现 → 并行复核 →(有发现时)修正;关闭时是一问一答,模型仍可自己派子代理。成本:每轮多 5 个勘察 + 3 个复核子代理,复核只要不是全部回 NO_ISSUES(失败/超时也算有发现)就再多跑一段修正,弱模型下几乎每轮都会跑;并行角色数上限由 [limits] max_tasks_per_turn 控制。": "When on, every task in this process is forced through: parallel scouting → implementation → parallel review → fixup (when there are findings). When off it is plain question-and-answer, and the model can still dispatch its own subagents. Cost: 5 scouting + 3 review subagents per round, plus an extra fixup pass whenever review does not come back all NO_ISSUES (failures and timeouts count as findings), so a weak model will run it almost every round. The parallel role cap is [limits] max_tasks_per_turn.",
  "用 fast 模型总结当前对话并存档到 .kanzei/summaries/": "Summarize this chat with the fast model and archive it under .kanzei/summaries/",
  "把当前对话(含思考/工具轨迹摘要)复制为 markdown,方便贴给其他 AI": "Copy this chat, including reasoning and tool traces, as Markdown",
  "搜索当前对话": "Search this chat",
  "搜索对话": "Search chat",
  "上一个匹配": "Previous match",
  "下一个匹配": "Next match",
  "模型(agent 默认或直选)": "Model (agent default or direct selection)",
  "思考强度(仅推理模型有效;越高越慢越贵)": "Reasoning effort (higher is slower and more expensive)",
  "模式": "Mode",
  "自动推进取活顺序": "Automatic work priority",
  "想做什么?可粘贴/拖拽图片或 PDF": "What would you like to do? Paste or drop images or PDFs",
  "展开或收起继续文案编辑": "Expand or collapse continuation prompt editor",
  "选择已沉淀的 SOP": "Choose a saved SOP",
  "按当前文案继续推进": "Continue with the current prompt",
  "添加图片或 PDF": "Add image or PDF",
  "输入交付方式": "Input delivery mode",
  "切换当前项目": "Switch current project",
  "需求与缺陷并排对照，便于核对互相引用的条目": "Compare requirements and defects side by side to inspect cross-references",
  "批量操作": "Bulk actions",
  "批量改状态": "Bulk change status",
  "批量改标签": "Bulk change tag",
  "检索记忆": "Search memory",
  "立即让 memory-manager 消化 inbox 草稿": "Consolidate inbox drafts now",
  "记忆架构总览": "Memory architecture overview",
  "记忆条目列表": "Memory entries",
  "跨轮趋势": "Cross-round trends",
  "逐轮画像": "Per-round profile",
  "重试上次失败操作": "Retry the last failed action",
  "复制日志": "Copy log",
  "复制运行日志": "Copy runtime log",
  "清空": "Clear",
  "清空日志": "Clear log",
  "git 分支 · 未提交改动": "Git branch · uncommitted changes",
  "上下文占用": "Context usage",
  "查看上下文成分": "View context components",
  "显示/隐藏运行日志": "Show or hide runtime log",
  "按工具类型筛选": "Filter by tool type",
  "按成败状态筛选": "Filter by result",
  "输入你的回答": "Enter your answer",
  "写入项目 .kanzei/kanzei.toml,之后不再询问": "Save to project .kanzei/kanzei.toml and stop asking",
  "在外部编辑器打开": "Open in external editor",
  "关闭": "Close",
  "关闭查看器": "Close viewer",
  "全选": "Select all",
  "隔离工作树": "Isolated worktrees",
  "小": "Small",
  "中": "Medium",
  "大": "Large",
  "全部执行状态": "All execution states",
  "已阻塞": "Blocked",
  "状态": "Status",
  "侧栏": "Sidebar",
  "更多": "More",
  "搜索": "Search",
  "思考:默认": "Reasoning: default",
  "思考:关闭": "Reasoning: off",
  "思考:低": "Reasoning: low",
  "思考:中": "Reasoning: medium",
  "思考:高": "Reasoning: high",
  "结伴开发": "Paired development",
  "缺陷优先": "Defect-first",
  "需求优先": "Requirement-first",
  "排队 queue": "Queue",
  "插入 steer": "Steer",
  "统一查看项目、对话、运行状态和最近活动。": "View projects, chats, run status, and recent activity together.",
  "深度管理页面：拖拽排序、字段编辑与批量操作都在这里；侧栏只负责浏览与取活。": "Use this page for ordering, field editing, and bulk actions; the sidebar is for browsing and picking work.",
  "对照": "Compare",
  "依赖视图": "Dependency view",
  "按依赖拓扑分层展示：可做层/被阻塞层，点击条目高亮其依赖链": "Layer items by dependency topology: ready / blocked; click an item to highlight its dependency chain",
  "可做(依赖已满足)": "Ready (dependencies satisfied)",
  "被阻塞(还有未完成依赖)": "Blocked (unfinished dependencies)",
  "暂无依赖关系": "No dependencies",
  "被依赖": "depended on by",
  "依赖": "depends on",
  "改标签…": "Change tag…",
  "应用": "Apply",
  "取消选择": "Clear selection",
  "待确认候选": "Pending candidates",
  "完成一个完整条目后自动提炼的 SOP 候选。候选不会自己入库——采纳才交给记忆管理子代理提炼成条目，丢弃则直接移出。": "SOP candidates extracted after completing an item. Candidates are stored only after you accept them; discard removes them.",
  "召回评估": "Recall evaluation",
  "空闲整理清单": "Idle cleanup list",
  "零采纳候选 = 召回≥3 但从未拉正文(语义显著却决策无关);复发候选 = 召回频次高的活跃条目。处置走既有墓碑机制(降级/修订),不静默删。": "Zero-adopt candidates are recalled ≥3 but never fetched; recurring candidates are active entries with frequent recalls. Cleanup uses the existing tombstone flow (demote/revise), never silent deletion.",
  "零采纳候选": "Zero-adopt candidate",
  "一键整理": "Clean up now",
  "对零采纳候选(召回≥3 采纳=0)一键降级为 stale,可逆不删": "Demote zero-adopt candidates (recalled ≥3, never fetched) to stale in one click; reversible, never deleted.",
  "已降级": "Demoted",
  "条记忆为 stale": "memory entries to stale",
  "跳过": "skipped",
  "无候选可降级": "Nothing to demote",
  "无零采纳候选需要整理": "No zero-adopt candidates to clean up",
  "复发候选": "Recurring candidate",
  "召回": "Recalled",
  "采纳": "Fetched",
  "暂无零采纳或复发候选": "No zero-adopt or recurring candidates",
  "每轮开跑时按 prompt 预检索命中了什么。「已采纳」= 召回后正文确实被拉取过；只注入索引行而没拉正文，就是召回了但没用上。": "Shows prompt-based recall hits for each round. Accepted means the full entry was fetched; an injected index row alone did not affect the run.",
  "每轮的上下文占用、token、工具分布与冗余指标。统计口径与 R-099 的冗余治理度量同源，不各算各的。": "Context usage, tokens, tool distribution, and redundancy per round, using the same metrics as R-099.",
  "写入全局": "Save globally",
  ",项目级 .kanzei/kanzei.toml 会覆盖全局。": "; project .kanzei/kanzei.toml overrides global settings.",
  "界面语言": "Interface language",
  "主循环": "Main loop",
  "子代理/杂活(本地模型)": "Subagents and background work (local model)",
  "名称": "Name",
  "协议": "Protocol",
  "API Key(环境变量或直填 + 测试)": "API key (environment variable or direct entry + test)",
  "上下文(token)": "Context (tokens)",
  "代理": "Proxy",
  "思考强度": "Reasoning effort",
  "关闭(不发思考参数)": "Off (do not send reasoning parameters)",
  "低": "Low",
  "高": "High",
  "默认档,顶栏可按进程临时覆盖。仅推理模型(Claude 思考、o 系/gpt-5 等)有效; 开启后 Anthropic 会按档位分配思考预算并自动抬高输出上限,OpenAI 系发送 reasoning effort。": "Default level; the top bar can override it per process. It applies only to reasoning models. Anthropic allocates a reasoning budget and OpenAI receives reasoning effort.",
  "Codex Fast mode": "Codex Fast mode",
  "批次": "Batches",
  "运行上限": "Runtime limits",
  "默认": "default",
  "留空 = 用内置默认(输入框里的灰字就是默认值)。只有填了的项会写进配置,今后默认值变了也跟着变。":
    "Leave blank to use the built-in default (shown as placeholder text). Only the fields you fill in are written to the config, so blanks follow future default changes.",
  "主对话输出上限": "Main output cap",
  "子代理输出上限": "Subagent output cap",
  "子代理墙钟上限": "Subagent wall clock",
  "秒": "sec",
  "单轮子代理数上限": "Subagents per turn",
  "压缩触发线": "Compaction trigger",
  "占窗口比例": "share of window",
  "压缩保留近期": "Keep recent",
  "比例": "ratio",
  "单波并行工具数": "Parallel tools per wave",
  "流中断重放次数": "Stream replay attempts",
  "传输重试次数": "Transport retries",
  "限流重试次数": "Rate-limit retries",
  "验证与提交节奏": "Verification & commit cadence",
  // R-170 之后继续文案不再渲染节奏规则,而 KanzeiConfig::merge() 也从未合并 [cadence]:
  // 全仓没有任何消费方。原文案承诺"下次注入的继续文案按新节奏渲染"是对用户的假承诺,
  // 与本批要治的"界面显示 A、运行用 B"同族,先如实说明;接回引擎后再改回去。
  "留空 = 用内置默认(§1.4 当前值)。改动写进 kanzei.toml [cadence],但引擎当前还没有接回这组参数(R-170 把规则从继续文案剥离后暂无消费方),改了不会改变 agent 的验证节奏;发版门禁与 CI 全量不受参数影响。":
    "Leave blank to use the built-in default (current §1.4 values). Changes are written to [cadence] in kanzei.toml, but the engine does not consume these parameters yet (nothing reads them back after R-170 moved the rules out of the continue prompt), so changing them will not change the agent's verification cadence; the release gate and CI full test suite are unaffected by these parameters.",
  "全量测试": "Full test suite",
  "条目关闭前": "Before entry close",
  "每次提交前": "Before every commit",
  "每 N 批": "Every N batches",
  "仅发版前": "Release only",
  "每 N 批的间隔": "Interval in batches for every-N-batches",
  "定向测试": "Targeted tests",
  "提交粒度": "Commit granularity",
  "每批一提交": "One commit per batch",
  "每条目一提交": "One commit per entry",
  "push 频率": "Push frequency",
  "条目完成后": "After entry completion",
  "每提交后": "After every commit",
  "定期自动": "Periodically (automatic)",
  "仅对 Codex 生效：仍使用当前模型（例如 luna），但会增加积分消耗以换取更快响应。": "Codex only: keeps the current model (for example, luna) but consumes more credits for faster responses.",
  "移动端桥接": "Mobile bridge",
  "仅监听本机回环地址；启动后把一次性配对 token 提供给移动端，停止服务即撤销。": "Listens only on loopback. Starting provides a one-time pairing token; stopping revokes it.",
  "本机服务": "Local service",
  "启动": "Start",
  "停止并撤销": "Stop and revoke",
  "代理管理容器": "Agent management container",
  "升级到 2": "Upgrade to 2",
  "当前项目中选择“总是允许”后保存的放行规则。删除后下次匹配时会再次询问。": "Rules saved after choosing Always allow in this project. Deleting a rule makes the next match ask again.",
  "操作": "Action",
  "资源": "Resource",
  "当前项目没有已记住的放行规则。": "This project has no remembered allow rules.",
  "当前版本": "Current version",
  "运行日志": "Runtime log",
  "全部类型": "All types",
  "追踪": "Tracking",
  "记住为": "Remember as",
  "允许一次": "Allow once",
  "提交回答": "Submit answer",
};
const I18N_DYNAMIC_EN = {
  "完成提示音不可用": "Completion sound unavailable",
  "系统通知不可用": "System notification unavailable",
  "展开左侧栏": "Expand left sidebar",
  "折叠左侧栏": "Collapse left sidebar",
  "打开侧栏": "Open sidebar",
  "收起侧栏": "Close sidebar",
  "打开/收起侧栏": "Open or close the sidebar",
  "打开或收起侧栏": "Open or close the sidebar",
  "仍在等待模型首个响应": "Still waiting for the model's first response",
  "模型开始响应": "Model started responding",
  "模型": "Model",
  "上下文上限": "Context limit",
  "上下文成分": "Context components",
  "输入上下文(系统/历史/工具结果)": "Input context (system/history/tool results)",
  "缓存读取(已复用上下文)": "Cache read (reused context)",
  "本轮输出": "Output this round",
  "最近一次压缩纪要已收进活动面板": "The latest compaction summary is in the activity panel",
  "合计": "Total",
  "出错": "Error",
  "出错中止": "Stopped after error",
  "已请求停止": "Stop requested",
  "运行中插入": "Steered while running",
  "运行中排队": "Queued while running",
  "已插入当前会话，将优先执行": "Inserted into the current session and will run first",
  "已加入队列，将按顺序执行": "Added to the queue and will run in order",
  "已存档": "Archived",
  "连续推进上限": "Continuous progress limit",
  // 自主推进不再自带七阶段:鞭挞开着而「勘察复核」关着时,顶栏必须明说,否则用户会
  // 沿用旧心智模型以为每轮都在勘察(2026-08-11 换闸门带来的行为变化)。
  "勘察复核未开(每轮直接实现)": "Scout & review is off (each round goes straight to implementation)",
  "勘察复核已开启:每个任务强制走勘察→实现→复核": "Scout & review on: every task is forced through scouting → implementation → review",
  "勘察复核已关闭:恢复一问一答,模型仍可自己派子代理": "Scout & review off: back to plain question-and-answer; the model can still dispatch its own subagents",
  "已切换为需求优先": "Switched to requirement-first",
  "已切换为缺陷优先": "Switched to defect-first",
  "停止指令失败": "Stop command failed",
  "未选择项目": "No project selected",
  "项目文档刷新失败": "Failed to refresh project documents",
  "读取附件失败": "Failed to read attachment",
  "复制运行日志失败": "Failed to copy runtime log",
  "复制上下文失败": "Failed to copy context",
  "状态流转失败": "Status transition failed",
  "连接中断,正在重新请求本轮": "Connection interrupted; requesting this round again",
  "连接中断,重放本轮": "Connection interrupted; replaying this round",
  "等待": "waiting",
  "本轮完成": "Round completed",
  "运行事件": "run event",
  "需求与工作": "Work items",
  "缺陷": "Defects",
  "核心": "Core",
  "后端": "Backend",
  "前端": "Frontend",
  "模型": "Model",
  "发布": "Release",
  "流程": "Process",
  "其他": "Other",
  "排序保存失败": "Failed to save order",
  "优先级保存失败": "Failed to save priority",
  "复杂度保存失败": "Failed to save complexity",
  "状态流转失败": "Status transition failed",
  "测试记录刷新失败": "Failed to refresh test records",
  "项目列表刷新失败": "Failed to refresh project list",
  "模型列表刷新失败": "Failed to refresh model list",
  "历史消息恢复失败": "Failed to restore history",
  "项目文档刷新失败": "Failed to refresh project documents",
  "切换项目失败": "Failed to switch project",
  "工作区刷新失败": "Failed to refresh workspace",
  "记忆页加载失败": "Failed to load memory page",
  "记忆条目加载失败": "Failed to load memory entries",
  "记忆保存失败": "Failed to save memory",
  "记忆检索失败": "Memory search failed",
  "项目初始化失败": "Failed to initialize project",
  "创建进程失败": "Failed to create process",
  "更新进程能力失败": "Failed to update process capability",
  "进程模式保存失败": "Failed to save process mode",
  "思考强度保存失败": "Failed to save reasoning effort",
  "进程模型保存失败": "Failed to save process model",
  "停止指令失败": "Stop command failed",
  "读取附件失败": "Failed to read attachment",
  "复制上下文失败": "Failed to copy context",
  "全部状态": "All statuses",
  "全部优先级": "All priorities",
  "全部标签": "All tags",
  "全部复杂度": "All complexity",
  "手动": "Manual",
  "优先级": "Priority",
  "编号": "ID",
  "模型列表已刷新": "Model list refreshed",
  "模型列表获取失败": "Failed to load model list",
  "撤销失败": "Failed to cancel",
  "队列刷新失败": "Failed to refresh queue",
  "测试记录刷新失败": "Failed to refresh test records",
  "进程列表刷新失败": "Failed to refresh process list",
  "待处理权限询问恢复失败": "Failed to restore pending permission requests",
  "项目初始化完成": "Project initialization complete",
  "已初始化并切换到新项目": "Initialized and switched to the new project",
  "已切换到进程": "Switched to process",
  "更新进程能力失败": "Failed to update process capability",
  "新项目目录路径(不存在时会创建)": "New project directory (created if missing)",
  "项目显示名(可留空)": "Project display name (optional)",
  "已初始化并切换到新项目": "Initialized and switched to the new project",
  "文件补全失败": "Failed to complete file suggestions",
  "继续文案已升级到新版(含【阻塞】刹车约定)": "Continue prompt upgraded (with the blocked brake rule)",
  "鞭挞已暂停": "Auto-run paused",
  "鞭挞已恢复": "Auto-run resumed",
  "本轮结束后将停止鞭挞": "Auto-run will stop after this round",
  "已取消本轮后停": "Stop-after-round cancelled",
  "鞭挞上限已设为": "Auto-run limit set to",
  "鞭挞仅适用于自主推进模式，请先切换模式": "Auto-run only works in self-directed progress mode; switch modes first",
  "鞭挞未开启:结伴开发模式不支持自动续跑": "Auto-run is disabled: paired development does not support continuation",
  "鞭挞已开启:每轮结束自动推进目标": "Auto-run enabled: advance the goal after each round",
  "鞭挞已关闭": "Auto-run disabled",
  "鞭挞启动,2 秒后开始…": "Auto-run starts in 2 seconds…",
  "当前模式不支持鞭挞，已自动关闭": "Auto-run disabled: current mode does not support it",
  "鞭挞已关闭：当前进程不是自主推进模式": "Auto-run disabled: current process is not self-directed",
  "已请求停止(本地已复位)": "Stop requested (local state reset)",
  "队列刷新失败": "Failed to refresh queue",
  "工作树操作完成，详细结果已写入运行日志": "Worktree operation completed; details written to runtime log",
  "创建工作树失败": "Failed to create worktree",
  "工作树已不可用": "Worktree unavailable",
  "进程列表刷新失败": "Failed to refresh process list",
  "规范文件已就绪": "Conventions file ready",
  "删除失败": "Delete failed",
  "读取权限规则失败": "Failed to read permission rules",
  "设置读取失败": "Failed to read settings",
  "启动移动端桥接失败": "Failed to start mobile bridge",
  "停止移动端桥接失败": "Failed to stop mobile bridge",
  "保存失败": "Save failed",
  "检查中": "Checking",
  "发现新版本": "New version found",
  "已是最新": "Already up to date",
  "检查失败": "Check failed",
  "下载中": "Downloading",
  "获取版本失败": "Failed to get version",
  "启动步骤": "Startup step",
  "加载失败": "Failed to load",
  "后台会话控制事件已路由": "Background session control event routed",
  "事件订阅失败": "Event subscription failed",
  "界面将收不到运行事件,请反馈": "The UI will not receive run events; please report this issue",
  "订阅高峰或网络较慢时属正常;超时上限": "This is normal during subscription peaks or slow networks; timeout limit",
  "桌面端启动": "desktop started",
  "设置页「检查更新」可一键安装": "Install it from Check for updates in Settings",
  "错误": "Error",
  "复制失败": "Copy failed",
  "已复制上下文": "Context copied",
  "段": "sections",
  "图片": "Image",
  "已发送给 agent": "Sent to agent",
  "已截断": "truncated",
  "新建": "Created",
  "用户": "User",
  "助手": "Assistant",
  "准备中": "Preparing",
  "正在发送": "Sending",
  "个附件": "attachments",
  "发送": "Send",
  "鞭挞": "Auto-run",
  "看一下这些附件": "Please inspect these attachments",
  "收起文案": "Collapse prompt",
  "最近提交": "Latest commit",
  "选择工作区项目": "Select workspace project",
  "个可选": "available",
  "撤销失败": "Cancel failed",
  "工作树已不可用": "Worktree unavailable",
  "创建工作树失败": "Failed to create worktree",
  "记录保存失败": "Failed to save record",
  "复杂度已保存": "Complexity saved",
  "运行画像加载失败": "Failed to load run profile",
  "提炼失败": "Extraction failed",
  "丢弃失败": "Discard failed",
  "选择导出目录失败": "Failed to choose export directory",
  "导出失败": "Export failed",
  "总结完成,已收纳并存档": "Summary completed and archived",
  "总结失败": "Summarization failed",
  "没有匹配": "No matches for",
  "匹配": "Matched",
  "个": "items",
  "不可见": "hidden",
  "未知探针类型": "Unknown probe type",
  "探针执行失败": "Probe failed",
  "自加载以来没有 console 错误或警告。": "No console errors or warnings since load.",
  "盒模型": "Box model",
  "思考": "Reasoning",
  "插入": "Steer",
  "输入任务开始 · 权限请求会弹窗询问 · Ctrl+Enter 发送": "Enter a task to begin · permission requests will prompt · Ctrl+Enter to send"
};
const I18N_ZH = new WeakMap();
const I18N_ATTR_ZH = new WeakMap();
// 长词优先,避免短词先命中把长词切碎;静态与动态资源共用同一复合文案入口。
const I18N_LOCALIZE_ENTRIES = [...Object.entries(I18N_EN), ...Object.entries(I18N_DYNAMIC_EN)]
  .sort(([a], [b]) => b.length - a.length);
const I18N_SOURCE_BY_EN = new Map(
  [...Object.entries(I18N_EN), ...Object.entries(I18N_DYNAMIC_EN)].map(([source, translated]) => [translated, source])
);
const I18N_REVERSE_ENTRIES = [...I18N_SOURCE_BY_EN.entries()]
  .sort(([a], [b]) => b.length - a.length);
// 紧邻这些字符时说明命中的是路径/标识符的一部分,不是产品文案。
const I18N_TOKEN_BOUNDARY = /[\\/._\-a-zA-Z0-9]/;

/// 只替换"独立出现"的产品文案。
/// 原实现对整段自由文本无边界 replaceAll,而该函数同时用于渲染用户输入、模型输出与
/// 文件路径,于是英文模式下 `crates/前端/模型.md` 被改写成 `crates/Frontend/Model.md`
/// ——展示层篡改用户数据(D-135)。现在命中处紧邻路径分隔符或 ASCII 标识符字符即跳过。
function replaceStandalone(text, source, translated) {
  if (!source) return text;
  let out = "";
  let index = 0;
  for (;;) {
    const at = text.indexOf(source, index);
    if (at < 0) return out + text.slice(index);
    const before = at > 0 ? text[at - 1] : "";
    const after = text[at + source.length] ?? "";
    const inToken = I18N_TOKEN_BOUNDARY.test(before) || I18N_TOKEN_BOUNDARY.test(after);
    out += text.slice(index, at) + (inToken ? source : translated);
    index = at + source.length;
  }
}
function localizeDynamic(value) {
  const text = String(value ?? "");
  if (!languageIsEnglish()) return text;
  // 整串命中优先:状态文案通常整句就是一个 key,这条路径最准也最快。
  const trimmed = text.trim();
  const whole = I18N_DYNAMIC_EN[trimmed] ?? I18N_EN[trimmed];
  if (whole) return text.replace(trimmed, whole);
  let out = text;
  for (const [source, translated] of I18N_LOCALIZE_ENTRIES) {
    out = replaceStandalone(out, source, translated);
  }
  return out;
}
function sourceFromLocalized(value) {
  const text = String(value ?? "");
  if (!languageIsEnglish()) return text;
  const trimmed = text.trim();
  const exact = I18N_SOURCE_BY_EN.get(trimmed);
  if (exact) return text.replace(trimmed, exact);
  let out = text;
  for (const [translated, source] of I18N_REVERSE_ENTRIES) {
    if (translated.length < 4) continue;
    out = replaceStandalone(out, translated, source);
  }
  return out;
}
function languageIsEnglish() {
  return (localStorage.getItem("kz-language") || "zh") === "en";
}
function t(key) {
  const language = localStorage.getItem("kz-language") || "zh";
  return language === "en" ? (I18N_EN[key] || I18N_DYNAMIC_EN[key] || key) : key;
}
function localizedDocStatus(status) {
  const labels = { todo: "To do", doing: "In progress", done: "Done", dropped: "Dropped", fixing: "Fixing", fixed: "Fixed", open: "Open", wontfix: "Won't fix" };
  return languageIsEnglish() ? (labels[status] || status) : status;
}
let applyingLanguage = false;
// D-202:本地化一律按 root 作用域执行,全文档重扫只留给初始化与切语言。
// 原先 observer 把**每一次** DOM 变动都放大成一次整页 TreeWalker + 整页属性扫描,
// 而流式输出每个 delta 都在改 DOM、单次成本又 ∝ 当前对话的文本节点数——
// 轮次越多越卡的主因就在这里(几百轮后单次重扫足以吃满一帧,点击排不上队)。
function localizeTextNode(node, language) {
  const parent = node.parentElement || node.parentNode;
  if (parent?.closest?.("[data-i18n-raw]")) return;
  if (!I18N_ZH.has(node)) {
    I18N_ZH.set(node, sourceFromLocalized(node.nodeValue));
  } else {
    const cached = I18N_ZH.get(node);
    const cachedTranslation = I18N_EN[cached.trim()] || I18N_DYNAMIC_EN[cached.trim()] || localizeDynamic(cached);
    if (node.nodeValue !== cached && node.nodeValue !== cachedTranslation) {
      I18N_ZH.set(node, sourceFromLocalized(node.nodeValue));
    }
  }
  const source = I18N_ZH.get(node);
  const key = source.trim();
  if (!key) return;
  const exact = I18N_EN[key] || I18N_DYNAMIC_EN[key];
  const next = language === "en"
    ? (exact ? source.replace(key, exact) : localizeDynamic(source))
    : source;
  if (next.length > 1_000_000) {
    throw new Error(`i18n text expansion detected:length=${next.length},key=${key.slice(0, 80)}`);
  }
  if (node.nodeValue !== next) node.nodeValue = next;
}
function localizeAttributes(element, language) {
  let originals = I18N_ATTR_ZH.get(element);
  if (!originals) {
    originals = new Map();
    I18N_ATTR_ZH.set(element, originals);
  }
  for (const attribute of ["title", "placeholder", "aria-label"]) {
    const value = element.getAttribute(attribute);
    if (!value) continue;
    // 原文缓存必须跟随写入方更新:只认首见值会把属性永久冻结在第一次的取值上
    // (侧栏 tooltip 折叠后仍显示「折叠左侧栏」),状态提示从此长期说谎(D-136)。
    // 判据:当前值既不是缓存原文也不是它的译文 ⇒ 是别处新写进来的,以新值为准。
    const cached = originals.get(attribute);
    if (cached === undefined) {
      originals.set(attribute, sourceFromLocalized(value));
    } else if (value !== cached) {
      const cachedTranslation = I18N_EN[cached.trim()] || I18N_DYNAMIC_EN[cached.trim()] || localizeDynamic(cached);
      if (value !== cachedTranslation) originals.set(attribute, sourceFromLocalized(value));
    }
    const source = originals.get(attribute);
    const key = source.trim();
    const next = language === "en" ? (I18N_EN[key] || localizeDynamic(source)) : source;
    // 同值也调 setAttribute 会照常入 MutationObserver 队列(DOM 规范如此),而 observer
    // 正监听这三个属性:无条件写 = observer→applyLanguage→写 的微任务死循环,主线程
    // 饿死、永不绘制,表现为启动黑屏(D-172)。写属性前必须比对,值没变绝不写。
    if (value !== next) element.setAttribute(attribute, next);
  }
}
/// 只本地化 root 这一棵子树;root 也可以是文本节点(characterData 变动的目标)。
function localizeRoot(root, language) {
  if (!root) return;
  if (typeof root.querySelectorAll !== "function") {
    if (root.nodeValue != null) localizeTextNode(root, language);
    return;
  }
  const walker = document.createTreeWalker(root, NodeFilter.SHOW_TEXT);
  while (walker.nextNode()) localizeTextNode(walker.currentNode, language);
  if (root.hasAttribute?.("title") || root.hasAttribute?.("placeholder") || root.hasAttribute?.("aria-label")) {
    localizeAttributes(root, language);
  }
  root.querySelectorAll("[title], [placeholder], [aria-label]").forEach((element) => localizeAttributes(element, language));
}
function localizeNodes(nodes) {
  if (applyingLanguage) return;
  applyingLanguage = true;
  try {
    const language = localStorage.getItem("kz-language") || "zh";
    for (const node of nodes) localizeRoot(node, language);
  } finally {
    applyingLanguage = false;
  }
}
function applyLanguage() {
  if (applyingLanguage) return;
  applyingLanguage = true;
  try {
    const language = localStorage.getItem("kz-language") || "zh";
    document.documentElement.lang = language === "en" ? "en" : "zh-CN";
    localizeRoot(document.body, language);
  } finally {
    applyingLanguage = false;
  }
}
const languageSelect = $("language-select");
languageSelect.value = localStorage.getItem("kz-language") || "zh";
languageSelect.addEventListener("change", () => {
  localStorage.setItem("kz-language", languageSelect.value);
  applyLanguage();
  syncDynamicUiLanguage();
  syncActivityPanel();
  syncSidebar();
  if (document.querySelector("#providers-table tbody")?.children.length) renderProviders();
  $("status-tokens").title = t("点击查看上下文成分");
  if (lastWorkspaceSnapshot) renderWorkspace(lastWorkspaceSnapshot);
  if (document.body.classList.contains("documents-active")) refreshDocs();
  if (currentProject) {
    refreshWorktrees();
    refreshConversationList();
  }
  if (askActive) $("ask-title").textContent = askActive.kind === "question" ? t("需要你的回答") : t("权限请求");
  updateAskQueueStatus();
});
applyLanguage();
// D-202:只本地化本次变动带进来的节点。records 为空(或宿主不给 records)时不做任何事——
// 全量本地化由初始化与切语言时的 applyLanguage() 负责,这里绝不能退回全文档重扫。
const languageObserver = new MutationObserver((records) => {
  const roots = [];
  for (const record of records) {
    if (record.type === "childList") {
      for (const node of record.addedNodes) roots.push(node);
    } else {
      roots.push(record.target);
    }
  }
  if (roots.length) localizeNodes(roots);
});
languageObserver.observe(document.body, {
  childList: true,
  subtree: true,
  characterData: true,
  attributes: true,
  attributeFilter: ["title", "placeholder", "aria-label"],
});
