// kanzei 桌面端前端逻辑(静态,无构建步骤)。
const { invoke } = window.__TAURI__.core;
const { listen } = window.__TAURI__.event;

// R-126:自加载起累积 console 错误与未捕获异常,供 ui_console 工具取样。
// 必须在最前面装:晚一步就漏掉初始化阶段的错误,而那正是最要命的一段。
const uiConsoleLog = [];
const UI_CONSOLE_MAX = 200;
function recordConsole(level, args) {
  if (uiConsoleLog.length >= UI_CONSOLE_MAX) uiConsoleLog.shift();
  uiConsoleLog.push({
    level,
    at: Date.now(),
    text: args.map((a) => (a instanceof Error ? `${a.message}\n${a.stack ?? ""}` : String(a))).join(" "),
  });
}
for (const level of ["error", "warn"]) {
  const original = console[level].bind(console);
  console[level] = (...args) => {
    recordConsole(level, args);
    original(...args);
  };
}
window.addEventListener("error", (event) => {
  recordConsole("uncaught", [event.message, event.filename ? `${event.filename}:${event.lineno}` : ""]);
});
window.addEventListener("unhandledrejection", (event) => {
  recordConsole("unhandled-rejection", [event.reason]);
});

// 事件订阅统一入口:注册失败必须可见(D-005 教训——ACL 拒绝时曾静默失联)。
function on(event, handler) {
  listen(event, (eventPayload) => {
    const sessionId = eventPayload.payload?.sessionId;
    const controlEvent = event === "kz:ask" || event === "kz:done" || event === "kz:error" || event === "kz:stopped";
    if (!controlEvent && sessionId && activeSessionId && sessionId !== activeSessionId) return;
    if (controlEvent && event !== "kz:ask" && sessionId && activeSessionId && sessionId !== activeSessionId) {
      refreshProcesses();
      log(`后台会话控制事件已路由:${event} ${sessionId}`);
      return;
    }
    handler(eventPayload);
  }).catch((err) => {
    log(`事件订阅失败 ${event}: ${err} — 界面将收不到运行事件,请反馈`, "err");
    $("log-panel").classList.remove("hidden");
  });
}

const $ = (id) => document.getElementById(id);
const messages = $("messages");
const promptBox = $("prompt");
const I18N_EN = {
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
  "外部阻塞": "Externally blocked", "阻塞": "Blocked", "可执行": "Ready", "阻塞原因": "Blocking reasons", "缺少阻塞原因": "Blocking reason missing", "解除条件": "Release condition", "下一步": "Next step", "等待项目外部条件、负责人或服务解除": "Waiting for an external condition, owner, or service",
  "复杂度": "Complexity", "未评估": "Not assessed", "设置缺陷复杂度": "Set defect complexity", "设置需求复杂度": "Set requirement complexity", "复杂度已保存": "Complexity saved",
  "配置读取失败": "Failed to read configuration", "配置": "Config", "删除规则": "Delete rule", "已停止并撤销设备 token": "Stopped and revoked device token", "没有可测试的 provider": "No provider to test", "测试中": "Testing", "连通性检查完成": "Connectivity check complete", "可用": "available",
  "订阅登录态": "Subscription login", "环境变量名(可选)": "Environment variable name (optional)", "读取该环境变量作为 key": "Use this environment variable as the key", "或直接粘贴 key": "Or paste a key directly", "直填优先于环境变量;明文存 kanzei.toml": "Direct value takes precedence; stored in kanzei.toml", "已设": "Set", "缺失": "Missing", "测试": "Test", "连接": "connection", "不限": "Unlimited",
  "自动压缩": "automatic compaction", "上下文": "Context", "点击查看上下文成分": "Click to view context details",
  "连接中断": "Connection interrupted", "重放本轮": "Replaying round", "总结中": "Summarizing", "当前没有可总结的对话": "No conversation to summarize", "小总结已收纳到活动面板": "Summary added to activity panel",
  "自主推进": "Self-directed progress", "等待下一轮": "Waiting for next round", "鞭挞恢复": "Auto-run resumed", "秒后继续": "seconds until continuing",
  "已停止": "Stopped", "完成": "Completed", "用户拒绝后停止": "Stopped after user rejection", "本轮完成": "Round completed", "按你的拒绝停止": "Stopped after your rejection",
  "没有可复制的内容": "Nothing to copy", "当前没有可复制的对话": "No conversation to copy",
  "当前任务还在运行，自动鞭挞将在本轮完成后继续": "The current task is still running; auto-run will continue after this round", "先在左侧「项目」里添加并选择一个目录": "Add and select a directory under Projects first",
  "已撤销排队输入": "Queued input cancelled", "暂无测试记录": "No test runs", "撤销": "Cancel", "撤销这条排队输入": "Cancel this queued input",
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
  "记忆标题": "Title", "召回钩子": "Recall hook", "记忆正文": "Body", "来源": "Source",
  "保存修改": "Save changes", "标题": "Title", "编辑标题": "Edit title", "编辑字段": "Edit field", "记忆已保存": "Memory saved", "记忆保存失败": "Failed to save memory", "找不到": "Not found:",
  "选择": "Select", "已选": "Selected", "改状态…": "Change status…", "混选类型,仅可改标签": "Mixed types — tags only",
  "先选择要改的状态或标签": "Pick a status or tag to apply first", "批量操作完成": "Bulk update done", "批量操作部分失败": "Bulk update partly failed",
  "内部调用": "inner calls", "只停这一条,不影响本轮其它工具": "Stop just this one; other tools in this round keep running",
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
  "运行画像加载失败": "Failed to load run metrics", "还没有轮次记录:跑一轮后这里会出现画像": "No rounds recorded yet — run once and metrics will appear here",
  "平均终端调用": "Avg terminal calls", "平均 git 查询组": "Avg git query groups", "edit 未命中率": "Edit miss rate",
  "平均步数": "Avg steps", "平均输出 token": "Avg output tokens", "近": "Last", "轮均值": "round average",
  "步": "steps", "终端": "terminal", "组": "groups", "未命中": "missed", "子代理": "subagents",
  "失败": "failed", "上下文": "context", "该轮早于度量落地,无画像": "This round predates metrics collection — no profile",
  "标记失效": "Mark stale", "恢复启用": "Reactivate", "没有命中的记忆": "No matching memory",
  "记忆检索失败": "Memory search failed", "inbox 尚有草稿未消化": "Inbox still has pending notes",
  "inbox 已整理完毕": "Inbox consolidated", "整理失败": "Consolidation failed",
  "暂无账单数据(跑一轮后生成)": "No bill yet (generated after a run)", "暂无轮次记录": "No rounds recorded",
  "暂无隔离工作树": "No isolated worktrees", "干净": "Clean", "项改动": "changed files", "差异": "Diff", "合并": "Merge", "放弃": "Discard",
  "工作树干净,没有未提交差异": "Worktree is clean; there are no uncommitted changes", "工作树差异已写入运行日志": "Worktree diff was written to the runtime log", "工作树操作完成，详细结果已写入运行日志": "Worktree operation completed; detailed results were written to the runtime log", "隔离工作树已创建": "Isolated worktree created", "放弃工作树": "Discard worktree", "未提交改动会阻止删除并保留现场": "Uncommitted changes will prevent deletion and be preserved",
  "历史消息恢复失败": "Failed to restore conversation history", "已恢复": "Restored", "历史消息": "historical messages", "组工具轨迹": "tool traces", "暂无历史对话": "No conversation history", "点击打开 · 勾选后点标题栏的删除图标批量删除": "Click to open · tick rows, then use the delete icon in the section header", "已打开历史对话": "Opened historical conversation", "先勾选要删除的历史对话": "Select conversations to delete first", "已删除": "Deleted", "份对话快照": " conversation snapshots", "历史对话加载失败": "Failed to load conversation history", "已开启新对话(历史已清空)": "New conversation started (history cleared)", "新对话:多轮历史已清空": "New conversation: multi-turn history cleared",
  "上下文占用过高,已自动压缩为纪要并延续对话": "Context was too large; it was compacted into a summary and the conversation continued", "自动压缩完成:多轮历史已替换为纪要": "Automatic compaction complete: multi-turn history replaced by a summary", "已手动停止": "Stopped manually", "已手动停止并取消": "Stopped manually and cancelled", "已取消": "cancelled", "上轮": "last round", "鞭挞停止": "Auto-run stopped", "处于暂停中,点顶栏「继续鞭挞」恢复": "paused; click \"Resume auto-run\" in the top bar to continue", "已自动取消勾选,再点鞭挞即可继续": "automatically unchecked; click Auto-run to continue", "已达连上限,点继续或重开鞭挞": "maximum consecutive rounds reached; click Continue or restart Auto-run", "上一轮没有实质动作,已追加一次具体推进指令(再无动作才会停)": "The previous round made no substantive progress; one concrete nudge was added (it stops if the next round is also inactive)", "连续两轮没有实质动作(可能目标已达成或确实无可推进项)": "Two consecutive rounds made no substantive progress (the goal may be complete or nothing can be advanced)", "连续两轮无动作,鞭挞停止": "No action for two consecutive rounds; Auto-run stopped", "无动作 · 追加推进指令": "No action · added nudge", "系统通知权限已拒绝，请在系统设置中允许后重试": "System notification permission was denied; allow it in system settings and try again", "系统通知权限未授予，完成提示将保留在应用内": "System notification permission was not granted; completion notices will remain in the app", "当前环境不支持系统通知，完成提示将保留在应用内": "System notifications are not supported here; completion notices will remain in the app", "系统通知权限请求失败": "Failed to request system notification permission", "运行中可插入或排队，按交付方式发送": "While running, send to steer or queue according to Delivery", "运行中请先完成或停止当前任务，再打开历史对话": "Finish or stop the current task before opening conversation history", "文件列表": "Files", "实际差异": "Diff", "未跟踪文件尚未包含在 git diff 中": "Untracked files are not included in git diff", "子代理启动中": "subagent starting", "历史子代理轨迹": "historical subagent trace", "历史轨迹": "historical trace", "回放": "replay", "文件": "file", "并排": "Split", "统一": "Unified", "展开或收起后台任务详情": "Expand or collapse background task details", "测试失败": "Test failed", "移除 provider": "Remove provider", "已删除权限规则": "Permission rule deleted", "删除权限规则": "Delete permission rule", "删除": "Delete", "移动端本机桥接已启动": "Local mobile bridge started", "先填写 agent id": "Enter an agent id first", "代理容器": "Agent container", "创建": "created", "升级": "upgraded", "回滚": "rolled back", "已保存": "Saved", "检查中…": "Checking…", "发现新版本": "New version found", "已是最新": "Already up to date", "检查失败": "Check failed", "下载中…(安装器就绪后会自动弹出)": "Downloading… (the installer will open when ready)", "工具": "Tool", "工具结果": "Tool result", "移动端本机桥接已停止": "Local mobile bridge stopped", "选择项目": "Select project", "移除(不删除文件)": "Remove (do not delete files)", "移除项目": "Remove project", "只解除登记,不会删除磁盘文件。": "Only unregister it; files on disk will not be deleted.", "重命名项目(只修改显示名)": "Rename project (display name only)", "重命名项目": "Rename project", "项目显示名": "Project display name", "新项目目录路径(不存在时会创建)": "New project directory (created if missing)", "项目显示名(可留空)": "Project display name (optional)", "已初始化并切换到新项目": "Initialized and switched to the new project", "项目初始化完成": "Project initialization complete", "创建进程失败": "Failed to create process", "更新进程能力失败": "Failed to update process capability", "进程模式保存失败": "Failed to save process mode", "进程思考强度保存失败": "Failed to save reasoning effort", "进程模型保存失败": "Failed to save process model", "进程列表刷新失败": "Failed to refresh process list", "待处理权限询问恢复失败": "Failed to restore pending permission requests", "已切换到进程": "Switched to process", "回答": "Answer", "权限": "Permission", "拒绝": "Deny", "总是允许": "Always allow", "自动放行失败": "Auto-allow failed", "权限应答失败": "Permission response failed", "已开启自动放行(本会话所有权限询问直接通过)": "Auto-allow enabled (all permission requests in this session pass automatically)", "已关闭自动放行": "Auto-allow disabled", "需求": "requirement", "缺陷": "defect", "自然语言描述": "Describe in natural language", "先写点描述": "Write a description first", "记录中": "Recording", "独立子代理后台进行": "independent subagent working in background", "已记录": "Recorded", "记录失败(内容已保留,可重试)": "Recording failed (content kept; retry available)", "提交": "Submit", "取消": "Cancel", "目标描述,回车创建(Esc 取消)": "Goal description, press Enter to create (Esc to cancel)", "未创建,点 ＋ 生成模板;agent 会自动遵守此文件": "Not created; click + to generate a template; the agent will follow it", "打开开发规范": "Open conventions", "个章节": " sections", "点击查看": "click to view", "规范文件已就绪": "Conventions file ready", "空": "Empty", "按 Enter 展开详情": "Press Enter to expand details", "点击展开": "click to expand", "点击循环调整优先级": "Click to cycle priority", "优先级已调整为": "Priority changed to", "转": "Move to", "记录状态/调整方向,回车保存": "Record status/adjustment, press Enter to save", "需求与缺陷已清空，自动推进已停止": "Requirements and defects are clear; Auto-run stopped", "自动推进停止:需求与缺陷已清空": "Auto-run stopped: requirements and defects are clear", "检查需求/缺陷是否清空失败": "Failed to check whether requirements/defects are clear", "移除附件": "Remove attachment", "不支持的附件类型": "Unsupported attachment type", "鞭挞已暂停": "Auto-run paused", "鞭挞已恢复": "Auto-run resumed", "本轮结束后将停止鞭挞": "Auto-run will stop after this round", "已取消本轮后停": "Stop-after-round cancelled", "鞭挞上限已设为": "Auto-run limit set to", "鞭挞仅适用于自主推进模式，请先切换模式": "Auto-run only works in Self-directed progress mode; switch modes first", "鞭挞未开启:结伴开发模式不支持自动续跑": "Auto-run not enabled: paired development mode does not support continuation", "鞭挞已开启:每轮结束自动推进目标": "Auto-run enabled: advance the goal after each round", "鞭挞已关闭": "Auto-run disabled", "鞭挞启动,2 秒后开始…": "Auto-run starting in 2 seconds…", "当前模式不支持鞭挞，已自动关闭": "The current mode does not support Auto-run; it was disabled", "鞭挞已关闭：当前进程不是自主推进模式": "Auto-run disabled: the current process is not Self-directed progress", "复制": "Copy", "复制消息": "Copy message", "可压缩重试": "Retry after compaction", "可重试错误": "Retryable error", "致命错误": "Fatal error", "重试上一次请求": "Retry last request", "正在重试…": "Retrying…", "思考中…": "Thinking…", "拖动调整面板宽度": "Drag to adjust panel width", "调整面板宽度": "Adjust panel width", "展开或收起思考过程": "Expand or collapse reasoning", "切换差异并排或统一视图": "Toggle split or unified diff view", "已复制": "Copied", "继续文案已升级到新版(含【阻塞】刹车约定)": "Continue prompt upgraded to the new version (with the blocked brake rule)", "已请求停止(本地已复位)": "Stop requested (local state reset)", "模型:agent 默认": "Model: agent default", "暂无排队输入": "No queued input", "任务运行中,先停止再开新对话": "Stop the running task before starting a new conversation", "先选择一个项目": "Select a project first", "开始总结当前对话…": "Starting conversation summary…", "轮": "rounds", "耗时": "duration",
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
  "只对当前进程启用/关闭只读子代理": "Enable or disable read-only subagents for this process",
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
  "改标签…": "Change tag…",
  "应用": "Apply",
  "取消选择": "Clear selection",
  "待确认候选": "Pending candidates",
  "完成一个完整条目后自动提炼的 SOP 候选。候选不会自己入库——采纳才交给记忆管理子代理提炼成条目，丢弃则直接移出。": "SOP candidates extracted after completing an item. Candidates are stored only after you accept them; discard removes them.",
  "召回评估": "Recall evaluation",
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
function applyLanguage() {
  if (applyingLanguage) return;
  applyingLanguage = true;
  try {
    const language = localStorage.getItem("kz-language") || "zh";
    document.documentElement.lang = language === "en" ? "en" : "zh-CN";
    const walker = document.createTreeWalker(document.body, NodeFilter.SHOW_TEXT);
    while (walker.nextNode()) {
      const node = walker.currentNode;
      const parent = node.parentElement || node.parentNode;
      if (parent?.closest?.("[data-i18n-raw]")) continue;
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
      if (!key) continue;
      const exact = I18N_EN[key] || I18N_DYNAMIC_EN[key];
      const next = language === "en"
        ? (exact ? source.replace(key, exact) : localizeDynamic(source))
        : source;
      if (next.length > 1_000_000) {
        throw new Error(`i18n text expansion detected:length=${next.length},key=${key.slice(0, 80)}`);
      }
      if (node.nodeValue !== next) node.nodeValue = next;
    }
    document.querySelectorAll("[title], [placeholder], [aria-label]").forEach((element) => {
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
        const translated = language === "en" ? (I18N_EN[key] || localizeDynamic(source)) : source;
        if (translated !== source || language !== "en") element.setAttribute(attribute, language === "en" ? translated : source);
      }
    });
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
const languageObserver = new MutationObserver(() => applyLanguage());
languageObserver.observe(document.body, {
  childList: true,
  subtree: true,
  characterData: true,
  attributes: true,
  attributeFilter: ["title", "placeholder", "aria-label"],
});
function setupResize(elementId, key, side, min, max) {
  const element = $(elementId);
  if (!element) return;
  const saved = Number.parseInt(localStorage.getItem(key), 10);
  if (Number.isFinite(saved)) element.style.width = `${Math.min(max, Math.max(min, saved))}px`;
  const handle = document.createElement("div");
  handle.className = "resize-handle";
  handle.title = t("拖动调整面板宽度");
  handle.tabIndex = 0;
  handle.setAttribute("role", "separator");
  handle.setAttribute("aria-orientation", "vertical");
  handle.setAttribute("aria-label", t("调整面板宽度"));
  element.appendChild(handle);
  const syncHandle = () => {
    const rect = element.getBoundingClientRect();
    handle.style.top = `${rect.top}px`;
    handle.style.height = `${rect.height}px`;
    handle.style.left = `${(side === "right" ? rect.right : rect.left) - 2}px`;
  };
  const setWidth = (width) => {
    const next = Math.min(max, Math.max(min, Math.round(width)));
    element.style.width = `${next}px`;
    localStorage.setItem(key, String(next));
    syncHandle();
  };
  const resetWidth = () => {
    localStorage.removeItem(key);
    element.style.width = "";
    syncHandle();
  };
  syncHandle();
  if ("ResizeObserver" in window) new ResizeObserver(syncHandle).observe(element);
  window.addEventListener("resize", syncHandle);
  let dragging = false;
  handle.addEventListener("pointerdown", (event) => {
    event.preventDefault();
    dragging = true;
    handle.classList.add("dragging");
    handle.setPointerCapture(event.pointerId);
    document.body.style.cursor = "col-resize";
  });
  handle.addEventListener("pointermove", (event) => {
    if (!dragging) return;
    const rect = element.getBoundingClientRect();
    setWidth(side === "right" ? event.clientX - rect.left : rect.right - event.clientX);
  });
  handle.addEventListener("keydown", (event) => {
    if (!["ArrowLeft", "ArrowRight", "Home"].includes(event.key)) return;
    event.preventDefault();
    if (event.key === "Home") return resetWidth();
    const rect = element.getBoundingClientRect();
    const delta = side === "right" ? (event.key === "ArrowRight" ? 8 : -8) : (event.key === "ArrowLeft" ? 8 : -8);
    setWidth(rect.width + delta);
  });
  handle.addEventListener("dblclick", resetWidth);
  const stop = () => {
    dragging = false;
    handle.classList.remove("dragging");
    document.body.style.cursor = "";
  };
  handle.addEventListener("pointerup", stop);
  handle.addEventListener("pointercancel", stop);
  handle.addEventListener("lostpointercapture", stop);
}
setupResize("sidebar", "kz-sidebar-width", "right", 220, 460);
setupResize("todo-panel", "kz-todo-width", "left", 240, 520);
setupResize("bg-panel", "kz-activity-width", "left", 240, 520);
let activeProcessId = null;
let activeSessionId = null;
let processItems = [];

let running = false;
let currentProject = null;
let currentAssistant = null;
let currentReasoning = null;
let attachments = [];
let lastRequest = null;
let runTokens = { input: 0, output: 0, cacheRead: 0, cacheWrite: 0 };

// ---------- 视图切换 ----------
document.querySelectorAll(".activity-item").forEach((item) => {
  item.addEventListener("click", () => {
    document.querySelectorAll(".activity-item").forEach((i) => {
      i.classList.remove("active");
      i.removeAttribute("aria-current");
    });
    item.classList.add("active");
    item.setAttribute("aria-current", "page");
    const view = item.dataset.view;
    document.body.classList.toggle("documents-active", view === "documents");
    document.querySelectorAll(".view").forEach((v) => v.classList.remove("active"));
    $(`view-${view}`).classList.add("active");
    if (view === "settings") loadSettings();
    if (view === "workspace") refreshWorkspace();
    if (view === "documents") refreshDocs();
    if (view === "memory") refreshMemory();
    if (view === "metrics") refreshMetrics();
  });
});

// ---------- toast ----------
let toastTimer = null;
let errorRetry = null;
function toast(text) {
  const el = $("toast");
  const source = String(text);
  const translated = Object.prototype.hasOwnProperty.call(I18N_EN, source) ? t(source) : source;
  el.textContent = localizeDynamic(translated);
  el.classList.remove("hidden");
  clearTimeout(toastTimer);
  toastTimer = setTimeout(() => el.classList.add("hidden"), 2600);
}
function reportPersistentError(text, { retry = null } = {}) {
  log(text, "err");
  errorRetry = retry;
  $("log-retry").classList.toggle("hidden", typeof retry !== "function");
  $("log-panel").classList.remove("hidden");
}
function toastError(text, options = {}) {
  reportPersistentError(text, options);
}

let completionAudioContext = null;
const baseTitle = document.title;

function playRunNotice(kind) {
  try {
    const AudioCtor = window.AudioContext || window.webkitAudioContext;
    if (!AudioCtor) return;
    completionAudioContext ??= new AudioCtor();
    if (completionAudioContext.state === "suspended") completionAudioContext.resume().catch(() => {});
    const now = completionAudioContext.currentTime;
    const frequencies = kind === "failed" ? [220, 165] : kind === "stopped" ? [330] : [523, 659];
    frequencies.forEach((frequency, index) => {
      const oscillator = completionAudioContext.createOscillator();
      const gain = completionAudioContext.createGain();
      oscillator.frequency.value = frequency;
      oscillator.type = "sine";
      gain.gain.setValueAtTime(0.0001, now + index * 0.11);
      gain.gain.exponentialRampToValueAtTime(0.12, now + index * 0.11 + 0.01);
      gain.gain.exponentialRampToValueAtTime(0.0001, now + index * 0.11 + 0.1);
      oscillator.connect(gain).connect(completionAudioContext.destination);
      oscillator.start(now + index * 0.11);
      oscillator.stop(now + index * 0.11 + 0.11);
    });
  } catch (error) {
    log(`完成提示音不可用:${error}`, "warn");
  }
}

let notificationPermissionPrompted = false;
function explainNotificationFallback(message) {
  log(message, "warn");
  $("log-panel").classList.remove("hidden");
  toast(message);
}
async function ensureNotificationPermission() {
  if (notificationPermissionPrompted) return false;
  notificationPermissionPrompted = true;
  if (!("Notification" in window)) {
    explainNotificationFallback(t("当前环境不支持系统通知，完成提示将保留在应用内"));
    return false;
  }
  if (Notification.permission === "granted") return true;
  if (Notification.permission === "denied") {
    explainNotificationFallback(t("系统通知权限已拒绝，请在系统设置中允许后重试"));
    return false;
  }
  try {
    const permission = await Notification.requestPermission();
    if (permission !== "granted") explainNotificationFallback(t("系统通知权限未授予，完成提示将保留在应用内"));
    return permission === "granted";
  } catch (error) {
    explainNotificationFallback(`${t("系统通知权限请求失败")}:${error}`);
    return false;
  }
}

function notifyRunState(kind, text) {
  const labels = { completed: t("运行完成"), failed: t("运行失败"), stopped: t("运行已停止") };
  const label = labels[kind] || t("运行状态");
  toast(`${label}: ${text}`);
  playRunNotice(kind);
  if (!document.hasFocus() || document.hidden) {
    document.title = `🔔 ${label} · ${baseTitle}`;
    if ("Notification" in window && Notification.permission === "granted") {
      try {
        new Notification(label, { body: text, tag: "kanzei-run-state" });
      } catch (error) {
        log(`系统通知不可用:${error}`, "warn");
      }
    }
  }
}

function resetTitleOnFocus() {
  if (!document.hidden && document.hasFocus()) document.title = baseTitle;
}
document.addEventListener("visibilitychange", resetTitleOnFocus);
window.addEventListener("focus", resetTitleOnFocus);
let activityPanelOpen = localStorage.getItem("kz-activity-panel") === "1";

function syncActivityPanel() {
  $("bg-panel").classList.toggle("hidden", !activityPanelOpen);
  const toggle = $("activity-toggle");
  toggle.classList.toggle("active", activityPanelOpen);
  toggle.textContent = activityPanelOpen ? `${t("活动")} ✓` : t("活动");
  toggle.title = activityPanelOpen ? t("隐藏右侧活动面板") : t("显示右侧活动面板");
}

$("activity-toggle").addEventListener("click", () => {
  activityPanelOpen = !activityPanelOpen;
  localStorage.setItem("kz-activity-panel", activityPanelOpen ? "1" : "0");
  syncActivityPanel();
});
syncActivityPanel();

let sidebarCollapsed = localStorage.getItem("kz-sidebar-collapsed") === "1";
function syncSidebar() {
  const sidebar = $("sidebar");
  const toggle = $("sidebar-toggle");
  sidebar.classList.toggle("collapsed", sidebarCollapsed);
  toggle.classList.toggle("active", sidebarCollapsed);
  toggle.setAttribute("aria-expanded", sidebarCollapsed ? "false" : "true");
  toggle.title = localizeDynamic(sidebarCollapsed ? "展开左侧栏" : "折叠左侧栏");
}
$("sidebar-toggle").addEventListener("click", () => {
  sidebarCollapsed = !sidebarCollapsed;
  localStorage.setItem("kz-sidebar-collapsed", sidebarCollapsed ? "1" : "0");
  syncSidebar();
});
syncSidebar();

// ---------- 运行日志面板 ----------
const LOG_MAX = 300;
function log(text, cls = "") {
  const lines = $("log-lines");
  const line = document.createElement("div");
  line.className = `log-line ${cls}`;
  const time = new Date().toTimeString().slice(0, 8);
  line.textContent = `${time}  ${localizeDynamic(text)}`;
  lines.appendChild(line);
  while (lines.childElementCount > LOG_MAX) lines.firstElementChild.remove();
  lines.scrollTop = lines.scrollHeight;
}
$("log-toggle").addEventListener("click", () => $("log-panel").classList.toggle("hidden"));
$("log-retry").addEventListener("click", async () => {
  if (typeof errorRetry !== "function") return;
  const retry = errorRetry;
  $("log-retry").disabled = true;
  try {
    await retry();
  } finally {
    $("log-retry").disabled = false;
  }
});
$("log-copy").addEventListener("click", async () => {
  const text = $("log-lines").innerText.trim();
  if (!text) {
    toast(t("暂无可复制的运行日志"));
    return;
  }
  try {
    await navigator.clipboard.writeText(text);
    toast(t("运行日志已复制"));
  } catch (error) {
    toastError(`复制运行日志失败:${error}`);
  }
});
$("log-clear").addEventListener("click", () => ($("log-lines").innerHTML = ""));

// ---------- 状态栏 ----------
let statusTextSource = "";
let statusRunning = false;
function setStatus(text, isRunning) {
  statusTextSource = String(text ?? "");
  statusRunning = !!isRunning;
  $("status-text").textContent = localizeDynamic(statusTextSource);
  $("status-mode").textContent = statusRunning ? t("运行中") : t("空闲");
  $("status-dot").className = `dot ${statusRunning ? "run" : "idle"}`;
  $("statusbar").classList.toggle("running", statusRunning);
}

// 运行计时 + 首响应看门狗:等太久时把"卡在哪"讲清楚。
let runStart = 0;
let firstSignal = false;
let elapsedTimer = null;
function startElapsed() {
  runStart = Date.now();
  firstSignal = false;
  clearInterval(elapsedTimer);
  elapsedTimer = setInterval(() => {
    const secs = Math.floor((Date.now() - runStart) / 1000);
    $("status-elapsed").textContent = `· ${secs}s`;
    if (!firstSignal && secs > 0 && secs % 15 === 0) {
      log(`仍在等待模型首个响应(已 ${secs}s)——订阅高峰或网络较慢时属正常;超时上限 15s 连接 / 180s 读`, "warn");
    }
  }, 1000);
}
function stopElapsed() {
  clearInterval(elapsedTimer);
  elapsedTimer = null;
  $("status-elapsed").textContent = "";
}
function markFirstSignal() {
  if (!firstSignal) {
    firstSignal = true;
    log(`模型开始响应(${((Date.now() - runStart) / 1000).toFixed(1)}s)`);
  }
}

let ctxLimit = null;
let ctxTokens = 0;
function renderTokens() {
  const t = runTokens;
  let text = t.input + t.output === 0
    ? ""
    : `in ${t.input} (cache r${t.cacheRead} w${t.cacheWrite}) · out ${t.output}`;
  const bar = $("ctx-bar");
  if (ctxTokens > 0) {
    const k = (ctxTokens / 1000).toFixed(1);
    if (ctxLimit) {
      const pct = Math.round((ctxTokens / ctxLimit) * 100);
      text += `${text ? " · " : ""}ctx ${k}k/${Math.round(ctxLimit / 1000)}k (${pct}%)`;
      $("status-tokens").classList.toggle("ctx-warn", pct >= 70);
      // 进度条:容量占用一眼可见,≥70% 变警示色(自动压缩阈值同源)。
      bar.classList.remove("hidden");
      bar.classList.toggle("warn", pct >= 70);
      $("ctx-bar-fill").style.width = `${Math.min(pct, 100)}%`;
      bar.title = `${t("上下文")} ${k}k / ${Math.round(ctxLimit / 1000)}k(${pct}%,≥70% ${t("自动压缩")})`;
    } else {
      text += `${text ? " · " : ""}ctx ${k}k`;
      bar.classList.add("hidden");
    }
  } else {
    bar.classList.add("hidden");
  }
  $("status-tokens").textContent = text;
}

function setRunning(value, statusText) {
  running = value;
  const send = $("send");
  send.disabled = false;
  send.title = value ? t("运行中可插入或排队，按交付方式发送") : "";
  send.setAttribute("aria-label", value ? t("运行中可插入或排队，按交付方式发送") : "发送");
  $("stop").classList.toggle("hidden", !value);
  setStatus(statusText ?? (value ? t("运行中") : t("空闲")), value);
}

// ---------- markdown-lite(安全子集:代码围栏/语言标识/行内码/加粗/标题/列表/表格/安全外链) ----------
function escapeHtml(s) {
  return String(s)
    .replace(/&/g, "&amp;")
    .replace(/</g, "&lt;")
    .replace(/>/g, "&gt;")
    .replace(/"/g, "&quot;")
    .replace(/'/g, "&#39;");
}
function splitTableRow(line) {
  const value = line.trim().replace(/^\|/, "").replace(/\|$/, "");
  return value.split("|").map((cell) => cell.trim());
}
function tableAlignment(cell) {
  if (/^:-+:$/.test(cell)) return "center";
  if (/^-+:$/.test(cell)) return "right";
  return "left";
}
function safeMarkdownUrl(value) {
  const url = value.trim();
  return /^(?:https?:\/\/|mailto:)/i.test(url) && !/[\s"'<]/.test(url) ? url : null;
}
function renderInlineMarkdown(raw) {
  const placeholders = [];
  const stash = (html) => {
    const token = `\u0000md-${placeholders.length}\u0000`;
    placeholders.push(html);
    return token;
  };
  let html = escapeHtml(raw);
  html = html.replace(/`([^`\n]+)`/g, (_, code) => stash(`<code>${code}</code>`));
  html = html.replace(/\[([^\]]+)\]\(([^)\s]+)\)/g, (_, label, url) => {
    const decodedUrl = url.replace(/&amp;/g, "&").replace(/&quot;/g, '"').replace(/&#39;/g, "'").replace(/&lt;/g, "<").replace(/&gt;/g, ">");
    const safeUrl = safeMarkdownUrl(decodedUrl);
    return safeUrl
      ? stash(`<a href="${escapeHtml(safeUrl)}" target="_blank" rel="noopener noreferrer">${label}</a>`)
      : `${label} (${escapeHtml(decodedUrl)})`;
  });
  html = html.replace(/\*\*([^*\n]+)\*\*/g, "<strong>$1</strong>");
  return html.replace(/\u0000md-(\d+)\u0000/g, (_, index) => placeholders[Number(index)]);
}
function renderMarkdown(raw) {
  const lines = String(raw).replace(/\r\n?/g, "\n").split("\n");
  let html = "";
  let paragraph = [];
  let list = null;
  let code = null;
  const flushParagraph = () => {
    if (!paragraph.length) return;
    html += `<p>${renderInlineMarkdown(paragraph.join("\n"))}</p>`;
    paragraph = [];
  };
  const flushList = () => {
    if (!list) return;
    html += `<${list.type}>${list.items.map((item) => `<li>${renderInlineMarkdown(item)}</li>`).join("")}</${list.type}>`;
    list = null;
  };
  const flushCode = () => {
    if (!code) return;
    const language = code.language ? code.language.replace(/[^a-zA-Z0-9_+-]/g, "") : "";
    const className = language ? ` class="language-${language}"` : "";
    html += `<pre class="code"><code${className}>${escapeHtml(code.lines.join("\n"))}</code></pre>`;
    code = null;
  };
  const renderTable = (header, separator, rows) => {
    const alignments = separator.map(tableAlignment);
    const cell = (tag, value, index) => `<${tag} style="text-align:${alignments[index] || "left"}">${renderInlineMarkdown(value)}</${tag}>`;
    html += `<table><thead><tr>${header.map((value, index) => cell("th", value, index)).join("")}</tr></thead><tbody>`;
    for (const row of rows) html += `<tr>${header.map((_, index) => cell("td", row[index] || "", index)).join("")}</tr>`;
    html += "</tbody></table>";
  };
  for (let index = 0; index < lines.length; index += 1) {
    const line = lines[index];
    const fence = line.match(/^\s*```\s*([^\s`]*)\s*$/);
    if (fence) {
      if (code) flushCode();
      else {
        flushParagraph();
        flushList();
        code = { language: fence[1], lines: [] };
      }
      continue;
    }
    if (code) {
      code.lines.push(line);
      continue;
    }
    if (!line.trim()) {
      flushParagraph();
      flushList();
      continue;
    }
    const heading = line.match(/^\s{0,3}(#{1,6})\s+(.+)$/);
    if (heading) {
      flushParagraph();
      flushList();
      html += `<strong class="md-h">${renderInlineMarkdown(heading[2])}</strong>`;
      continue;
    }
    const listItem = line.match(/^\s*(?:[-*+]\s+|\d+[.]\s+)(.+)$/);
    if (listItem) {
      flushParagraph();
      const type = /^\s*\d+[.]\s+/.test(line) ? "ol" : "ul";
      if (list && list.type !== type) flushList();
      list ??= { type, items: [] };
      list.items.push(listItem[1]);
      continue;
    }
    const nextLine = lines[index + 1];
    if (line.includes("|") && nextLine && isTableSeparator(nextLine)) {
      flushParagraph();
      flushList();
      const header = splitTableRow(line);
      const separator = splitTableRow(nextLine);
      const rows = [];
      index += 2;
      while (index < lines.length && lines[index].includes("|") && lines[index].trim()) {
        rows.push(splitTableRow(lines[index]));
        index += 1;
      }
      index -= 1;
      renderTable(header, separator, rows);
      continue;
    }
    flushList();
    paragraph.push(line);
  }
  if (code) flushCode();
  flushParagraph();
  flushList();
  return html;
}
function isTableSeparator(line) {
  const cells = splitTableRow(line);
  return cells.length >= 2 && cells.every((cell) => /^:?-{3,}:?$/.test(cell));
}


// ---------- 消息渲染 ----------
function clearEmptyState() {
  const empty = $("empty-state");
  if (empty) empty.remove();
}

let followLatest = true;
function nearBottom() {
  return messages.scrollHeight - messages.scrollTop - messages.clientHeight < 48;
}
function updateLatestButton() {
  $("jump-latest").classList.toggle("hidden", followLatest);
}
messages.addEventListener("scroll", () => {
  followLatest = nearBottom();
  updateLatestButton();
});
function scrollBottom(force = false) {
  if (force || followLatest) messages.scrollTop = messages.scrollHeight;
  updateLatestButton();
}
function copyButton() {
  const button = document.createElement("button");
  button.className = "copy-btn";
  button.type = "button";
  button.textContent = t("复制");
  button.title = t("复制消息");
  return button;
}

function addMessage(cls, text) {
  clearEmptyState();
  const el = document.createElement("div");
  el.className = `msg ${cls}`;
  const body = document.createElement("div");
  body.className = "message-body";
  body.textContent = text;
  const actions = document.createElement("span");
  actions.className = "msg-actions";
  actions.appendChild(copyButton());
  el.append(body, actions);
  messages.appendChild(el);
  scrollBottom();
  return el;
}

function addUserMessage(text, promptAttachments = []) {
  const el = addMessage("user", text);
  if (promptAttachments.length === 0) return el;
  const body = el.querySelector(".message-body");
  const attachments = document.createElement("div");
  attachments.className = "message-attachments";
  for (const attachment of promptAttachments) {
    const item = document.createElement("span");
    item.className = "message-attachment";
    const kind = attachment.media_type?.startsWith("image/") ? t("图片") : "PDF";
    item.textContent = `📎 ${attachment.file_name} · ${kind} · ${t("已发送给 agent")}`;
    attachments.appendChild(item);
  }
  body.appendChild(attachments);
  return el;
}

function addErrorMessage(message, { retryable = false } = {}) {
  const el = addMessage("error", "");
  const body = el.querySelector(".message-body");
  const contextOverflow = /context[_ ]length|context overflow|prompt is too long|input is too long|上下文.{0,4}(过长|超限)/i.test(message);
  const level = document.createElement("strong");
  level.className = "error-level";
  level.textContent = contextOverflow ? t("可压缩重试") : retryable ? t("可重试错误") : t("致命错误");
  const text = document.createElement("div");
  text.textContent = message;
  body.append(level, text);
  if (retryable && lastRequest) {
    const actions = el.querySelector(".msg-actions");
    const retry = document.createElement("button");
    retry.className = "retry-btn";
    retry.type = "button";
    retry.textContent = t("重试上一次请求");
    retry.addEventListener("click", () => {
      retry.disabled = true;
      retry.textContent = t("正在重试…");
      sendText(lastRequest.prompt, { promptAttachments: lastRequest.attachments });
    });
    actions.appendChild(retry);
  }
  return el;
}

function isRetryableError(message) {
  return /timed out|timeout|connect|connection|dns|网络|连接|超时|context[_ ]length|context overflow|prompt is too long|input is too long|上下文.{0,4}(过长|超限)/i.test(message);
}

function reportError(message, { retryable = isRetryableError(message) } = {}) {
  addErrorMessage(message, { retryable });
  log(`错误:${message}`, "err");
}

let outputChars = 0;
function appendAssistant(text) {
  if (!currentAssistant) {
    currentAssistant = addMessage("assistant md", "");
    currentAssistant.dataset.raw = "";
  }
  currentAssistant.dataset.raw += text;
  currentAssistant.querySelector(".message-body").innerHTML = renderMarkdown(currentAssistant.dataset.raw);
  outputChars += text.length;
  // 侧边栏"最近在说":assistant 输出的最新一行。
  const lines = currentAssistant.dataset.raw
    .split("\n")
    .map((l) => l.replace(/[#*`]/g, "").trim())
    .filter(Boolean);
  if (lines.length) liveSet("live-note", `💬 ${lines[lines.length - 1].slice(0, 60)}`);
  scrollBottom();
}

// ---------- 主对话内联工具块(R-090):运行细节进对话流,主对话不再贫乏 ----------
// 形态对齐 Claude Code:一行 `工具名(主要参数)` + 一行 `⎿ 结果摘要`,详情默认折叠。
// 实时与历史回放共用同一个构造器,两处观感必须一致。

/// 工具调用的人类摘要:取该工具最有信息量的那个参数,而不是整坨 JSON。
function toolCallSummary(name, input) {
  const source = input && typeof input === "object" ? input : {};
  const pick = (...keys) => {
    for (const key of keys) {
      const value = source[key];
      if (typeof value === "string" && value.trim()) return value.trim();
      if (typeof value === "number") return String(value);
    }
    return "";
  };
  let arg;
  switch (name) {
    case "read": case "write": case "edit": arg = pick("path", "file_path", "file"); break;
    case "bash": case "process": arg = pick("command", "action"); break;
    case "grep": arg = pick("pattern"); break;
    case "glob": arg = pick("pattern", "path"); break;
    case "task": arg = pick("prompt"); break;
    case "memory_search": arg = pick("query"); break;
    case "memory_note": arg = pick("summary"); break;
    case "webfetch": arg = pick("url"); break;
    case "question": arg = pick("question"); break;
    case "req": case "defect": case "goal": case "decision": case "memory": case "source": case "finding":
      arg = [pick("action"), pick("id", "title")].filter(Boolean).join(" ");
      break;
    default:
      arg = pick("path", "command", "query", "pattern", "url", "id", "title", "action", "summary");
  }
  arg = String(arg).replace(/\s+/g, " ").trim();
  return arg.length > 76 ? `${arg.slice(0, 75)}…` : arg;
}

/// 结果摘要:取第一行有信息量的内容(bash 的 "exit code: 0" 独占首行时顺延到下一行)。
function toolResultPreview(content, isError) {
  const lines = String(content ?? "")
    .split("\n")
    .map((line) => line.trim())
    .filter(Boolean);
  const meaningful = lines.find((line) => !/^exit code:\s*0$/i.test(line)) || lines[0] || "";
  if (!meaningful) return isError ? t("失败") : t("完成");
  return meaningful.length > 110 ? `${meaningful.slice(0, 109)}…` : meaningful;
}

/// 构造一个工具块。done=false 时是运行中占位,后续由 fillToolBlock 收尾。
function buildToolBlock(name, input) {
  const wrap = document.createElement("div");
  wrap.className = "msg tool-msg running";
  const head = document.createElement("button");
  head.type = "button";
  head.className = "tool-msg-head";
  head.setAttribute("aria-expanded", "false");
  const icon = document.createElement("span");
  icon.className = "tool-msg-status";
  icon.textContent = "⏺";
  const label = document.createElement("span");
  label.className = "tool-msg-name";
  label.textContent = name;
  const arg = document.createElement("span");
  arg.className = "tool-msg-arg";
  const summary = toolCallSummary(name, input);
  arg.textContent = summary ? `(${summary})` : "";
  head.append(icon, label, arg);
  // 可访问名带上参数;"展开或收起"只进 aria-label,绝不进可见文本。
  head.setAttribute("aria-label", `${name} ${summary} — ${t("展开或收起工具详情")}`);
  const result = document.createElement("div");
  result.className = "tool-msg-result hidden";
  const detail = document.createElement("div");
  detail.className = "tool-msg-detail hidden";
  head.addEventListener("click", () => {
    if (!detail.children.length) return;
    const open = detail.classList.toggle("hidden");
    head.setAttribute("aria-expanded", String(!open));
  });
  wrap.append(head, result, detail);
  return { wrap, head, icon, result, detail };
}

/// 收尾:状态图标 + 结果摘要行 + 折叠详情(完整输入与输出)。
function fillToolBlock(block, { ok, content, display, input }) {
  block.wrap.classList.remove("running");
  block.wrap.classList.add(ok ? "ok" : "err");
  // 形状与颜色双重区分:只靠颜色对色盲不可辨(D-105 无障碍口径)。
  block.icon.textContent = ok ? "⏺" : "✗";
  const preview = toolResultPreview(content, !ok);
  block.result.textContent = `⎿ ${preview}`;
  block.result.classList.remove("hidden");
  appendDisplayBlock(block.detail, display);
  const full = String(content ?? "");
  // 详情只在结果确实比摘要长时才给,避免"展开了还是那一行"。
  if (full.trim() && full.trim() !== preview) {
    const pre = document.createElement("pre");
    pre.className = "tool-msg-raw";
    pre.textContent = full.length > 8000 ? `${full.slice(0, 8000)}\n…(${t("已截断")})` : full;
    block.detail.appendChild(pre);
  }
  if (input && Object.keys(input).length) {
    const pre = document.createElement("pre");
    pre.className = "tool-msg-raw args";
    pre.textContent = JSON.stringify(input, null, 2);
    block.detail.appendChild(pre);
  }
  if (block.detail.children.length) block.wrap.classList.add("has-detail");
}

const chatToolBlocks = new Map();
const CHAT_TOOL_KEEP = 200; // D-090 同款上界:长跑只保留最近块的活引用,DOM 留在历史里。
function chatToolStart(id, name, summary, input) {
  if (!id || chatToolBlocks.has(id)) return;
  clearEmptyState();
  // 实时路径拿不到结构化 input(事件里只有 summary 文本),退化为把 summary 当参数展示。
  const block = buildToolBlock(name, input ?? { command: summary });
  messages.appendChild(block.wrap);
  chatToolBlocks.set(id, block);
  if (chatToolBlocks.size > CHAT_TOOL_KEEP) {
    chatToolBlocks.delete(chatToolBlocks.keys().next().value);
  }
  scrollBottom();
}
function chatToolEnd(id, ok, preview, display) {
  const block = chatToolBlocks.get(id);
  if (!block) return;
  fillToolBlock(block, { ok, content: preview, display });
}

let currentReasoningHead = null;
function appendReasoning(text) {
  if (!currentReasoning) {
    // 思考块:每个思考段独立一块,头部实时显示摘要首行,默认折叠(R-015 修正)。
    clearEmptyState();
    const wrap = document.createElement("div");
    wrap.className = "msg reasoning";
    const head = document.createElement("button");
    head.type = "button";
    head.className = "reasoning-head";
    head.setAttribute("aria-label", t("展开或收起思考过程"));
    head.setAttribute("aria-expanded", "false");
    head.textContent = `· ${t("思考中…")}`;
    const body = document.createElement("div");
    body.className = "reasoning-body md hidden";
    body.dataset.raw = "";
    head.addEventListener("click", () => {
      // 单行摘要没有可展开的正文,点了别装作有反应。
      if (head.classList.contains("expandable")) {
        body.classList.toggle("hidden");
        head.setAttribute("aria-expanded", String(!body.classList.contains("hidden")));
      }
    });
    wrap.append(head, body);
    messages.appendChild(wrap);
    currentReasoning = body;
    currentReasoningHead = head;
  }
  currentReasoning.dataset.raw += text;
  currentReasoning.innerHTML = renderMarkdown(currentReasoning.dataset.raw);
  if (currentReasoningHead) {
    // 预览取最新的非空行:思考推进时头部跟着走,不再冻结在第一行。
    const lines = currentReasoning.dataset.raw
      .split("\n")
      .map((l) => l.replace(/[#*`]/g, "").trim())
      .filter(Boolean);
    const preview = (lines[lines.length - 1] || "").slice(0, 60);
    // codex 常常只给一行摘要标题:没有更多内容就不给"展开"的假承诺。
    const expandable = lines.length > 1;
    currentReasoningHead.textContent = `· ${preview || t("思考中…")}${expandable ? `(${t("点击展开")})` : ""}`;
    currentReasoningHead.classList.toggle("expandable", expandable);
    if (!expandable) currentReasoningHead.setAttribute("aria-expanded", "false");
  }
  scrollBottom();
}

// ---------- 活动面板(R-037):全部工具调用按序入列,详情点击展开,跑完保留可回看 ----------
const bgEntries = new Map(); // call_id -> {el, title, prog, meta, detail, startedAt, done}
const diffSummary = new Map();
const BG_MAX = 120;
function renderDiffSummary() {
  const panel = $("diff-summary");
  const label = $("diff-summary-toggle");
  const files = [...diffSummary.values()];
  const additions = files.reduce((sum, item) => sum + item.additions, 0);
  const deletions = files.reduce((sum, item) => sum + item.deletions, 0);
  label.textContent = files.length ? `· ${files.length} ${t("文件")} +${additions}/−${deletions}` : "";
  panel.classList.toggle("hidden", files.length === 0);
  panel.innerHTML = files.map((item) => `<div class="diff-summary-row"><span>${escapeHtml(item.path)}</span><span>+${item.additions}/−${item.deletions}</span></div>`).join("");
}

function bgSync() {
  // 面板开关只由用户控制;工具事件只能更新内容,不能擅自开关。
  syncActivityPanel();
  applyBgFilters();
}
// R-095:活动面板此前只收 task 与 memory_note,绝大多数轮次里它是空的——"打开也没啥用"。
// 现在它是完整的活动流水,靠筛选控制噪音;主对话仍保留内联工具块,两者定位不同:
// 主对话是叙事,活动面板是可检索的执行记录。
function isActivityTool() {
  return true;
}

const BG_TOOL_TYPES = {
  bash: "terminal", process: "terminal",
  read: "file", write: "file", edit: "file", multiedit: "file", glob: "file", grep: "file",
  req: "tracker", defect: "tracker", goal: "tracker", source: "tracker", finding: "tracker", decision: "tracker",
  task: "agent",
  memory_note: "memory", memory_search: "memory", memory_stats: "memory",
};
function bgToolType(name) {
  return BG_TOOL_TYPES[name] ?? "other";
}
// 终端类输出才提供复制/导出:diff 与追踪结果在主对话里已有更好的呈现。
function bgIsTerminal(name) {
  return bgToolType(name) === "terminal";
}

const bgFilters = {
  type: localStorage.getItem("kz-bg-type") || "all",
  status: localStorage.getItem("kz-bg-status") || "all",
};
function bgEntryStatus(entry) {
  if (!entry.done) return "running";
  return entry.el.classList.contains("err") ? "err" : "ok";
}
function applyBgFilters() {
  let shown = 0;
  for (const entry of bgEntries.values()) {
    const typeOk = bgFilters.type === "all" || entry.type === bgFilters.type;
    const statusOk = bgFilters.status === "all" || bgEntryStatus(entry) === bgFilters.status;
    const visible = typeOk && statusOk;
    entry.el.classList.toggle("hidden", !visible);
    if (visible) shown += 1;
  }
  const count = $("bg-count");
  // 有筛选时同时给出"筛出/总数",否则看到 3 条会以为本轮只跑了 3 个工具。
  if (count) {
    count.textContent = bgEntries.size
      ? shown === bgEntries.size
        ? `· ${bgEntries.size}`
        : `· ${shown}/${bgEntries.size}`
      : "";
  }
}

/// 差异汇总必须独立于活动面板的过滤:diff 来自 write/edit,而这两个工具已不进活动面板,
/// 原先把累计写在 bgEnd 里就等于永远拿不到数据,#diff-summary 变成接不到数据源的空壳(D-137)。
function recordDiffSummary(display) {
  if (display?.kind !== "diff") return;
  diffSummary.set(display.path || `#${diffSummary.size + 1}`, {
    path: display.path || "未命名文件",
    additions: display.additions || 0,
    deletions: display.deletions || 0,
  });
  renderDiffSummary();
}

function bgAdd(id, name, summary, input) {
  if (!id || bgEntries.has(id)) return;
  const type = bgToolType(name);
  const el = document.createElement("div");
  el.className = `bg-entry running bg-type-${type}`;
  el.dataset.bgId = id;
  el.dataset.bgTool = name;
  const title = document.createElement("button");
  title.type = "button";
  title.className = "bg-title";
  title.setAttribute("aria-label", t("展开或收起后台任务详情"));
  title.setAttribute("aria-expanded", "false");
  // 工具名与目标分开呈现:此前拼成一行长文本被 ellipsis 截断,看不出改的是哪个文件、
  // 跑的是哪条命令——"打开也没啥用"的直接原因(R-095 验收 ⑤)。
  const toolName = document.createElement("span");
  toolName.className = "bg-tool";
  toolName.textContent = name;
  const target = document.createElement("span");
  target.className = "bg-target";
  target.textContent = summary;
  title.append(toolName, target);
  title.title = summary;
  const prog = document.createElement("div");
  prog.className = "bg-prog";
  prog.textContent = name === "task" ? `… ${t("子代理启动中")}` : "…";
  const meta = document.createElement("div");
  meta.className = "bg-meta";
  const actions = document.createElement("div");
  actions.className = "bg-actions";
  const detail = document.createElement("div");
  detail.className = "bg-detail hidden";
  // 完整入参永远可展开:summary 是一行摘要,复核"到底拿什么参数调的"要看原文。
  if (input && Object.keys(input).length) {
    const args = document.createElement("pre");
    args.className = "tool-display term bg-args";
    args.textContent = JSON.stringify(input, null, 2);
    detail.appendChild(args);
    el.classList.add("has-detail");
  }
  title.addEventListener("click", () => {
    if (detail.children.length) {
      detail.classList.toggle("hidden");
      title.setAttribute("aria-expanded", String(!detail.classList.contains("hidden")));
    }
  });
  el.append(title, prog, meta, actions, detail);
  const list = $("bg-list");
  list.appendChild(el);
  while (list.children.length > BG_MAX) {
    const first = list.firstElementChild;
    if (!first) break;
    bgEntries.delete(first.dataset.bgId);
    first.remove();
  }
  const entry = {
    el, title, prog, meta, detail, actions, type, name, summary, input,
    children: new Map(), startedAt: Date.now(), done: false,
  };
  bgEntries.set(id, entry);
  bgRenderActions(id, entry);
  bgSync();
  list.scrollTop = list.scrollHeight;
}

// 每条的可操作项。运行中的后台进程/子代理能单独停;结束后能重跑;
// 终端类输出能复制与导出——这三样是"面板能干活"与"面板只是日志"的分界。
function bgRenderActions(id, entry) {
  entry.actions.innerHTML = "";
  const add = (label, title, handler) => {
    const btn = document.createElement("button");
    btn.type = "button";
    btn.className = "ghost mini";
    btn.textContent = label;
    btn.title = title;
    btn.addEventListener("click", (event) => {
      event.stopPropagation();
      handler();
    });
    entry.actions.appendChild(btn);
    return btn;
  };
  if (!entry.done && (entry.type === "agent" || entry.type === "terminal")) {
    add(t("停止"), t("只停这一条,不影响本轮其它工具"), async () => {
      try {
        // 后台进程有独立句柄可单独停;子代理没有单条停止通道,只能停整轮。
        if (entry.name === "bash" || entry.name === "process") {
          const pid = entry.input?.process_id ?? entry.input?.processId;
          if (pid) {
            await invoke("run_tool_process_stop", { projectDir: currentProject, processId: String(pid) });
            toast(t("已停止该后台进程"));
            return;
          }
        }
        await invoke("stop_run", { sessionId: activeSessionId });
        toast(t("已请求停止"));
      } catch (error) {
        toastError(`${t("停止失败")}:${error}`);
      }
    });
  }
  if (entry.done) {
    add(t("重跑"), t("把这次调用的参数填回输入框,确认后再执行"), () => {
      // 不直接重放:工具调用有副作用,必须经用户确认。填回输入框是最轻的确认方式。
      const text = `重跑这次调用:${entry.name} ${entry.summary}\n参数:\n${JSON.stringify(entry.input ?? {}, null, 2)}`;
      $("prompt").value = text;
      $("prompt").focus();
      toast(t("已填入输入框,确认后发送"));
    });
  }
  if (bgIsTerminal(entry.name)) {
    add(t("复制"), t("复制完整输出"), async () => {
      await navigator.clipboard.writeText(bgPlainText(entry));
      toast(t("已复制"));
    });
    add(t("导出"), t("把完整输出存成文件"), () => {
      const blob = new Blob([bgPlainText(entry)], { type: "text/plain;charset=utf-8" });
      const url = URL.createObjectURL(blob);
      const a = document.createElement("a");
      a.href = url;
      a.download = `${entry.name}-${id}.txt`.replace(/[^\w.-]/g, "_");
      a.click();
      URL.revokeObjectURL(url);
      toast(t("已导出"));
    });
  }
}

function bgPlainText(entry) {
  return [
    `# ${entry.name} ${entry.summary}`,
    entry.input ? `\n## 入参\n${JSON.stringify(entry.input, null, 2)}` : "",
    `\n## 输出\n${entry.detail.textContent || entry.prog.textContent || ""}`,
  ].join("\n");
}
function highlightLine(container, text, language) {
  const pattern = /("(?:\\.|[^"])*"|'(?:\\.|[^'])*'|\/\/.*|#.*|\b\d+(?:\.\d+)?\b|\b(?:fn|let|const|function|class|return|if|else|for|while|pub|struct|use|import|from|true|false|null|None|async|await)\b)/g;
  let cursor = 0;
  for (const match of text.matchAll(pattern)) {
    if (match.index > cursor) container.appendChild(document.createTextNode(text.slice(cursor, match.index)));
    const token = document.createElement("span");
    token.className = match[0].startsWith("//") || match[0].startsWith("#") ? "syntax-comment" : /^['"]/.test(match[0]) ? "syntax-string" : /^\d/.test(match[0]) ? "syntax-number" : "syntax-keyword";
    token.textContent = match[0];
    container.appendChild(token);
    cursor = match.index + match[0].length;
  }
  if (cursor < text.length) container.appendChild(document.createTextNode(text.slice(cursor)));
}

function renderDiff(display) {
  const block = document.createElement("div");
  block.className = "tool-display diff";
  let mode = "unified";
  const header = document.createElement("div");
  header.className = "diff-file-header";
  const label = document.createElement("span");
  label.textContent = `${display.path || t("文件")}  +${display.additions || 0} −${display.deletions || 0} · ${display.language || "text"}`;
  const toggle = document.createElement("button");
  toggle.type = "button";
  toggle.className = "ghost mini";
  toggle.setAttribute("aria-label", t("切换差异并排或统一视图"));
  toggle.setAttribute("aria-pressed", "false");
  toggle.textContent = t("并排");
  header.append(label, toggle);
  const body = document.createElement("div");
  body.className = "diff-body";
  const lines = display.lines?.length ? display.lines : (display.diff || "").split("\n").filter(Boolean).map((text) => ({ kind: text[0] === "+" ? "add" : text[0] === "-" ? "del" : "ctx", text: text.slice(1) }));
  function render() {
    body.innerHTML = "";
    body.className = `diff-body ${mode}`;
    if (mode === "unified") {
      let oldLine = 1;
      let newLine = 1;
      for (const line of lines) {
        const row = document.createElement("div");
        row.className = `diff-row ${line.kind || "ctx"}`;
        const oldNo = document.createElement("span");
        const newNo = document.createElement("span");
        oldNo.className = newNo.className = "diff-line-number";
        oldNo.textContent = line.old_line ?? (line.kind === "add" ? "" : oldLine++);
        newNo.textContent = line.new_line ?? (line.kind === "del" ? "" : newLine++);
        const text = document.createElement("code");
        highlightLine(text, line.text || "", display.language || "text");
        row.append(oldNo, newNo, text);
        body.appendChild(row);
      }
    } else {
      const rows = [];
      for (let i = 0; i < lines.length; i += 1) {
        const line = lines[i];
        if (line.kind === "del" && lines[i + 1]?.kind === "add") rows.push([line, lines[++i]]);
        else if (line.kind === "del") rows.push([line, null]);
        else if (line.kind === "add") rows.push([null, line]);
        else rows.push([line, line]);
      }
      for (const [left, right] of rows) {
        const row = document.createElement("div");
        row.className = "diff-split-row";
        for (const line of [left, right]) {
          const pane = document.createElement("div");
          pane.className = `diff-pane ${line?.kind || "empty"}`;
          if (line) {
            const no = document.createElement("span");
            no.className = "diff-line-number";
            no.textContent = line.old_line ?? line.new_line ?? "";
            const text = document.createElement("code");
            highlightLine(text, line.text || "", display.language || "text");
            pane.append(no, text);
          }
          row.appendChild(pane);
        }
        body.appendChild(row);
      }
    }
  }
  toggle.addEventListener("click", () => {
    mode = mode === "unified" ? "split" : "unified";
    toggle.textContent = mode === "unified" ? t("并排") : t("统一");
    toggle.setAttribute("aria-pressed", String(mode === "split"));
    render();
  });
  block.append(header, body);
  render();
  return block;
}
function appendDisplayBlock(parent, display) {
  if (!display) return;
  if (display.kind === "diff") {
    parent.appendChild(renderDiff(display));
  } else if (display.kind === "terminal") {
    const block = document.createElement("div");
    block.className = "tool-display term";
    block.textContent = `$ ${display.command}\n${display.output}`;
    parent.appendChild(block);
  } else if (display.kind === "create") {
    const block = document.createElement("div");
    block.className = "tool-display term";
    block.textContent = `${t("新建")} ${display.path}(${display.bytes} bytes)\n${display.preview}`;
    parent.appendChild(block);
  }
}
function bgProgress(id, text, trace) {
  const entry = bgEntries.get(id);
  if (!entry) return;
  if (text) entry.prog.textContent = text;
  if (!trace) return;
  entry.detail.classList.add("trace-detail");
  let child = entry.children.get(trace.child_id);
  if (trace.phase === "start") {
    if (!child) {
      const row = document.createElement("div");
      row.className = "bg-child running";
      const head = document.createElement("div");
      head.className = "bg-child-head";
      head.textContent = `${trace.name} ${trace.summary || ""}`;
      const meta = document.createElement("div");
      meta.className = "bg-child-meta";
      row.append(head, meta);
      entry.detail.appendChild(row);
      child = { row, head, meta };
      entry.children.set(trace.child_id, child);
      entry.el.classList.add("has-detail");
    }
  } else if (child) {
    child.row.classList.remove("running");
    child.row.classList.add(trace.ok ? "ok" : "err");
    child.meta.textContent = trace.preview || (trace.ok ? t("完成") : t("失败"));
    appendDisplayBlock(child.row, trace.display);
  }
}

function bgEnd(id, ok, preview, display) {
  const entry = bgEntries.get(id);
  if (!entry) return;
  entry.done = true;
  entry.el.classList.remove("running");
  entry.el.classList.add(ok ? "ok" : "err");
  entry.prog.textContent = preview || (ok ? t("完成") : t("失败"));
  // 元信息一行说清:成败、耗时、子代理内部调用数。此前只有一个秒数,
  // 看不出成没成,也看不出子代理到底干了多少活(R-095 验收 ⑤)。
  const ms = Date.now() - entry.startedAt;
  const elapsed = ms >= 1000 ? `${(ms / 1000).toFixed(1)}s` : `${ms}ms`;
  const bits = [ok ? `✓ ${t("成功")}` : `✕ ${t("失败")}`, elapsed];
  if (entry.type === "agent") bits.push(`${t("内部调用")} ${entry.children.size}`);
  entry.meta.textContent = bits.join(" · ");
  // 结构化详情进面板内展开区(diff/终端/新建/todo)。
  const d = display;
  if (d?.kind === "diff") {
    entry.title.textContent += `  +${d.additions} −${d.deletions}`;
  }
  appendDisplayBlock(entry.detail, d);
  if (!ok && preview) {
    const err = document.createElement("div");
    err.className = "tool-display term";
    err.textContent = preview;
    entry.detail.appendChild(err);
  }
  if (entry.detail.children.length) entry.el.classList.add("has-detail");
  bgRenderActions(id, entry);
  bgSync();
}
function renderRecoveredTraces(payloads) {
  const ids = new Set();
  for (const payload of payloads || []) {
    for (const event of payload.events || []) {
      ids.add(event.id);
      if (!bgEntries.has(event.id)) bgAdd(event.id, "task", t("历史子代理轨迹"));
      bgProgress(event.id, event.text, event.trace);
    }
  }
  for (const id of ids) {
    const entry = bgEntries.get(id);
    if (!entry) continue;
    entry.done = true;
    entry.el.classList.remove("running");
    entry.el.classList.add("ok");
    entry.prog.textContent = t("历史轨迹");
    entry.meta.textContent = t("回放");
  }
}

function bgClear() {
  for (const entry of bgEntries.values()) entry.el.remove();
  bgEntries.clear();
  diffSummary.clear();
  $("bg-list").innerHTML = "";
  renderDiffSummary();
  bgSync();
}
// 中止/出错时把仍在跑的条目标记为中止,不再空转。
function bgAbortRunning(label) {
  for (const entry of bgEntries.values()) {
    if (!entry.done) {
      entry.done = true;
      entry.el.classList.remove("running");
      entry.el.classList.add("err");
      entry.prog.textContent = label;
    }
  }
}
setInterval(() => {
  for (const entry of bgEntries.values()) {
    if (!entry.done) entry.meta.textContent = `${Math.round((Date.now() - entry.startedAt) / 1000)}s`;
  }
}, 1000);

// ---------- 当前进展:侧边栏实时状态卡(把握 agent 进度,不用等它汇报) ----------
const liveTextSources = new Map();
function syncDynamicUiLanguage() {
  if (statusTextSource) setStatus(statusTextSource, statusRunning);
  for (const [id, source] of liveTextSources) {
    const el = $(id);
    if (!el) continue;
    el.textContent = localizeDynamic(source);
    el.title = localizeDynamic(source);
  }
  if (!$("context-detail")?.classList.contains("hidden")) renderContextDetail();
  renderAutoStatus(autoStopReason);
}
function liveSet(id, text) {
  const el = $(id);
  const source = String(text ?? "");
  if (!source) {
    liveTextSources.delete(id);
    el.classList.add("hidden");
    return;
  }
  liveTextSources.set(id, source);
  el.classList.remove("hidden");
  el.textContent = localizeDynamic(source);
  el.title = localizeDynamic(source);
}
function liveIdle(label) {
  const turn = $("live-turn");
  const source = String(label ?? "");
  liveTextSources.set("live-turn", source);
  turn.textContent = localizeDynamic(source);
  turn.classList.add("dim");
  liveSet("live-action", "");
}
function liveTurn(text) {
  const turn = $("live-turn");
  const source = String(text ?? "");
  liveTextSources.set("live-turn", source);
  turn.textContent = localizeDynamic(source);
  turn.classList.remove("dim");
}

// ---------- 事件订阅 ----------
on("kz:status", (e) => {
  const p = e.payload;
  log(`[${p.stage}] ${p.detail}`);
  if (running) setStatus(`${p.stage} · ${p.detail}`, true);
});
on("kz:meta", (e) => {
  $("status-model").textContent = `${e.payload.model} · ${e.payload.profile}`;
  ctxLimit = e.payload.contextLimit ?? null;
  log(`模型 ${e.payload.model} · agent ${e.payload.agent} · profile ${e.payload.profile}${ctxLimit ? ` · 上下文上限 ${Math.round(ctxLimit / 1000)}k` : ""}`);
  if (running) setStatus("等待模型响应", true);
});
on("kz:turn", (e) => {
  const p = e.payload;
  if (p.step > 1) {
    clearEmptyState();
    // 轮次分隔不再进主对话区(用户定调:对话为主);轮次在侧边栏"当前进展"实时可见。
  }
  // 活动面板跨轮保留历史,由用户主动清空/切换项目时清理。
  currentAssistant = null;
  currentReasoning = null;
  currentReasoningHead = null;
  const roundLabel = languageIsEnglish() ? `Round ${p.step}${p.maxSteps > 0 ? `/${p.maxSteps}` : ""}` : p.maxSteps > 0 ? `第 ${p.step}/${p.maxSteps} 轮` : `第 ${p.step} 轮`;
  liveTurn(roundLabel);
  if (running) setStatus(`${roundLabel} · ${t("等待模型")}`, true);
});
on("kz:text", (e) => {
  markFirstSignal();
  // 文本开始后,后续思考属于新的思考段。
  currentReasoning = null;
  currentReasoningHead = null;
  if (running) setStatus("生成中" + ` · ${(outputChars / 1000).toFixed(1)}k`, true);
  appendAssistant(e.payload.text);
});
on("kz:reasoning", (e) => {
  markFirstSignal();
  if (running) setStatus("思考中", true);
  appendReasoning(e.payload.text);
});
let todoItems = [];
function renderTodoPanel(items, done, total) {
  todoItems = items || [];
  const panel = $("todo-panel");
  const list = $("todo-list");
  list.innerHTML = "";
  panel.classList.toggle("hidden", todoItems.length === 0);
  $("todo-count").textContent = total ? `${done}/${total}` : "";
  for (const item of todoItems) {
    const row = document.createElement("div");
    row.className = `todo-entry ${item.status}`;
    const status = document.createElement("span");
    status.className = "todo-status";
    status.textContent = item.status === "done" ? "✓" : item.status === "doing" ? "●" : item.status === "dropped" ? "×" : "○";
    const content = document.createElement("span");
    content.className = "todo-content";
    content.textContent = item.content;
    row.append(status, content);
    list.appendChild(row);
  }
}

// R-037 对话为主:工具活动一律不进主对话区,收束到右侧活动面板。
let lastCompactionSummary = "";
let lastCompactionEntry = null;

function addCompactionEntry(summary) {
  const el = document.createElement("div");
  el.className = "bg-entry ok compaction-entry";
  const title = document.createElement("button");
  title.type = "button";
  title.className = "bg-title";
  title.setAttribute("aria-label", t("展开或收起上下文压缩纪要"));
  title.setAttribute("aria-expanded", "true");
  title.textContent = t("上下文压缩 · 点击查看纪要");
  const detail = document.createElement("div");
  detail.className = "bg-detail";
  detail.textContent = summary;
  el.append(title, detail);
  title.addEventListener("click", () => {
    detail.classList.toggle("hidden");
    title.setAttribute("aria-expanded", String(!detail.classList.contains("hidden")));
  });
  $("bg-list").appendChild(el);
  while ($("bg-list").childElementCount > BG_MAX) $("bg-list").firstElementChild.remove();
  lastCompactionEntry = el;
}

function addSummaryEntry(summary, path = "") {
  const el = document.createElement("div");
  el.className = "bg-entry ok summary-entry";
  const title = document.createElement("button");
  title.type = "button";
  title.className = "bg-title";
  title.setAttribute("aria-label", t("展开或收起对话总结"));
  title.setAttribute("aria-expanded", "true");
  title.textContent = t("对话小总结 · 点击查看");
  const detail = document.createElement("div");
  detail.className = "bg-detail";
  detail.textContent = path ? `${summary}\n\n${t("已存档")}: ${path}` : summary;
  el.append(title, detail);
  title.addEventListener("click", () => {
    detail.classList.toggle("hidden");
    title.setAttribute("aria-expanded", String(!detail.classList.contains("hidden")));
  });
  $("bg-list").appendChild(el);
  while ($("bg-list").childElementCount > BG_MAX) $("bg-list").firstElementChild.remove();
  return el;
}
function renderContextDetail() {
  const detail = $("context-detail");
  const t = runTokens;
  const total = t.input + t.cacheRead + t.output;
  detail.innerHTML = `<strong>${localizeDynamic("上下文成分")}</strong><br>${localizeDynamic("输入上下文(系统/历史/工具结果)")}: ${t.input.toLocaleString()} tokens<br>${localizeDynamic("缓存读取(已复用上下文)")}: ${t.cacheRead.toLocaleString()} tokens<br>${localizeDynamic("本轮输出")}: ${t.output.toLocaleString()} tokens${lastCompactionSummary ? `<br>${localizeDynamic("最近一次压缩纪要已收进活动面板")}` : ""}<br>${localizeDynamic("合计")}: ${total.toLocaleString()} tokens`;
  detail.classList.remove("hidden");
  $("status-tokens").setAttribute("aria-expanded", "true");
  if (lastCompactionEntry) {
    activityPanelOpen = true;
    syncActivityPanel();
    lastCompactionEntry.scrollIntoView({ behavior: "smooth", block: "nearest" });
  }
}

function hideContextDetail() {
  $("context-detail").classList.add("hidden");
  $("status-tokens").setAttribute("aria-expanded", "false");
}
function toggleContextDetail() {
  if ($("context-detail").classList.contains("hidden")) renderContextDetail();
  else hideContextDetail();
}

$("status-tokens").title = t("点击查看上下文成分");
$("status-tokens").classList.add("context-clickable");
$("status-tokens").addEventListener("click", toggleContextDetail);
document.addEventListener("keydown", (event) => {
  if (event.key === "Escape") hideContextDetail();
});
document.addEventListener("click", (event) => {
  if (event.target.closest("#status-tokens, #context-detail")) return;
  hideContextDetail();
});
on("kz:tool-start", (e) => {
  markFirstSignal();
  log(`${t("工具")} ${e.payload.name} ${e.payload.summary}`);
  currentAssistant = null;
  currentReasoning = null;
  chatToolStart(e.payload.id, e.payload.name, e.payload.summary, e.payload.input);
  if (isActivityTool(e.payload.name)) bgAdd(e.payload.id, e.payload.name, e.payload.summary, e.payload.input);
  liveSet("live-action", `⚙ ${e.payload.name} ${e.payload.summary.slice(0, 60)}`);
  setStatus(`${t("工具执行中")} · ${e.payload.name}`, true);
});
on("kz:task-progress", (e) => bgProgress(e.payload.id, e.payload.text, e.payload.trace));
on("kz:tool-end", (e) => {
  const p = e.payload;
  log(`${t("工具结果")} ${p.name}: ${p.ok ? t("成功") : t("失败")} — ${p.preview}`, p.ok ? "" : "warn");
  // 工作焦点:req/defect/goal 的增改结果最能代表"它在干哪件事"。
  if (p.ok && ["req", "defect", "goal"].includes(p.name)) {
    liveSet("live-focus", `◉ ${p.preview.replace(/^(updated|added):?\s*/, "").slice(0, 60)}`);
    // 文档已经变了,侧栏列表与状态按钮跟着刷新,不等本轮结束。
    refreshDocsSoon();
  }
  // 测试记录同理:跑完测试后左侧应立即出现结果。
  if (p.ok && ["source", "finding"].includes(p.name)) refreshDocsSoon();
  // 改了文件或跑了命令,工作区状态徽章跟着变(提交后 +N 应当立刻归零)。
  if (p.ok && ["write", "edit", "multiedit", "bash"].includes(p.name)) refreshGitSoon();
  if (p.display?.kind === "todo") {
    renderTodoPanel(p.display.items || [], p.display.done || 0, p.display.total || 0);
  }
  chatToolEnd(p.id, p.ok, p.preview, p.display);
  recordDiffSummary(p.display);
  bgEnd(p.id, p.ok, p.preview, p.display);
  setStatus("运行中", true);
});
on("kz:step", (e) => {
  const p = e.payload;
  runTokens.input += p.input;
  runTokens.output += p.output;
  runTokens.cacheRead += p.cacheRead;
  runTokens.cacheWrite += p.cacheWrite;
  // 本轮 prompt 体积 ≈ 当前上下文占用。
  ctxTokens = p.input + p.cacheRead;
  renderTokens();
  log(`${t("一轮完成")}:in ${p.input} (cache r${p.cacheRead}) · out ${p.output} · ctx ${(ctxTokens / 1000).toFixed(1)}k`);
});
on("kz:error", (e) => {
  cancelAutoContinueTimer();
  const message = e.payload.message;
  reportError(message);
  stopElapsed();
  setRunning(false, "出错");
  bgAbortRunning(`(${localizeDynamic("出错中止")})`);
  liveIdle("出错");
  notifyRunState("failed", message);
  $("log-panel").classList.remove("hidden");
  refreshProcesses();
});
// 流中途断开后重放本轮:后端会把本轮从头重新生成,已渲染的残缺输出必须丢掉,
// 否则重放出的文本会接在半截内容后面变成重复段落。本轮工具尚未执行,无副作用。
on("kz:stream-restart", (e) => {
  const p = e.payload ?? {};
  if (currentAssistant) {
    currentAssistant.remove();
    currentAssistant = null;
  }
  currentReasoning = null;
  currentReasoningHead = null;
  outputChars = 0;
  addMessage("notice", `⟳ ${localizeDynamic("连接中断,正在重新请求本轮")}(${p.attempt}/${p.max})`);
  log(`${localizeDynamic("连接中断,重放本轮")} ${p.attempt}/${p.max},${localizeDynamic("等待")} ${p.delayMs}ms`, "warn");
  setStatus(`${t("连接中断")} · ${t("重放本轮")} ${p.attempt}/${p.max}`, true);
});
on("kz:compacted", (e) => {
  lastCompactionSummary = e.payload?.summary ?? "";
  addMessage("notice", `🗜 ${t("上下文占用过高,已自动压缩为纪要并延续对话")}`);
  if (lastCompactionSummary) addCompactionEntry(lastCompactionSummary);
  log(t("自动压缩完成:多轮历史已替换为纪要"));
  ctxTokens = 0;
  renderTokens();
});
on("kz:stopped", (e) => {
  cancelAutoContinueTimer();
  hideAsk();
  const cancelled = e.payload?.cancelled_queue ?? 0;
  addMessage("notice", cancelled > 0 ? `${t("已停止")}, ${t("已取消")} ${cancelled} ${t("条")} ${t("排队输入")}` : t("已停止"));
  log(cancelled > 0 ? `${t("已手动停止并取消")} ${cancelled} ${t("条")} ${t("排队输入")}` : t("已手动停止"));
  stopElapsed();
  setRunning(false, "已停止");
  bgAbortRunning(`(${t("已停止")})`);
  liveIdle("已停止");
  notifyRunState("stopped", cancelled > 0 ? `${t("已停止")}, ${t("已取消")} ${cancelled} ${t("条")} ${t("排队输入")}` : t("已停止"));
  refreshPendingInputs();
  refreshProcesses();
});
on("kz:done", async (e) => {
  const p = e.payload;
  setAutoStopReason(p.halted ? t("用户拒绝后停止") : t("本轮完成"));

  addMessage(
    "notice",
    `${t("完成")} · steps ${p.steps}${p.history ? ` · 会话 ${p.history} 条` : ""}${p.halted ? ` · ${t("按你的拒绝停止")}` : ""}`
  );
  log(`${t("运行完成")}: ${p.steps} ${t("轮")}, ${t("耗时")} ${((Date.now() - runStart) / 1000).toFixed(1)}s`);
  stopElapsed();
  notifyRunState(p.halted ? "stopped" : "completed", p.halted ? t("按你的拒绝停止") : `${t("完成")} ${p.steps} ${t("轮")}`);
  setRunning(false);
  // 对齐 Claude:当前对话跑完一轮就出现在历史列表里,不用等重启/切项目。
  refreshConversationList();
  // 活动面板保留本轮全部轨迹供回看,下一轮开跑时才翻页(kz:turn step 1)。
  liveIdle(`${t("空闲")} · ${t("上轮")} ${p.steps} ${t("轮")} ${t("完成")}`);
  refreshDocs();
  refreshGit();
  refreshPendingInputs();

  // 鞭挞:正常完成且上轮有实质动作(>1 轮 = 有工具调用)才续;拒绝/纯聊天即停。
  if (await stopAutoWhenBacklogEmpty()) return;
  if ($("auto-continue").checked && autoContinueAllowed() && !p.halted) {
    if (autoPaused) {
      addMessage("notice", `${t("鞭挞停止")}: ${t("处于暂停中,点顶栏「继续鞭挞」恢复")}`);
      setAutoStopReason("已暂停");
      return;
    }
    if (autoStopAfterRound) {
      autoStopAfterRound = false;
      $("auto-stop-round").checked = false;
      // 说清是哪个开关停的:否则用户只看到"停了",无从判断该去关什么。
      addMessage("notice", `${t("鞭挞停止")}:${t("本轮后停")}(${t("已自动取消勾选,再点鞭挞即可继续")})`);
      log(`${t("鞭挞停止")}:${t("本轮后停")}`);
      setAutoStopReason(`${t("本轮后停")},${t("已停止")}`);
      autoRounds = 0;
      noActionRounds = 0;
      return;
    }
    // 连数上限先于其它判定:追加推进指令也要占一轮,不能借这条路冲破上限。
    const max = autoContinueMax();
    if (autoRounds >= max) {
      addMessage("notice", `${t("鞭挞停止")}:${t("已达连上限,点继续或重开鞭挞")} (${max})`);
      setAutoStopReason(`${t("鞭挞停止")}:${t("已达连上限,点继续或重开鞭挞")}`);
      autoRounds = 0;
      noActionRounds = 0;
      return;
    }
    // 无实质动作(没有任何工具调用)的处理。模型不再有"声明阻塞"这条出口(用户定调:
    // 阻塞太好走),刹车只由机械条件触发:
    // ① 第一次无动作 → 先给一次具体的推进指令,不停;
    // ② 连续第二次无动作 → 停,避免空转烧钱(D-044 的教训)。
    if (p.steps <= 1 && autoRounds > 0) {
      if (noActionRounds === 0) {
        noActionRounds = 1;
        addMessage("notice", t("上一轮没有实质动作,已追加一次具体推进指令(再无动作才会停)"));
        log(`${t("鞭挞")}:${t("无动作 · 追加推进指令")}`);
        autoRounds += 1;
        renderAutoStatus(`${t("无动作 · 追加推进指令")} ${autoRounds}/${autoContinueMax()}`);
        cancelAutoContinueTimer();
        const generation = autoContinueGeneration;
        autoContinueTimer = setTimeout(() => {
          autoContinueTimer = null;
          if (generation !== autoContinueGeneration || autoPaused || autoStopAfterRound) return;
          if ($("auto-continue").checked && autoContinueAllowed() && !running) {
            sendText(nudgePrompt(), { auto: true });
          }
        }, 2000);
        return;
      }
      addMessage("notice", `${t("鞭挞停止")}:${t("连续两轮没有实质动作(可能目标已达成或确实无可推进项)")}`);
      log(`${t("鞭挞停止")}:${t("连续两轮无动作,鞭挞停止")}`);
      setAutoStopReason(t("连续两轮无动作,鞭挞停止"));
      autoRounds = 0;
      noActionRounds = 0;
      return;
    }
    noActionRounds = 0;
    autoRounds += 1;
    setStatus(`${t("自主推进")} ${autoRounds}/${max} · 2 ${t("秒后继续")}…`, false);
    renderAutoStatus(`${t("自主推进")} ${autoRounds}/${max} · ${t("等待下一轮")}`);
    scheduleAutoContinue();
  }
});

// ---------- 权限弹窗 ----------
const askQueues = new Map();
let askActive = null;

function askSessionId(payload) {
  return payload?.sessionId || activeSessionId || "__default__";
}

function askQueueFor(sessionId) {
  let queue = askQueues.get(sessionId);
  if (!queue) {
    queue = [];
    askQueues.set(sessionId, queue);
  }
  return queue;
}

on("kz:ask", (e) => {
  const sessionId = askSessionId(e.payload);
  e.payload.sessionId = sessionId;
  // 自动放行(yolo):后台会话也必须直接得到答复,不能因不在当前页签而挂起。
  if (e.payload.kind !== "question" && $("auto-allow").checked) {
    log(`${t("自动放行")}:${e.payload.action} ${e.payload.resource}`);
    invoke("answer_ask", { id: e.payload.id, reply: "once" }).catch((err) =>
      reportPersistentError(`${t("自动放行失败")}:${err}`)
    );
    return;
  }
  askQueueFor(sessionId).push(e.payload);
  if (sessionId === activeSessionId) pumpAsk();
});

$("auto-allow").checked = localStorage.getItem("kz-auto-allow") === "1";
$("auto-allow").addEventListener("change", () => {
  localStorage.setItem("kz-auto-allow", $("auto-allow").checked ? "1" : "0");
  log($("auto-allow").checked ? t("已开启自动放行(本会话所有权限询问直接通过)") : t("已关闭自动放行"));
});

function updateAskQueueStatus() {
  const queue = activeSessionId ? askQueueFor(activeSessionId) : [];
  const total = (askActive ? 1 : 0) + queue.length;
  const status = $("ask-queue-status");
  const preview = $("ask-queue-preview");
  status.textContent = total > 1
    ? `${t("当前请求")} 1/${total} · ${languageIsEnglish() ? `${total - 1} ${t("条待处理")}` : `${t("还有")} ${total - 1} ${t("条待处理")}`}`
    : t("当前无其他待处理请求");
  const lines = queue.slice(0, 4).map((item, index) => {
    const text = item.kind === "question" ? item.question : `${item.action} · ${item.resource}`;
    return `${index + 2}. ${text}`;
  });
  preview.textContent = lines.join("\n");
  preview.classList.toggle("hidden", lines.length === 0);
}

function pumpAsk() {
  if (askActive || !activeSessionId) {
    updateAskQueueStatus();
    return;
  }
  const queue = askQueueFor(activeSessionId);
  if (queue.length === 0) {
    updateAskQueueStatus();
    return;
  }
  askActive = queue.shift();
  const question = askActive.kind === "question";
  $("ask-title").textContent = question ? t("需要你的回答") : t("权限请求");
  $("permission-fields").classList.toggle("hidden", question);
  $("permission-buttons").classList.toggle("hidden", question);
  $("question-fields").classList.toggle("hidden", !question);
  $("question-buttons").classList.toggle("hidden", !question);
  if (question) {
    $("ask-question").textContent = askActive.question;
    const options = $("ask-options");
    options.innerHTML = "";
    for (const option of askActive.options || []) {
      const button = document.createElement("button");
      button.className = "ghost ask-option";
      button.textContent = option;
      button.addEventListener("click", () => answerAsk(option));
      options.appendChild(button);
    }
    $("ask-answer").value = askActive.default || "";
    setTimeout(() => $("ask-answer").focus(), 0);
  } else {
    $("ask-action").textContent = askActive.action;
    $("ask-resource").textContent = askActive.resource;
    $("ask-remember").textContent = `${askActive.action} ${askActive.remember ?? askActive.resource}`;
    setTimeout(() => $("ask-allow").focus(), 0);
  }
  $("ask-overlay").classList.remove("hidden");
  updateAskQueueStatus();
}

function hideAsk(preserveActive = false) {
  if (preserveActive && askActive) {
    askQueueFor(askActive.sessionId).unshift(askActive);
  } else if (activeSessionId) {
    askQueueFor(activeSessionId).length = 0;
  }
  askActive = null;
  $("ask-overlay").classList.add("hidden");
  updateAskQueueStatus();
}
async function answerAsk(reply) {
  if (!askActive) return;
  const id = askActive.id;
  const question = askActive.kind === "question";
  const summary = question ? askActive.question : `${askActive.action}: ${askActive.resource}`;
  askActive = null;
  $("ask-overlay").classList.add("hidden");
  updateAskQueueStatus();
  const replyLabel = reply === "deny" ? t("拒绝") : reply === "always" ? t("总是允许") : reply;
  log(`${question ? t("回答") : t("权限")} ${replyLabel} — ${summary}`);
  try {
    await invoke("answer_ask", { id, reply });
  } catch (err) {
    reportPersistentError(`${t("权限应答失败")}:${err}`);
  }
  pumpAsk();
}

$("ask-deny").addEventListener("click", () => answerAsk("deny"));
$("ask-always").addEventListener("click", () => answerAsk("always"));
$("ask-allow").addEventListener("click", () => answerAsk("once"));
$("ask-cancel").addEventListener("click", () => answerAsk("cancel"));
$("ask-submit").addEventListener("click", () => answerAsk($("ask-answer").value.trim()));
$("ask-answer").addEventListener("keydown", (event) => {
  if (event.key === "Enter") answerAsk($("ask-answer").value.trim());
});
document.addEventListener("keydown", (event) => {
  if (event.key !== "Escape") return;
  if (!$('ask-overlay').classList.contains("hidden") && askActive) {
    answerAsk(askActive.kind === "question" ? "cancel" : "deny");
    return;
  }
  if (!$('viewer-overlay').classList.contains("hidden")) $("viewer-close").click();
});

// ---------- 阅读辅助 ----------
async function copyReadable(el) {
  const text = el.dataset.raw || [...el.childNodes]
    .filter((node) => !(node.nodeType === Node.ELEMENT_NODE && node.classList.contains("msg-actions")))
    .map((node) => node.textContent || "")
    .join("")
    .trim();
  if (!text) return toast(t("没有可复制的内容"));
  try {
    await navigator.clipboard.writeText(text);
    toast(t("已复制"));
  } catch (err) {
    toastError(`${t("复制失败")}:${err}`);
  }
}
messages.addEventListener("click", (event) => {
  const button = event.target.closest(".copy-btn");
  if (button) copyReadable(button.closest(".msg, .tool-chip"));
});

// ---------- 复制上下文:整段对话导出为 markdown(贴给其他 AI 用) ----------
$("copy-context").addEventListener("click", async () => {
  const parts = [];
  for (const el of messages.children) {
    if (el.classList.contains("user")) {
      const text = (el.querySelector(".message-body")?.textContent ?? el.textContent).trim();
      if (text) parts.push(`## ${t("用户")}\n${text}`);
    } else if (el.classList.contains("assistant")) {
      const raw = (el.dataset.raw ?? el.textContent).trim();
      if (raw) parts.push(`## ${t("助手")}\n${raw}`);
    } else if (el.classList.contains("reasoning")) {
      const raw = el.querySelector(".reasoning-body")?.dataset.raw?.trim();
      if (raw) parts.push(`> ${t("思考")}:${raw.split("\n").find(Boolean)?.slice(0, 160) ?? ""}`);
    } else if (el.classList.contains("tool-chip")) {
      const head = el.querySelector(".head")?.textContent?.trim();
      if (head) parts.push(`> ${t("工具")}:${head.slice(0, 200)}`);
    } else if (el.classList.contains("turn-divider")) {
      parts.push(`---\n${el.textContent}`);
    }
  }
  if (!parts.length) {
    toast(t("当前没有可复制的对话"));
    return;
  }
  try {
    await navigator.clipboard.writeText(parts.join("\n\n"));
    toast(`${t("已复制上下文")}(${parts.length} ${t("段")})`);
  } catch (err) {
    toastError(`${t("复制上下文失败")}:${err}`);
  }
});

let searchMatches = [];
let searchIndex = 0;
function updateSearch() {
  const query = $("chat-search-input").value.trim().toLowerCase();
  document.querySelectorAll(".search-hit, .search-current").forEach((el) => el.classList.remove("search-hit", "search-current"));
  searchMatches = query ? [...messages.querySelectorAll(".msg, .tool-chip")].filter((el) => el.textContent.toLowerCase().includes(query)) : [];
  searchIndex = Math.min(searchIndex, Math.max(0, searchMatches.length - 1));
  searchMatches.forEach((el) => el.classList.add("search-hit"));
  if (searchMatches.length) {
    const current = searchMatches[searchIndex];
    current.classList.add("search-current");
    current.scrollIntoView({ block: "center" });
  }
  $("chat-search-count").textContent = query ? `${searchMatches.length ? searchIndex + 1 : 0}/${searchMatches.length}` : "";
}
function moveSearch(delta) {
  if (!searchMatches.length) return;
  searchIndex = (searchIndex + delta + searchMatches.length) % searchMatches.length;
  updateSearch();
}
$("chat-search-toggle").addEventListener("click", () => {
  const bar = $("chat-search");
  bar.classList.toggle("hidden");
  if (!bar.classList.contains("hidden")) $("chat-search-input").focus();
});
$("chat-search-input").addEventListener("input", () => { searchIndex = 0; updateSearch(); });
$("chat-search-input").addEventListener("keydown", (event) => {
  if (event.key === "Enter") moveSearch(event.shiftKey ? -1 : 1);
  if (event.key === "Escape") $("chat-search").classList.add("hidden");
});
$("chat-search-prev").addEventListener("click", () => moveSearch(-1));
$("chat-search-next").addEventListener("click", () => moveSearch(1));
$("jump-latest").addEventListener("click", () => {
  followLatest = true;
  scrollBottom(true);
  messages.scrollTo({ top: messages.scrollHeight, behavior: "smooth" });
});

// ---------- 发送 / 停止 ----------
// 鞭挞状态:自动续跑计数(手动发送归零),上限防失控。
const DEFAULT_AUTO_CONTINUE_MAX = 10;
let autoRounds = 0;
let autoPaused = false;
let autoStopAfterRound = false;
let autoContinueTimer = null;
let autoContinueGeneration = 0;
let autoStopReason = "";
// 连续无实质动作的轮数:第一次只追加推进指令,第二次才刹车。
let noActionRounds = 0;
const DEFAULT_CONTINUE_PROMPT =
  "继续推进。取活顺序按本轮末尾给出的「开发重心」执行(它来自记忆里的用户定调,是唯一权威);" +
  "两个队列内部都按文档顺序自上而下拿第一个可做的,列表已按阶段排好,不要自行挑看起来容易的。\n" +
  "1. 本轮必须产生落地动作:改代码或跑测试。先做再说明,不要只做判断。\n" +
  "2. 粒度 = 一轮一个完整条目:以做完当前这一条缺陷/需求为本轮目标;" +
  "同构批量改动(i18n、重命名、迁移这类)一轮吃掉完整类别,不要按两三处微切片。" +
  "确实超出单轮容量才按验收子项分轮,并在进展里写明批次边界。" +
  "「工作量大」「要改多个文件」都是正常工作,不是停下的理由。\n" +
  "3. 卡住就换一条:某条一时推不动,在「进展」里记一句原因,直接跳到下一条继续,不要停下来等。\n" +
  "「阻塞」字段只写解除权不在你手里的事(已问过用户在等回复/缺凭据/依赖外部服务/用户直营)," +
  "且要写出具名解除人;「涉及多文件」「跨层改动」「需先确认方案(但没真问过)」都不是阻塞,写进展。" +
  "顺手复核碰到的条目:阻塞条件已满足的当场清空「阻塞」字段。看到 [调度死锁] 横幅时按横幅执行。\n" +
  "4. 关闭条目前逐条对照验收原文,每项给出精确代码位置证据;声称完成的能力必须有真实调用方或消费者," +
  "没有消费者的命令、死代码或只展示不接数据源的壳不算完成;沿用既有实现要显式标注为既有能力而非本次交付;" +
  "不得缩小验收里的平台或范围限定词。任一项证据不足就保留活动态写清缺口,不要打勾。\n" +
  "5. doing 最多 2 个;已满就继续推进这两项。标着「阶段 5 后」的功能需求暂不启动。\n" +
  "6. 已通过测试的未提交改动,先按规范 §6 用 git 提交(不带署名)再继续。" +
  "验证选择与改动面匹配:纯 ui/ 改动跑 node 检查与冒烟脚本,动了 crates/ 才跑 cargo test。\n" +
  "一直做下去,不要用纯文本收尾。";

// 旧版默认文案:用户没改过(存的就是某个旧默认)时静默升级到新默认,
// 否则鞭挞的刹车契约会和提示词对不上(用户自定义过的文案不动)。
const LEGACY_CONTINUE_PROMPTS = [
  // 开发重心版:规则 3 只说"在条目里记一句原因",模型把它落成「阻塞」字段,
  // 而调度器把该字段当永久压制 → 31/35 条目被自记阻塞锁死(D-163)。
  "继续推进。取活顺序按本轮末尾给出的「开发重心」执行(它来自记忆里的用户定调,是唯一权威);" +
    "两个队列内部都按文档顺序自上而下拿第一个可做的,列表已按阶段排好,不要自行挑看起来容易的。\n" +
    "1. 本轮必须产生落地动作:改代码或跑测试。先做再说明,不要只做判断。\n" +
    "2. 粒度 = 一轮一个完整条目:以做完当前这一条缺陷/需求为本轮目标;" +
    "同构批量改动(i18n、重命名、迁移这类)一轮吃掉完整类别,不要按两三处微切片。" +
    "确实超出单轮容量才按验收子项分轮,并在进展里写明批次边界。" +
    "「工作量大」「要改多个文件」都是正常工作,不是停下的理由。\n" +
    "3. 卡住就换一条:某条一时推不动,在条目里记一句原因,直接跳到下一条继续,不要停下来等。\n" +
    "4. 关闭条目前对照验收原文,给出改动位置;没有调用方的命令或按钮不算完成;" +
    "沿用既有实现要说明。拿不准就保留活动态写清缺口,不要打勾。\n" +
    "5. doing 最多 2 个;已满就继续推进这两项。标着「阶段 5 后」的功能需求暂不启动。\n" +
    "6. 已通过测试的未提交改动,先按规范 §6 用 git 提交(不带署名)再继续。" +
    "验证选择与改动面匹配:纯 ui/ 改动跑 node 检查与冒烟脚本,动了 crates/ 才跑 cargo test。\n" +
    "一直做下去,不要用纯文本收尾。",
  // 硬编码取活顺序版:开篇写死"先扫 defects.md",与结尾追加的取活模式行直接矛盾,
  // 开篇权威句胜出 → 用户切「需求优先」始终不生效(D-128)。
  "继续推进。取活顺序 = 文档顺序:先从上往下扫 defects.md,再扫 requirements.md," +
    "拿第一个可做的。列表已按阶段排好,不要自行挑看起来容易的。\n" +
    "1. 本轮必须产生落地动作:改代码或跑测试。先做再说明,不要只做判断。\n" +
    "2. 粒度 = 一轮一个完整条目:以做完当前这一条缺陷/需求为本轮目标;" +
    "同构批量改动(i18n、重命名、迁移这类)一轮吃掉完整类别,不要按两三处微切片。" +
    "确实超出单轮容量才按验收子项分轮,并在进展里写明批次边界。" +
    "「工作量大」「要改多个文件」都是正常工作,不是停下的理由。\n" +
    "3. 卡住就换一条:某条一时推不动,在条目里记一句原因,直接跳到下一条继续,不要停下来等。\n" +
    "4. 关闭条目前对照验收原文,给出改动位置;没有调用方的命令或按钮不算完成;" +
    "沿用既有实现要说明。拿不准就保留活动态写清缺口,不要打勾。\n" +
    "5. doing 最多 2 个;已满就继续推进这两项。标着「阶段 5 后」的功能需求暂不启动。\n" +
    "6. 已通过测试的未提交改动,先按规范 §6 用 git 提交(不带署名)再继续。" +
    "验证选择与改动面匹配:纯 ui/ 改动跑 node 检查与冒烟脚本,动了 crates/ 才跑 cargo test。\n" +
    "一直做下去,不要用纯文本收尾。",
  // 微切片版:「最小可执行步骤」导致 i18n 类批量任务两三处一轮,单条缺陷拖 30+ 轮(D-114,用户定调改为一轮一条目)。
  "继续推进。取活顺序 = 文档顺序:先从上往下扫 defects.md,再扫 requirements.md," +
    "拿第一个可做的。列表已按阶段排好,不要自行挑看起来容易的。\n" +
    "1. 本轮必须产生落地动作:改代码或跑测试。先做再说明,不要只做判断。\n" +
    "2. 大项拆着做,本轮只推进下一个最小可执行步骤。" +
    "「工作量大」「要改多个文件」「需要多轮」都是正常工作,不是停下的理由。\n" +
    "3. 卡住就换一条:某条一时推不动,在条目里记一句原因,直接跳到下一条继续,不要停下来等。\n" +
    "4. 关闭条目前对照验收原文,给出改动位置;没有调用方的命令或按钮不算完成;" +
    "沿用既有实现要说明。拿不准就保留活动态写清缺口,不要打勾。\n" +
    "5. doing 最多 2 个;已满就继续推进这两项。标着「阶段 5 后」的功能需求暂不启动。\n" +
    "6. 已通过测试的未提交改动,先按规范 §6 用 git 提交(不带署名)再继续。\n" +
    "一直做下去,不要用纯文本收尾。",
  "继续:先检查缺陷列表,再检查需求与活跃目标,推进下一个具体步骤并落地(改代码/跑测试/更新文档);" +
    "完成后用 goal update 记录状态。收尾优先:已是 doing 的事项先关闭再开新的,doing 同时不超过 2 个。" +
    "取活顺序:按缺陷列表优先,随后按需求列表自上而下拿第一个可做的(列表顺序即用户意志,priority 只是背景信息)。" +
    "若工作区有已通过测试的未提交改动,先按规范 §6 用 git 提交(不带署名)再继续。" +
    "若活跃目标/需求全部被阻塞或无可推进项:只用【纯文本】说明原因并停住——" +
    "不要调用任何工具、不要往 goal/req 写'仍在阻塞'类记录、不要产生空提交;" +
    "纯文本回复会让鞭挞自动刹车,写阻塞日记则会让它空转烧钱。",
  // D-097 版:引入【阻塞】刹车契约,但还没有阶段/证据等级与完成判定约束。
  "继续推进。取活顺序:缺陷列表优先,然后按需求列表自上而下拿第一个可做的" +
    "(列表顺序即用户意志,priority 只是背景信息)。\n" +
    "1. 本轮必须产生一个具体落地动作:改代码、跑测试、或更新文档。先做再说明,不要只做判断。\n" +
    "2. 大项拆着做:复杂度大的条目不要求本轮关闭,只要推进它的下一个最小可执行步骤。" +
    "「工作量大」「要改多个文件」「需要多轮」都不是阻塞,是正常工作。\n" +
    "3. doing 已满 2 个不代表没事可做——那意味着继续推进这两个 doing 项,而不是停下。\n" +
    "4. 已通过测试的未提交改动,先按规范 §6 用 git 提交(不带署名)再继续。\n" +
    "5. 只有确实缺少外部输入时才算阻塞:等待用户拍板、缺凭据/权限、依赖外部服务或他人。" +
    "此时回复以【阻塞】开头的纯文本,写清缺什么、解除条件是什么,不要调用任何工具、" +
    "不要往 goal/req/defect 写「仍在阻塞」类记录、不要产生空提交。\n" +
    "除【阻塞】外不要用纯文本收尾——没有动作的一轮会被判为空转。",
  // 证据等级版:测试门槛过严,且仍保留【阻塞】出口(用户定调:阻塞太好走,取消)。
  "继续推进。取活顺序 = 文档顺序:先从上往下扫 defects.md,再从上往下扫 requirements.md," +
    "拿第一个可做的。列表已按质量阶段排好,不要自行挑看起来容易的。\n" +
    "1. 本轮必须产生落地动作:改代码、跑测试或补证据。先做再说明,不要只做判断。\n" +
    "2. 每条都带 `阶段`/`不变量`/`证据等级`。修复要保护该不变量本身,不能把故障挪到另一条路径;" +
    "验证要达到标注的证据等级——E2 需跨模块/并发/故障注入,E3 需真实运行时;" +
    "单元测试证明不了 E2 结论,静态检查证明不了任何运行时结论。\n" +
    "3. 关闭条目前逐条对照验收原文:每项给出代码位置证据;声称完成的能力必须有真实调用方" +
    "(没有消费者的命令或按钮判为未完成);沿用既有实现要显式标注为既有能力;" +
    "不得缩小验收里的平台或范围限定词。做不到就保留活动态并写清缺口,不要打勾。\n" +
    "4. 大项拆着做:本轮只推进下一个最小可执行步骤。" +
    "「工作量大」「要改多个文件」「需要多轮」都不是阻塞,是正常工作。\n" +
    "5. doing 最多 2 个;已满就继续推进这两项,不是停下。标着「阶段 5 后」的功能需求," +
    "在质量收口完成前不启动。\n" +
    "6. 已通过测试的未提交改动,先按规范 §6 用 git 提交(不带署名)再继续。\n" +
    "7. 只有确实缺少外部输入时才算阻塞:等待用户拍板、缺凭据/权限、依赖外部服务或他人。" +
    "此时回复以【阻塞】开头的纯文本,写清缺什么、解除条件是什么,不要调用任何工具、" +
    "不要往 goal/req/defect 写「仍在阻塞」类记录、不要产生空提交。\n" +
    "除【阻塞】外不要用纯文本收尾——没有动作的一轮会被判为空转。",
];
// 没有实质动作时先给一次具体的推进指令,而不是直接停:一轮无动作往往是模型
// 在"这条该不该做"上想岔了,而不是真没活干(D-097)。
const NUDGE_PROMPT =
  "上一轮没有产生任何实质动作。不要再做可行性判断,直接执行:\n" +
  "从 defects.md 最上面一条开始,说出它的下一个最小可执行步骤(具体到文件和改动),然后立刻做掉。\n" +
  "那一条一时推不动就跳到下一条,需求同理——总有一条是能动手的。\n" +
  "如果每一条都标着阻塞:先复核阻塞是否还成立。多数是你自己历轮写下的,解除条件早已满足,\n" +
  "清空这些条目的「阻塞」字段再取活;真正卡住的只有等用户拍板的那几条,把它们点名列给用户。\n" +
  "不要为了凑动作去做与当前条目无关的事,也不要只更新追踪文档就算一轮。";

function nudgePrompt() {
  const first = selectedWorkPriority() === "requirement-first" ? "requirements.md" : "defects.md";
  const second = selectedWorkPriority() === "requirement-first" ? "defects.md" : "requirements.md";
  return NUDGE_PROMPT.replace("defects.md", first).replace("需求同理", `${second} 同理`);
}

function selectedAgent() {
  const mode = $("profile-select").value;
  if (mode === "dev-pair") return { profile: "dev", agent: "dev-pair" };
  if (mode === "dev-auto") return { profile: "dev", agent: "dev" };
  return { profile: "research", agent: "research" };
}
function workPriorityStorageKey() {
  return `kz-work-priority:${currentProject || "default"}`;
}
function selectedWorkPriority() {
  return $("work-priority-select").value === "requirement-first" ? "requirement-first" : "defect-first";
}
function syncWorkPriorityControl() {
  const saved = localStorage.getItem(workPriorityStorageKey());
  $("work-priority-select").value = saved === "requirement-first" ? saved : "defect-first";
  loadWorkFocus();
}

// 开发重心 = preference 记忆条目(真源)。下拉框只是快捷写法,记忆页可手写任意细度
// (「先收完这批缺陷再转需求」这类二元开关表达不了的意图);提示词由记忆生成,
// 所以开关与提示词不可能再互相矛盾——D-128 的根因就是二者写死后对打。
let workFocusMemory = null;
const WORK_FOCUS_PRESETS = {
  "defect-first": {
    title: "开发重心:缺陷优先",
    body: "取活顺序:先从上到下扫描 defects.md,再扫描 requirements.md;前一个队列没有可做项时才看后一个。\npriority 标签只是背景信息,不改变列表顺序。",
  },
  "requirement-first": {
    title: "开发重心:需求优先",
    body: "取活顺序:先从上到下扫描 requirements.md,再扫描 defects.md;前一个队列没有可做项时才看后一个。\npriority 标签只是背景信息,不改变列表顺序。",
  },
};
async function loadWorkFocus() {
  if (!currentProject) return;
  try {
    workFocusMemory = await invoke("memory_focus_get", { projectDir: currentProject });
  } catch {
    workFocusMemory = null;
  }
  // 回显:手写的自定义重心不强行归入两个预设,保持用户当前选择不被覆盖。
  const title = workFocusMemory?.title || "";
  if (title.includes("需求优先")) $("work-priority-select").value = "requirement-first";
  else if (title.includes("缺陷优先")) $("work-priority-select").value = "defect-first";
}

function renderAutoStatus(text = autoStopReason) {
  const el = $("auto-status");
  if (!el) return;
  const max = autoContinueMax();
  el.textContent = localizeDynamic(text || `连续推进上限 ${max}`);
}
function continuePrompt() {
  const base = $("continue-prompt").value.trim() || DEFAULT_CONTINUE_PROMPT;
  // 重心正文优先取记忆(用户可手写细度);记忆缺失时回落到下拉框预设。
  const focus = workFocusMemory?.body?.trim() || WORK_FOCUS_PRESETS[selectedWorkPriority()].body;
  const from = workFocusMemory?.id ? `记忆 ${workFocusMemory.id}` : "当前选择";
  return `${base}\n开发重心(来自${from},这是取活顺序的唯一权威):\n${focus}`;
}

function setAutoStopReason(reason) {
  autoStopReason = reason;
  renderAutoStatus(reason);
}
function autoContinueAllowed() {
  return $("profile-select").value === "dev-auto";
}
function autoContinueMax() {
  const value = Number.parseInt($("auto-max").value, 10);
  return Number.isFinite(value) ? Math.min(100, Math.max(1, value)) : DEFAULT_AUTO_CONTINUE_MAX;
}
function cancelAutoContinueTimer() {
  if (autoContinueTimer) clearTimeout(autoContinueTimer);
  autoContinueTimer = null;
  autoContinueGeneration += 1;
}
function scheduleAutoContinue() {
  cancelAutoContinueTimer();
  const generation = autoContinueGeneration;
  autoContinueTimer = setTimeout(() => {
    autoContinueTimer = null;
    if (generation !== autoContinueGeneration || autoPaused || autoStopAfterRound) return;
    if ($("auto-continue").checked && autoContinueAllowed() && !running) {
      sendText(continuePrompt(), { auto: true });
    }
  }, 2000);
}

async function stopAutoWhenBacklogEmpty() {
  if (!$("auto-continue").checked || !autoContinueAllowed()) return false;
  try {
    const snapshot = await invoke("docs_snapshot", { projectDir: currentProject });
    const active = [...(snapshot.requirements ?? []), ...(snapshot.defects ?? [])]
      .some((entry) => !entry.closed && !["done", "dropped", "fixed", "wontfix"].includes(entry.status));
    if (active) return false;
    $("auto-continue").checked = false;
    localStorage.setItem("kz-auto-continue", "0");
    autoRounds = 0;
    cancelAutoContinueTimer();
    setAutoStopReason(t("需求与缺陷已清空，自动推进已停止"));
    addMessage("notice", `✅ ${t("需求与缺陷已清空，自动推进已停止")}`);
    log(t("自动推进停止:需求与缺陷已清空"));
    return true;
  } catch (error) {
    log(`${t("检查需求/缺陷是否清空失败")}:${error}`, "warn");
    return false;
  }
}
function renderAttachments() {
  const box = $("attachments");
  box.innerHTML = "";
  box.classList.toggle("hidden", attachments.length === 0);
  attachments.forEach((item, index) => {
    const chip = document.createElement("button");
    chip.type = "button";
    chip.className = "attachment-chip";
    chip.textContent = `${item.file_name} ×`;
    chip.title = t("移除附件");
    chip.setAttribute("aria-label", `${t("移除附件")} ${item.file_name}`);
    chip.addEventListener("click", () => { attachments.splice(index, 1); renderAttachments(); });
    box.appendChild(chip);
  });
}

function addFiles(files) {
  for (const file of files) {
    if (!(file.type.startsWith("image/") || file.type === "application/pdf" || file.name.toLowerCase().endsWith(".pdf"))) {
      toast(`${t("不支持的附件类型")}: ${file.name}`);
      continue;
    }
    const reader = new FileReader();
    reader.onload = () => {
      const dataUrl = String(reader.result);
      attachments.push({ file_name: file.name, media_type: file.type || "application/pdf", data: dataUrl.split(",", 2)[1] || "" });
      renderAttachments();
    };
    reader.onerror = () => toastError(`${t("读取附件失败")}: ${file.name}`);
    reader.readAsDataURL(file);
  }
}

$("attach").addEventListener("click", () => $("attachment-input").click());
$("attachment-input").addEventListener("change", (e) => { addFiles(e.target.files); e.target.value = ""; });
promptBox.addEventListener("dragover", (e) => { e.preventDefault(); });
promptBox.addEventListener("drop", (e) => { e.preventDefault(); addFiles(e.dataTransfer.files); });
promptBox.addEventListener("paste", (e) => {
  const files = [...(e.clipboardData?.files || [])];
  if (files.length) { e.preventDefault(); addFiles(files); }
});

async function sendText(prompt, { auto = false, promptAttachments = [] } = {}) {
  // 任何拒绝发送的理由都要说出来,绝不静默(D-004)。
  if (!prompt) return;
  const delivery = $("delivery-select").value;
  if (running && auto) {
    toast(t("当前任务还在运行，自动鞭挞将在本轮完成后继续"));
    return;
  }
  if (!currentProject) {
    toast(t("先在左侧「项目」里添加并选择一个目录"));
    return;
  }
  if (!auto) void ensureNotificationPermission();
  if (running) {
    addMessage("user", prompt);
    log(`${t("运行中")}${delivery === "steer" ? t("插入") : t("排队")}:${prompt.slice(0, 80)}`);
    try {
      const mode = selectedAgent();
      await invoke("run_prompt", {
        prompt,
        projectDir: currentProject,
        profile: mode.profile,
        agent: mode.agent,
        model: $("model-select").value || null,
        delivery,
        attachments: promptAttachments,
        processId: activeProcessId,
      });
      toast(localizeDynamic(delivery === "steer" ? "已插入当前会话，将优先执行" : "已加入队列，将按顺序执行"));
      await refreshPendingInputs();
    } catch (err) {
      reportError(String(err), { retryable: false });
    }
    return;
  }
  if (!auto) {
    autoRounds = 0;
    noActionRounds = 0;
    cancelAutoContinueTimer();
  }
  currentAssistant = null;
  currentReasoning = null;
  runTokens = { input: 0, output: 0, cacheRead: 0, cacheWrite: 0 };
  ctxTokens = 0;
  outputChars = 0;
  renderTokens();
  const attachmentStatus = promptAttachments.length > 0
    ? `${auto ? `${t("鞭挞")} ${autoRounds}/${autoContinueMax()} · ` : ""}${t("正在发送")} ${promptAttachments.length} ${t("个附件")} · ${t("准备中")}`
    : auto ? `${t("鞭挞")} ${autoRounds}/${autoContinueMax()} · ${t("准备中")}` : t("准备中");
  if (auto) {
    addMessage("notice", `${t("鞭挞已触发")} · ${autoRounds}/${autoContinueMax()}`);
  } else {
    addUserMessage(prompt, promptAttachments);
  }
  setRunning(true, attachmentStatus);
  startElapsed();
  log(`${auto ? t("鞭挞") : t("发送")}:${prompt.slice(0, 80)}`);
  try {
    const mode = selectedAgent();
    const request = {
      prompt,
      projectDir: currentProject,
      profile: mode.profile,
      agent: mode.agent,
      model: $("model-select").value || null,
      workPriority: selectedWorkPriority(),
      delivery,
      attachments: promptAttachments.map((item) => ({ ...item })),
      processId: activeProcessId,
    };
    if (!auto) lastRequest = request;
    await invoke("run_prompt", request);
  } catch (err) {
    reportError(String(err));
    stopElapsed();
    setRunning(false);
  }
}

const PROMPT_HISTORY_KEY = "kz-prompt-history";
const PROMPT_HISTORY_LIMIT = 30;
let promptHistory = (() => {
  try { return JSON.parse(localStorage.getItem(PROMPT_HISTORY_KEY) || "[]").filter((item) => typeof item === "string"); }
  catch (_) { return []; }
})();
let promptHistoryIndex = -1;
let promptHistoryDraft = "";

function rememberPrompt(prompt) {
  const value = prompt.trim();
  if (!value) return;
  promptHistory = [value, ...promptHistory.filter((item) => item !== value)].slice(0, PROMPT_HISTORY_LIMIT);
  localStorage.setItem(PROMPT_HISTORY_KEY, JSON.stringify(promptHistory));
  promptHistoryIndex = -1;
}

function navigatePromptHistory(direction) {
  if (promptHistory.length === 0) return false;
  if (promptHistoryIndex === -1) promptHistoryDraft = promptBox.value;
  const next = promptHistoryIndex + direction;
  if (next < 0 || next > promptHistory.length) return false;
  promptHistoryIndex = next;
  promptBox.value = next === promptHistory.length ? promptHistoryDraft : promptHistory[next];
  promptBox.setSelectionRange(promptBox.value.length, promptBox.value.length);
  return true;
}

let fileSuggestions = [];
let fileSuggestionIndex = -1;
let fileSuggestionToken = null;
let fileSuggestionRequest = 0;

function currentFileToken() {
  const cursor = promptBox.selectionStart;
  const before = promptBox.value.slice(0, cursor);
  const match = before.match(/(?:^|\s)@([^\s]*)$/);
  if (!match) return null;
  return { start: cursor - match[1].length - 1, end: cursor, query: match[1] };
}

function hideFileSuggestions() {
  fileSuggestions = [];
  fileSuggestionIndex = -1;
  fileSuggestionToken = null;
  $("file-suggestions").classList.add("hidden");
  $("file-suggestions").replaceChildren();
}

function renderFileSuggestions() {
  const box = $("file-suggestions");
  box.replaceChildren();
  fileSuggestions.forEach((path, index) => {
    const button = document.createElement("button");
    button.type = "button";
    button.className = `file-suggestion${index === fileSuggestionIndex ? " active" : ""}`;
    button.textContent = `@${path}`;
    button.addEventListener("mousedown", (event) => {
      event.preventDefault();
      chooseFileSuggestion(index);
    });
    box.appendChild(button);
  });
  box.classList.toggle("hidden", fileSuggestions.length === 0);
}

function chooseFileSuggestion(index = fileSuggestionIndex) {
  const path = fileSuggestions[index];
  const token = currentFileToken() || fileSuggestionToken;
  if (!path || !token) return;
  promptBox.value = `${promptBox.value.slice(0, token.start)}@${path} ${promptBox.value.slice(token.end)}`;
  const cursor = token.start + path.length + 2;
  promptBox.focus();
  promptBox.setSelectionRange(cursor, cursor);
  hideFileSuggestions();
}

async function refreshFileSuggestions() {
  const token = currentFileToken();
  if (!token || !currentProject) {
    hideFileSuggestions();
    return;
  }
  fileSuggestionToken = token;
  const request = ++fileSuggestionRequest;
  try {
    const paths = await invoke("project_files", { projectDir: currentProject, query: token.query });
    if (request !== fileSuggestionRequest || !currentFileToken()) return;
    fileSuggestions = paths;
    fileSuggestionIndex = paths.length ? 0 : -1;
    renderFileSuggestions();
  } catch (error) {
    hideFileSuggestions();
    log(`文件补全失败:${error}`, "warn");
  }
}

let fileSuggestionTimer = null;
promptBox.addEventListener("input", () => {
  promptHistoryIndex = -1;
  clearTimeout(fileSuggestionTimer);
  fileSuggestionTimer = setTimeout(refreshFileSuggestions, 80);
});
function stopAutoForManualInput() {
  if (!$('auto-continue').checked) return false;
  $('auto-continue').checked = false;
  localStorage.setItem("kz-auto-continue", "0");
  autoRounds = 0;
  noActionRounds = 0;
  cancelAutoContinueTimer();
  const message = t("收到手动输入，鞭挞已停止");
  setAutoStopReason(message);
  addMessage("notice", message);
  toast(message);
  log(message);
  return true;
}

function send() {
  const prompt = promptBox.value.trim();
  if (!prompt && attachments.length === 0) return;
  stopAutoForManualInput();
  // 只有附件没有文字时,sendText 的空 prompt 早退会静默吞掉附件(附件在此已被清空)。
  // 给一句默认描述,让图片/文件真的发得出去。
  if (!prompt && attachments.length > 0) {
    sendText(t("看一下这些附件"), { promptAttachments: attachments });
    promptBox.value = "";
    attachments = [];
    renderAttachments();
    return;
  }
  rememberPrompt(prompt);
  hideFileSuggestions();
  const promptAttachments = attachments;
  promptBox.value = "";
  attachments = [];
  renderAttachments();
  sendText(prompt, { promptAttachments });
}

$("send").addEventListener("click", send);
$("continue-btn").addEventListener("click", () => sendText(continuePrompt()));

async function openSopPicker() {
  if (!currentProject) {
    toast(t("先在左侧「项目」里添加并选择一个目录"));
    return;
  }
  const panel = $("sop-picker-panel");
  const list = $("sop-list");
  panel.classList.remove("hidden");
  list.replaceChildren();
  const loading = document.createElement("p");
  loading.className = "dim";
  loading.textContent = `${t("选择 SOP")}…`;
  list.appendChild(loading);
  try {
    const scopes = await Promise.all(["project", "global"].map((scope) =>
      invoke("memory_entries", { projectDir: currentProject, scope, category: "sop" })
    ));
    const entries = scopes.flat().filter((entry) => entry.status === "active");
    list.replaceChildren();
    if (!entries.length) {
      const empty = document.createElement("p");
      empty.className = "dim";
      empty.textContent = t("暂无可调用的 SOP");
      list.appendChild(empty);
      return;
    }
    for (const entry of entries) {
      const button = document.createElement("button");
      button.type = "button";
      button.className = "sop-entry";
      const title = document.createElement("strong");
      title.textContent = entry.title;
      const description = document.createElement("span");
      description.className = "dim";
      description.textContent = entry.description || entry.body?.slice(0, 120) || "";
      button.append(title, description);
      button.addEventListener("click", () => {
        const content = String(entry.body || "").trim();
        promptBox.value = content;
        panel.classList.add("hidden");
        stopAutoForManualInput();
        promptBox.focus();
        if (!content) {
          toast(t("SOP 内容为空"));
          return;
        }
        rememberPrompt(content);
        const delivery = $("delivery-select");
        const previous = delivery.value;
        delivery.value = "queue";
        void sendText(content).finally(() => { delivery.value = previous; });
        toast(t("SOP 已填入继续输入"));
      });
      list.appendChild(button);
    }
  } catch (error) {
    list.replaceChildren();
    const failed = document.createElement("p");
    failed.className = "dim";
    failed.textContent = `${t("SOP 加载失败")}: ${error}`;
    list.appendChild(failed);
  }
}
$("sop-picker").addEventListener("click", openSopPicker);
$("sop-picker-close").addEventListener("click", () => $("sop-picker-panel").classList.add("hidden"));

$("continue-toggle").addEventListener("click", () => {
  const panel = $("continue-panel");
  const open = panel.classList.toggle("hidden") === false;
  $("continue-toggle").setAttribute("aria-expanded", String(open));
  $("continue-toggle").textContent = t(open ? "收起文案" : "继续文案");
  if (open) $("continue-prompt").focus();
});
$("auto-continue").checked = localStorage.getItem("kz-auto-continue") === "1";
renderAutoStatus();
// 存的是旧默认文案时静默升级:否则刹车契约(【阻塞】标记)与提示词对不上,
// 用户自己改过的文案不动。
{
  const stored = (localStorage.getItem("kz-continue-prompt") || "").trim();
  const isLegacyDefault = LEGACY_CONTINUE_PROMPTS.some((old) => old.trim() === stored);
  if (!stored || isLegacyDefault) {
    localStorage.setItem("kz-continue-prompt", DEFAULT_CONTINUE_PROMPT);
    $("continue-prompt").value = DEFAULT_CONTINUE_PROMPT;
    if (isLegacyDefault) log(t("继续文案已升级到新版(含【阻塞】刹车约定)"));
  } else {
    $("continue-prompt").value = stored;
  }
}
$("continue-prompt").addEventListener("change", () => {
  const value = $("continue-prompt").value.trim();
  localStorage.setItem("kz-continue-prompt", value || DEFAULT_CONTINUE_PROMPT);
  $("continue-prompt").value = value || DEFAULT_CONTINUE_PROMPT;
});
$("auto-max").value = Math.min(100, Math.max(1, Number.parseInt(localStorage.getItem("kz-auto-max"), 10) || DEFAULT_AUTO_CONTINUE_MAX));
// 「本轮后停」是一次性意图,不是偏好:绝不持久化。
// 曾经持久化过——勾一次后 localStorage 永远是 "1",每次启动都重新武装,
// 表现为"鞭挞跑一轮就停,怎么都停不掉"(D-111)。这里顺手清掉存量键。
localStorage.removeItem("kz-auto-stop-round");
$("auto-stop-round").checked = false;
autoStopAfterRound = false;
$("auto-pause").addEventListener("click", () => {
  autoPaused = !autoPaused;
  $("auto-pause").classList.toggle("active", autoPaused);
  $("auto-pause").textContent = autoPaused ? t("继续鞭挞") : t("暂停鞭挞");
  if (autoPaused) cancelAutoContinueTimer();
  // BUG 修复:恢复时如果正处于轮间空闲,必须重新调度,否则鞭挞静默死亡。
  if (!autoPaused && !running && $("auto-continue").checked && autoContinueAllowed()) {
    setStatus(`${t("鞭挞恢复")},2 ${t("秒后继续")}…`, false);
    scheduleAutoContinue();
  }
  log(autoPaused ? t("鞭挞已暂停") : t("鞭挞已恢复"));
});
$("auto-stop-round").addEventListener("change", () => {
  autoStopAfterRound = $("auto-stop-round").checked;
  log(autoStopAfterRound ? t("本轮结束后将停止鞭挞") : t("已取消本轮后停"));
});
$("auto-max").addEventListener("change", () => {
  const max = autoContinueMax();
  $("auto-max").value = max;
  localStorage.setItem("kz-auto-max", String(max));
  renderAutoStatus();
  autoRounds = 0;
  cancelAutoContinueTimer();
  log(`${t("鞭挞上限已设为")} ${max} ${t("轮")}`);
});
$("auto-continue").addEventListener("change", () => {
  if ($("auto-continue").checked && !autoContinueAllowed()) {
    $("auto-continue").checked = false;
    localStorage.setItem("kz-auto-continue", "0");
    autoRounds = 0;
    cancelAutoContinueTimer();
    toast(t("鞭挞仅适用于自主推进模式，请先切换模式"));
    log(t("鞭挞未开启:结伴开发模式不支持自动续跑"));
    return;
  }
  localStorage.setItem("kz-auto-continue", $("auto-continue").checked ? "1" : "0");
  autoRounds = 0;
  if (!$('auto-continue').checked) cancelAutoContinueTimer();
  log($("auto-continue").checked ? `${t("鞭挞已开启:每轮结束自动推进目标")} (${t("轮")} ${autoContinueMax()})` : t("鞭挞已关闭"));
  // BUG 修复(触发):空闲时勾上鞭挞必须立刻抽第一鞭——原来只挂在"上一轮结束"上,
  // 冷启动勾选后永远没有第一轮,必须手点"继续"才动。
  if ($("auto-continue").checked && !running && !autoPaused) {
    setStatus("鞭挞启动,2 秒后开始…", false);
    scheduleAutoContinue();
  }
});
const PROFILE_STORAGE_KEY = "kz-profile";
const savedProfile = localStorage.getItem(PROFILE_STORAGE_KEY);
if (["dev-pair", "dev-auto", "research"].includes(savedProfile)) {
  $("profile-select").value = savedProfile;
}
// 后端只认 dev/research(决定 agent 选择),dev-auto 是前端的鞭挞档位,按进程单独记住,
// 否则切换进程回显时自主推进会被静默降级成结伴开发。
// R-115:这份映射必须落盘。早期只放在内存里,重启后它是空的,回退分支就把模式
// 降级成结伴开发——哪怕 kz-profile 里明明存着自主推进(D-155)。
const PROCESS_PROFILE_KEY = "kz-process-profile";
const processProfileUi = new Map(
  Object.entries(readJson(PROCESS_PROFILE_KEY, {})).filter(([, v]) =>
    ["dev-pair", "dev-auto", "research"].includes(v),
  ),
);
function persistProcessProfiles() {
  writeJson(PROCESS_PROFILE_KEY, Object.fromEntries(processProfileUi));
}
// localStorage 里的 JSON 可能被手改坏;读不出来就当没有,绝不让偏好读取抛异常
// 把整个初始化带崩。
function readJson(key, fallback) {
  try {
    const parsed = JSON.parse(localStorage.getItem(key) || "null");
    return parsed && typeof parsed === "object" ? parsed : fallback;
  } catch {
    return fallback;
  }
}
function writeJson(key, value) {
  try {
    localStorage.setItem(key, JSON.stringify(value));
  } catch {
    /* 配额满等情况:偏好丢失可以接受,不该打断当前操作 */
  }
}
function syncAutoContinueWithProfile() {
  if (autoContinueAllowed() || !$("auto-continue").checked) return;
  $("auto-continue").checked = false;
  localStorage.setItem("kz-auto-continue", "0");
  autoRounds = 0;
  cancelAutoContinueTimer();
  renderAutoStatus();
  log(t("当前模式不支持鞭挞，已自动关闭"));
  toast(t("鞭挞已关闭：当前进程不是自主推进模式"));
}
function applyProfileValue(backendProfile) {
  const remembered = activeProcessId ? processProfileUi.get(activeProcessId) : null;
  // 回退顺序:本进程的记忆 → 全局上次选择 → dev-pair。少了中间这一档,
  // 新进程与重启后的旧进程都会被静默降级成结伴开发。
  const globalChoice = localStorage.getItem(PROFILE_STORAGE_KEY);
  const fallback = ["dev-pair", "dev-auto"].includes(globalChoice) ? globalChoice : "dev-pair";
  if (backendProfile === "research") $("profile-select").value = "research";
  else $("profile-select").value = remembered && remembered !== "research" ? remembered : fallback;
  localStorage.setItem(PROFILE_STORAGE_KEY, $("profile-select").value);
  syncAutoContinueWithProfile();
}
$("profile-select").addEventListener("change", () => {
  localStorage.setItem(PROFILE_STORAGE_KEY, $("profile-select").value);
  if (activeProcessId) {
    processProfileUi.set(activeProcessId, $("profile-select").value);
    persistProcessProfiles();
    const profile = $("profile-select").value === "research" ? "research" : "dev";
    invoke("process_update", { processId: activeProcessId, profile })
      .catch((error) => reportPersistentError(`${t("进程模式保存失败")}:${error}`));
  }
  syncAutoContinueWithProfile();
});
$("work-priority-select").addEventListener("change", async () => {
  const value = selectedWorkPriority();
  localStorage.setItem(workPriorityStorageKey(), value);
  if (!currentProject) return;
  try {
    // 切换 = 写记忆(真源),不是只改本地开关;记忆页随后可把正文改成任意细度。
    workFocusMemory = await invoke("memory_focus_set", {
      projectDir: currentProject,
      title: WORK_FOCUS_PRESETS[value].title,
      body: WORK_FOCUS_PRESETS[value].body,
    });
    log(localizeDynamic(value === "requirement-first" ? "已切换为需求优先" : "已切换为缺陷优先"));
  } catch (err) {
    toastError(`${t("开发重心保存失败")}:${err}`);
  }
});
$("stop").addEventListener("click", () => {
  // 本地立即复位,不依赖后端事件回执(事件通道故障时停止键也必须有效)。
  cancelAutoContinueTimer();
  autoRounds = 0;
  invoke("stop_run", { projectDir: currentProject, processId: activeProcessId }).catch((err) => reportPersistentError(`停止指令失败:${err}`));
  hideAsk();
  stopElapsed();
  setRunning(false, "已停止");
  log(t("已请求停止(本地已复位)"));
});
promptBox.addEventListener("keydown", (e) => {
  if (e.key === "Escape" && !$("file-suggestions").classList.contains("hidden")) {
    e.preventDefault();
    hideFileSuggestions();
    return;
  }
  if ((e.key === "Tab" || e.key === "Enter") && fileSuggestions.length > 0 && !e.ctrlKey && !e.metaKey) {
    e.preventDefault();
    chooseFileSuggestion();
    return;
  }
  if (e.key === "ArrowDown" && (promptBox.selectionStart === promptBox.value.length || promptBox.value === "")) {
    if (fileSuggestions.length > 0) {
      e.preventDefault();
      fileSuggestionIndex = (fileSuggestionIndex + 1) % fileSuggestions.length;
      renderFileSuggestions();
      return;
    }
    if (navigatePromptHistory(1)) e.preventDefault();
  } else if (e.key === "ArrowUp" && (promptBox.selectionStart === 0 || promptBox.value === "")) {
    if (fileSuggestions.length > 0) {
      e.preventDefault();
      fileSuggestionIndex = (fileSuggestionIndex - 1 + fileSuggestions.length) % fileSuggestions.length;
      renderFileSuggestions();
      return;
    }
    if (navigatePromptHistory(-1)) e.preventDefault();
  } else if (e.key === "Enter" && (e.ctrlKey || e.metaKey)) {
    e.preventDefault();
    send();
  } else if (e.key === "Enter" && !e.shiftKey) {
    e.preventDefault();
    send();
  }
});

window.addEventListener("keydown", (e) => {
  const modifier = e.ctrlKey || e.metaKey;
  if (!modifier || e.altKey) return;
  if (e.key.toLowerCase() === "k") {
    e.preventDefault();
    promptBox.focus();
    return;
  }
  if (!e.shiftKey) return;
  if (e.key.toLowerCase() === "c") {
    e.preventDefault();
    $("stop").click();
  } else if (e.key.toLowerCase() === "n") {
    e.preventDefault();
    $("new-chat").click();
  }
});

// ---------- 模型直选 ----------
async function loadModels() {
  const select = $("model-select");
  const saved = localStorage.getItem(prefKey("model")) ?? localStorage.getItem("kz-model") ?? "";
  select.innerHTML = "";
  const def = document.createElement("option");
  def.value = "";
  def.textContent = t("模型:agent 默认");
  select.appendChild(def);
  try {
    const models = await invoke("models_list", { projectDir: currentProject });
    const ids = new Set(models.map((m) => m.id));
    for (const m of models) {
      const opt = document.createElement("option");
      opt.value = m.id;
      opt.textContent = m.label;
      if (m.id === saved) opt.selected = true;
      select.appendChild(opt);
    }
    // D-167:探测不到不等于用不了——端点可能没实现 /models,key 也可能还没配好。
    // 手填过的模型要留在列表里,否则下次重开又得再填一遍。
    for (const id of manualModels()) {
      if (ids.has(id)) continue;
      const opt = document.createElement("option");
      opt.value = id;
      opt.textContent = `${id}(手填)`;
      if (id === saved) opt.selected = true;
      select.appendChild(opt);
    }
    const custom = document.createElement("option");
    custom.value = MANUAL_MODEL_SENTINEL;
    custom.textContent = t("＋ 手填模型…");
    select.appendChild(custom);
    log(`模型列表已刷新(${models.length} 个可选)`);
  } catch (err) {
    reportPersistentError(`模型列表获取失败:${err}`);
  }
}

// 手填模型:provider:model 直指。有些 OpenAI 兼容端点不提供 /models,
// 或者 key 尚未配好导致探测为空,这条通道保证配了 provider 就一定能用。
const MANUAL_MODEL_SENTINEL = "__manual__";
function manualModels() {
  const list = readJson(prefKey("manual-models"), []);
  return Array.isArray(list) ? list.filter((x) => typeof x === "string") : [];
}
function addManualModel(id) {
  const list = manualModels();
  if (!list.includes(id)) list.push(id);
  writeJson(prefKey("manual-models"), list);
}
// R-115:模型与思考强度按项目记——不同项目常配不同模型,共用一个全局键会互相打架。
// 思考强度此前只写不读(kz-reasoning 全仓零处 getItem),等于每次重启都回默认档。
function prefKey(name) {
  return `kz-${name}:${currentProject || "default"}`;
}
function restoreProjectPrefs() {
  const reasoning = localStorage.getItem(prefKey("reasoning"));
  const select = $("reasoning-select");
  // 选项不存在时不要硬塞:赋一个无效值会让 select 落到空串,反而清掉配置默认档。
  if (reasoning !== null && [...select.options].some((o) => o.value === reasoning)) {
    select.value = reasoning;
  }
  const delivery = localStorage.getItem("kz-delivery");
  const deliverySelect = $("delivery-select");
  if (delivery && [...deliverySelect.options].some((o) => o.value === delivery)) {
    deliverySelect.value = delivery;
  }
  restoreDocFilters();
}

// 思考强度:空值=用配置默认档,其余为本进程覆盖。
$("reasoning-select").addEventListener("change", () => {
  const value = $("reasoning-select").value;
  localStorage.setItem(prefKey("reasoning"), value);
  if (activeProcessId) {
    invoke("process_update", { processId: activeProcessId, reasoning: value })
      .catch((error) => reportPersistentError(`${t("进程思考强度保存失败")}:${error}`));
  }
});

$("model-select").addEventListener("change", () => {
  const select = $("model-select");
  if (select.value === MANUAL_MODEL_SENTINEL) {
    const input = (window.prompt(t("填 provider:model,例如 deepseek:deepseek-chat")) || "").trim();
    // provider 名必须对得上配置里的键,否则后端 resolve_model 会直接失败。
    if (!/^[\w.-]+:.+$/.test(input)) {
      if (input) toast(t("格式应为 provider:model"));
      select.value = localStorage.getItem(prefKey("model")) || "";
      return;
    }
    addManualModel(input);
    localStorage.setItem(prefKey("model"), input);
    loadModels().then(() => {
      $("model-select").value = input;
    });
    if (activeProcessId) {
      invoke("process_update", { processId: activeProcessId, model: input })
        .catch((error) => reportPersistentError(`${t("进程模型保存失败")}:${error}`));
    }
    return;
  }
  localStorage.setItem(prefKey("model"), select.value);
  if (activeProcessId) {
    // 空串=清除本进程的模型覆盖(回落 agent 默认);传 null 会被后端当作"不修改"。
    invoke("process_update", { processId: activeProcessId, model: $("model-select").value })
      .catch((error) => reportPersistentError(`${t("进程模型保存失败")}:${error}`));
  }
});

// ---------- 队列输入 ----------
function renderPendingInputs(items) {
  const list = $("queue-list");
  const count = $("queue-count");
  // 排队条挂在 composer(用户定调:排队输入放到排队按钮那里),空队列整条隐藏。
  $("composer-queue")?.classList.toggle("hidden", !items.length);
  list.innerHTML = "";
  count.textContent = items.length ? `(${items.length})` : "";
  if (!items.length) {
    const empty = document.createElement("div");
    empty.className = "queue-empty";
    empty.textContent = t("暂无排队输入");
    list.appendChild(empty);
    return;
  }
  for (const item of items) {
    const entry = document.createElement("div");
    entry.className = "queue-entry";
    entry.title = item.prompt;
    const prompt = document.createElement("div");
    prompt.className = "queue-prompt";
    prompt.textContent = item.prompt;
    const delivery = document.createElement("span");
    delivery.className = "queue-delivery";
    delivery.textContent = item.delivery === "steer" ? "steer" : "queue";
    const cancel = document.createElement("button");
    cancel.className = "queue-cancel";
    cancel.textContent = t("撤销");
    cancel.title = t("撤销这条排队输入");
    cancel.addEventListener("click", async () => {
      cancel.disabled = true;
      try {
        const changed = await invoke("cancel_input", {
          projectDir: currentProject,
          inputId: item.input_id,
          processId: activeProcessId,
        });
        if (changed) {
          toast(t("已撤销排队输入"));
          await refreshPendingInputs();
        }
      } catch (err) {
        cancel.disabled = false;
        toastError(`撤销失败:${err}`);
      }
    });
    entry.append(prompt, delivery, cancel);
    list.appendChild(entry);
  }
}

async function refreshPendingInputs() {
  if (!currentProject) {
    renderPendingInputs([]);
    return;
  }
  try {
    renderPendingInputs(await invoke("list_pending_inputs", {
      projectDir: currentProject,
      processId: activeProcessId,
    }));
  } catch (err) {
    log(`队列刷新失败:${err}`, "warn");
  }
}

function renderTestRuns(snapshot) {
  const list = $("test-list");
  const records = [...(snapshot?.active ?? []), ...(snapshot?.archived ?? [])];
  list.replaceChildren();
  $("test-count").textContent = `${records.length}`;
  if (!records.length) {
    const empty = document.createElement("div");
    empty.className = "doc-empty";
    empty.textContent = t("暂无测试记录");
    list.appendChild(empty);
    return;
  }
  for (const record of records.slice().reverse()) {
    const row = document.createElement("div");
    row.className = `test-entry test-${record.status}`;
    row.textContent = `${record.status === "passed" ? "✓" : record.status === "failed" ? "×" : record.status === "running" ? "●" : "○"} ${record.id} ${record.title}`;
    row.title = (record.fields ?? []).map((field) => `${field.key}: ${field.value}`).join("\n");
    list.appendChild(row);
  }
}

async function refreshTests() {
  if (!currentProject) {
    renderTestRuns({ active: [], archived: [] });
    return;
  }
  try {
    renderTestRuns(await invoke("test_runs_snapshot", { projectDir: currentProject }));
  } catch (error) {
    log(`测试记录刷新失败:${error}`, "warn");
  }
}

$("tests-refresh").addEventListener("click", refreshTests);

let worktreeItems = [];
function renderWorktrees(items) {
  worktreeItems = items ?? [];
  const list = $("worktree-list");
  list.replaceChildren();
  if (!worktreeItems.length) {
    const empty = document.createElement("div");
    empty.className = "doc-empty";
    empty.textContent = t("暂无隔离工作树");
    list.appendChild(empty);
    return;
  }
  for (const item of worktreeItems) {
    const row = document.createElement("div");
    row.className = "worktree-entry";
    const label = document.createElement("div");
    label.textContent = `${item.branch} · ${item.clean ? t("干净") : `${item.files.length} ${t("项改动")}`}`;
    label.title = item.path;
    const actions = document.createElement("div");
    for (const [text, action] of [[t("差异"), "diff"], [t("合并"), "merge"], [t("放弃"), "discard"]]) {
      const button = document.createElement("button");
      button.className = `ghost mini ${action === "merge" ? "worktree-merge" : ""}`;
      button.textContent = text;
      button.addEventListener("click", () => handleWorktreeAction(item, action));
      actions.appendChild(button);
    }
    row.append(label, actions);
    list.appendChild(row);
  }
}
async function refreshWorktrees() {
  if (!currentProject) return renderWorktrees([]);
  const saved = JSON.parse(localStorage.getItem(`kz-worktrees:${currentProject}`) || "[]");
  const live = [];
  for (const path of saved) {
    try { live.push(await invoke("worktree_diff", { projectDir: currentProject, worktreePath: path })); }
    catch (error) { log(`工作树已不可用:${path} · ${error}`, "warn"); }
  }
  renderWorktrees(live);
}
async function handleWorktreeAction(item, action) {
  try {
    if (action === "diff") {
      if (item.clean) {
        toast(t("工作树干净,没有未提交差异"));
      } else {
        const file_list = item.files.join("\n");
        const diff = item.diff?.trim() || t("未跟踪文件尚未包含在 git diff 中");
        log(`${item.branch}\n${t("文件列表")}:\n${file_list}\n\n${t("实际差异")}:\n${diff}`, "info");
        $("log-panel").classList.remove("hidden");
        toast(t("工作树差异已写入运行日志"));
      }
      return;
    }
    if (action === "discard" && !window.confirm(`${t("放弃工作树")} ${item.branch}？${t("未提交改动会阻止删除并保留现场")}`)) return;
    const command = action === "merge" ? "worktree_merge" : "worktree_discard";
    const result = await invoke(command, { projectDir: currentProject, worktreePath: item.path });
    if (String(result).length > 160) {
      log(String(result), "info");
      $("log-panel").classList.remove("hidden");
      toast(t("工作树操作完成，详细结果已写入运行日志"));
    } else {
      toast(result);
    }
    if (action === "discard") {
      const paths = JSON.parse(localStorage.getItem(`kz-worktrees:${currentProject}`) || "[]").filter((path) => path !== item.path);
      localStorage.setItem(`kz-worktrees:${currentProject}`, JSON.stringify(paths));
    }
    await refreshWorktrees();
    refreshGit();
  } catch (error) {
    toastError(String(error), { retry: () => handleWorktreeAction(item, action) });
  }
}("click", refreshWorktrees);
$("worktree-add").addEventListener("click", async () => {
  if (!currentProject) return;
  const name = `thread-${new Date().toISOString().replace(/[-:TZ.]/g, "").slice(0, 14)}`;
  try {
    const item = await invoke("worktree_create", { projectDir: currentProject, name });
    const paths = JSON.parse(localStorage.getItem(`kz-worktrees:${currentProject}`) || "[]");
    paths.push(item.path);
    localStorage.setItem(`kz-worktrees:${currentProject}`, JSON.stringify(paths));
    toast(`${t("隔离工作树已创建")}:${item.path}`);
    await refreshWorktrees();
  } catch (error) {
    toastError(`创建工作树失败:${error}`);
  }
});

// ---------- R-030:项目内独立进程 ----------
let syncedRunningProcessId = null;
function renderProcesses(items) {
  processItems = items ?? [];
  if (!activeProcessId || !processItems.some((item) => item.id === activeProcessId)) {
    const preferred = processItems.find((item) => item.id.startsWith("d|")) || processItems[0];
    activeProcessId = preferred?.id ?? null;
  }
  const active = processItems.find((item) => item.id === activeProcessId);
  activeSessionId = active?.session_id ?? null;
  pumpAsk();
  // 活动进程换人时按后端真实状态重算运行态(切项目/进程后旧会话的 kz:done 收不到)。
  // 只在身份变化时同步,避免与"停止"按钮的本地即时复位互相打架。
  if (activeProcessId !== syncedRunningProcessId) {
    syncedRunningProcessId = activeProcessId;
    setRunning(Boolean(active?.running), active?.running ? t("运行中") : t("空闲"));
  }
  const tabs = $("process-tabs");
  tabs.replaceChildren();
  for (const item of processItems) {
    const tab = document.createElement("button");
    tab.type = "button";
    tab.className = `process-tab${item.id === activeProcessId ? " active" : ""}${item.running ? " running" : ""}`;
    tab.textContent = `${item.label}${item.running ? " ●" : ""}`;
    tab.title = `${item.id}${item.model ? ` · ${item.model}` : ""}`;
    tab.addEventListener("click", () => switchProcess(item.id));
    tabs.appendChild(tab);
  }
  $("process-subagent").checked = active?.subagent ?? true;
}

async function refreshProcesses() {
  if (!currentProject) return;
  try {
    renderProcesses(await invoke("process_list", { projectDir: currentProject }));
  } catch (err) {
    log(`${t("进程列表刷新失败")}:${err}`, "warn");
  }
}

async function refreshPendingAsks() {
  if (!currentProject || !activeSessionId) return;
  try {
    const pending = await invoke("pending_asks_get", {
      projectDir: currentProject,
      processId: activeProcessId,
    });
    const queue = askQueueFor(activeSessionId);
    const known = new Set(queue.map((item) => item.id));
    if (askActive?.sessionId === activeSessionId) known.add(askActive.id);
    for (const payload of pending || []) {
      if (!known.has(payload.id)) {
        queue.push(payload);
        known.add(payload.id);
      }
    }
    pumpAsk();
  } catch (err) {
    log(`${t("待处理权限询问恢复失败")}:${err}`, "warn");
  }
}


async function switchProcess(processId) {
  if (processId === activeProcessId) return;
  const target = processItems.find((item) => item.id === processId);
  if (!target) return;
  // 后端只保存 dev/research；切换前先把前端的 dev-auto 档位绑定到旧进程，
  // 这样回切时不会因后端 profile=dev 而退回 dev-pair。
  if (activeProcessId) processProfileUi.set(activeProcessId, $("profile-select").value);
  hideAsk(true);
  activeProcessId = processId;
  activeSessionId = target.session_id;
  pumpAsk();
  setRunning(target.running, target.running ? t("运行中") : t("空闲"));
  renderProcesses(processItems);
  clearChat();
  bgClear();
  renderTodoPanel([], 0, 0);
  await loadConversation();
  await refreshPendingAsks();
  await refreshDocs();
  await loadModels();
  // 模型下拉按进程回显:未设置覆盖时回到 agent 默认(空值),不保留上一个进程的选择。
  $("model-select").value = target.model || "";
  if (target.profile) applyProfileValue(target.profile);
  refreshGit();
  refreshPendingInputs();
  refreshProcesses();
  log(`${t("已切换到进程")} ${target.label}`);
}

$("process-add").addEventListener("click", async () => {
  if (!currentProject) return;
  try {
    const item = await invoke("process_create", { projectDir: currentProject, subagent: true });
    await refreshProcesses();
    await switchProcess(item.id);
  } catch (err) {
    toastError(`${t("创建进程失败")}:${err}`);
  }
});

$("process-subagent").addEventListener("change", async (event) => {
  if (!activeProcessId) return;
  try {
    await invoke("process_update", { processId: activeProcessId, subagent: event.target.checked });
    await refreshProcesses();
  } catch (err) {
    event.target.checked = !event.target.checked;
    toastError(`${t("更新进程能力失败")}:${err}`);
  }
});

// ---------- 项目管理 ----------
function baseName(path) {
  const parts = path.replaceAll("\\", "/").split("/").filter(Boolean);
  return parts[parts.length - 1] || path;
}

function syncDocumentsProjectSelect(prefs) {
  const select = $("documents-project-select");
  if (!select) return;
  select.replaceChildren();
  for (const path of prefs.projects ?? []) {
    select.appendChild(new Option(prefs.names?.[path] || baseName(path), path));
  }
  select.value = prefs.current ?? "";
  select.disabled = !(prefs.projects ?? []).length;
}

function renderProjects(prefs) {
  const previousProject = currentProject;
  currentProject = prefs.current;
  syncWorkPriorityControl();
  // R-115:按项目记的偏好(模型/思考强度/筛选)要跟着项目切换回填,
  // 也覆盖了启动这一次——currentProject 在这里才第一次确定。
  restoreProjectPrefs();
  if (previousProject !== currentProject) {
    activeProcessId = null;
    activeSessionId = null;
  }
  const list = $("project-list");
  list.innerHTML = "";
  for (const path of prefs.projects) {
    const item = document.createElement("div");
    item.className = `project-item${path === prefs.current ? " active" : ""}`;
    item.setAttribute("role", "button");
    item.tabIndex = 0;
    item.setAttribute("aria-label", `${t("选择项目")} ${prefs.names?.[path] || baseName(path)}`);
    const name = document.createElement("span");
    name.className = "name";
    name.textContent = prefs.names?.[path] || baseName(path);
    const pathEl = document.createElement("span");
    pathEl.className = "path";
    pathEl.textContent = path;
    const remove = document.createElement("button");
    remove.className = "icon-btn remove";
    remove.textContent = "×";
    remove.title = t("移除(不删除文件)");
    remove.setAttribute("aria-label", `${t("移除项目")} ${name.textContent}`);
    remove.addEventListener("click", async (e) => {
      e.stopPropagation();
      if (!window.confirm(`${t("移除项目")}“${name.textContent}”吗？${t("只解除登记,不会删除磁盘文件。")}`)) return;
      try {
        const wasCurrent = currentProject === path;
        const next = await invoke("projects_remove", { path });
        renderProjects(next);
        if (wasCurrent && currentProject !== path) {
          clearChat();
          bgClear();
          renderTodoPanel([], 0, 0);
          await loadConversation();
          await refreshDocs();
          await loadModels();
          refreshGit();
          await refreshPendingInputs();
        }
      } catch (err) {
        toastError(String(err));
      }
    });
    const rename = document.createElement("button");
    rename.className = "icon-btn rename";
    rename.textContent = "✎";
    rename.title = t("重命名项目(只修改显示名)");
    rename.setAttribute("aria-label", `${t("重命名项目")} ${name.textContent}`);
    rename.addEventListener("click", async (e) => {
      e.stopPropagation();
      const nextName = window.prompt(t("项目显示名"), prefs.names?.[path] || baseName(path));
      if (nextName === null || !nextName.trim()) return;
      try {
        renderProjects(await invoke("projects_rename", { path, name: nextName.trim() }));
      } catch (err) {
        toastError(String(err));
      }
    });
    item.append(name, pathEl, rename, remove);
    item.addEventListener("keydown", (e) => {
      if (e.key !== "Enter" && e.key !== " ") return;
      e.preventDefault();
      item.click();
    });
    item.addEventListener("click", async () => {
      const previous = currentProject;
      renderProjects(await invoke("projects_select", { path }));
      if (previous && previous !== path) {
        // 运行状态属于会话:切项目后必须按目标项目重算,否则旧项目的 kz:done 被会话过滤器
        // 丢弃,新项目会永久卡在"运行中"(发送键禁用)。refreshProcesses 会带回真实状态。
        setRunning(false, "空闲");
        clearChat();
        bgClear();
        renderTodoPanel([], 0, 0);
        await loadConversation();
      }
      refreshDocs();
      loadModels();
      refreshGit();
      refreshPendingInputs();
    });
    list.appendChild(item);
  }
  $("project-label").textContent = prefs.current ?? `(${localizeDynamic("未选择项目")})`;
  syncDocumentsProjectSelect(prefs);
  refreshProcesses();
}

$("project-init").addEventListener("click", async () => {
  const path = window.prompt(t("新项目目录路径(不存在时会创建)"));
  if (path === null || !path.trim()) return;
  const name = window.prompt(t("项目显示名(可留空)"), baseName(path.trim()));
  if (name === null) return;
  try {
    const prefs = await invoke("projects_init", {
      path: path.trim(),
      name: name.trim() || null,
    });
    renderProjects(prefs);
    clearChat(t("已初始化并切换到新项目"));
    await loadConversation();
    await refreshDocs();
    await loadModels();
    refreshGit();
    await refreshPendingInputs();
    toast(t("项目初始化完成"));
  } catch (err) {
    toastError(String(err));
  }
});

$("project-add").addEventListener("click", async () => {
  try {
    const prefs = await invoke("projects_pick");
    if (prefs) {
      const previous = currentProject;
      renderProjects(prefs);
      if (previous !== currentProject) {
        clearChat();
        bgClear();
        renderTodoPanel([], 0, 0);
        await loadConversation();
        await refreshDocs();
        await loadModels();
        refreshGit();
        await refreshPendingInputs();
      } else {
        await refreshDocs();
      }
    }
  } catch (err) {
    toastError(String(err));
  }
});

// ---------- 侧边栏文档(可展开 + 状态流转) ----------
const reqFilters = { status: "all", priority: "all", complexity: "all", tag: "all", blocked: "all", sort: localStorage.getItem("kz-req-sort") || "manual", grouped: localStorage.getItem("kz-grouped-req") !== "0" };
const defectFilters = { status: "all", priority: "all", tag: "all", blocked: "all", grouped: localStorage.getItem("kz-grouped-defect") !== "0" };

// R-115:筛选条件按项目持久化。此前只有 sort 与 grouped 落盘,状态/优先级/复杂度/
// 标签/阻塞五项每次重启都回"全部"——盯着某一类条目做事时,重开一次全白设。
// 只回填已知字段:localStorage 里的旧结构或手改内容不该污染筛选状态。
const FILTER_FIELDS = {
  req: ["status", "priority", "complexity", "tag", "blocked", "sort"],
  defect: ["status", "priority", "tag", "blocked"],
};
function saveDocFilters() {
  const pick = (state, fields) => Object.fromEntries(fields.map((f) => [f, state[f]]));
  writeJson(prefKey("filters"), {
    req: pick(reqFilters, FILTER_FIELDS.req),
    defect: pick(defectFilters, FILTER_FIELDS.defect),
    docReq: pick(documentFilters.req, FILTER_FIELDS.req),
    docDefect: pick(documentFilters.defect, FILTER_FIELDS.defect),
  });
}
function restoreDocFilters() {
  const saved = readJson(prefKey("filters"), {});
  const apply = (state, data, fields) => {
    if (!data || typeof data !== "object") return;
    for (const field of fields) {
      if (typeof data[field] === "string") state[field] = data[field];
    }
  };
  apply(reqFilters, saved.req, FILTER_FIELDS.req);
  apply(defectFilters, saved.defect, FILTER_FIELDS.defect);
  apply(documentFilters.req, saved.docReq, FILTER_FIELDS.req);
  apply(documentFilters.defect, saved.docDefect, FILTER_FIELDS.defect);
  syncDocFilterControls();
}
// 状态回填到控件上,否则下拉显示"全部"而实际在筛选,看起来就像列表丢了条目。
function syncDocFilterControls() {
  const pairs = [
    ["req-status-filter", reqFilters.status],
    ["req-complexity-filter", reqFilters.complexity],
    ["req-priority-filter", reqFilters.priority],
    ["req-blocked-filter", reqFilters.blocked],
    ["req-sort", reqFilters.sort],
    ["defect-blocked-filter", defectFilters.blocked],
  ];
  for (const [id, value] of pairs) {
    const el = $(id);
    if (el && [...el.options].some((o) => o.value === value)) el.value = value;
  }
}
// 标签受控词表(conventions §1.35,用户定调):分组顺序即展示顺序。
const DOC_TAG_ORDER = ["核心", "后端", "前端", "模型", "发布", "流程"];
function docGroupTag(entry) {
  const tags = entryTags(entry);
  return DOC_TAG_ORDER.find((tag) => tags.includes(tag)) || "其他";
}
const priorityRank = { P0: 0, P1: 1, P2: 2, P3: 3 };
const statusRank = { doing: 0, todo: 1, done: 2, dropped: 3 };
const complexityRank = { "小": 0, "中": 1, "大": 2 };
function entryTags(entry) {
  const field = (entry.fields ?? []).find(([key]) => ["标签", "tags", "tag"].includes(String(key).toLowerCase()));
  return String(field?.[1] || "").split(/[\s,]+/).map((tag) => tag.trim()).filter(Boolean);
}
function tagOptions(entries) {
  // 受控词表优先,词表外的存量标签跟在后面(过渡期可见,便于归一)。
  const seen = new Set(entries.flatMap(entryTags));
  const extras = [...seen].filter((tag) => !DOC_TAG_ORDER.includes(tag)).sort((a, b) => a.localeCompare(b));
  return [...DOC_TAG_ORDER.filter((tag) => seen.has(tag)), ...extras];
}
// 返回实际生效的值:保存的标签在当前项目里可能根本不存在,那时下拉会回落成
// "全部",但**筛选状态必须跟着回落**——否则状态里还留着那个标签,列表被筛空,
// 而界面显示"没有筛选",看起来就是"条目凭空掉了"(D-169)。
function syncTagFilter(select, entries, selected = "all") {
  select.replaceChildren(new Option(localizeDynamic("全部标签"), "all"));
  for (const tag of tagOptions(entries)) select.appendChild(new Option(localizeDynamic(tag), tag));
  select.value = selectedOptions(select, selected);
  return select.value;
}
function selectedOptions(select, selected) {
  return [...select.options].some((option) => option.value === selected) ? selected : "all";
}

function entryBlocked(entry) {
  return Boolean(entry?.blocked);
}
function matchesBlockedFilter(entry, value) {
  return value === "all" || (value === "blocked" ? entryBlocked(entry) : !entryBlocked(entry));
}
function filterRequirements(entries, filters = reqFilters) {
  const filtered = entries
    .filter((entry) => filters.status === "all" || entry.status === filters.status)
    .filter((entry) => filters.priority === "all" || entry.priority === filters.priority)
    .filter((entry) => filters.tag === "all" || entryTags(entry).includes(filters.tag))
    .filter((entry) => matchesBlockedFilter(entry, filters.blocked ?? "all"));
  const complexityValue = (entry) => entry.complexity || "unassessed";
  const complexityFiltered = filtered.filter((entry) => filters.complexity === "all" || complexityValue(entry) === filters.complexity);
  // 手动模式(R-054 默认):文件顺序即开发顺序,不做任何排序。
  if (filters.sort === "manual") return complexityFiltered;
  return complexityFiltered.sort((a, b) => {
    if (filters.sort === "id") return a.id.localeCompare(b.id, undefined, { numeric: true });
    if (filters.sort === "complexity") return (complexityRank[complexityValue(a)] ?? 99) - (complexityRank[complexityValue(b)] ?? 99) || a.id.localeCompare(b.id, undefined, { numeric: true });
    if (filters.sort === "status") {
      return (statusRank[a.status] ?? 99) - (statusRank[b.status] ?? 99) || a.id.localeCompare(b.id, undefined, { numeric: true });
    }
    return (priorityRank[a.priority] ?? 99) - (priorityRank[b.priority] ?? 99)
      || (statusRank[a.status] ?? 99) - (statusRank[b.status] ?? 99)
      || a.id.localeCompare(b.id, undefined, { numeric: true });
  });
}


// R-054:拖拽重排(手动模式限定)。拖完提交完整 ID 序——注意 order 必须覆盖
// 全部条目,所以在有筛选时禁止拖拽(顺序不完整会被引擎拒绝)。
let dragReqId = null;
function reqDragEnabled(filters = reqFilters) {
  return filters.sort === "manual" && filters.status === "all" && filters.priority === "all" && filters.complexity === "all" && filters.tag === "all" && (filters.blocked ?? "all") === "all";
}
// R-123 职责分离:侧边栏 = 浏览与取活(只读详情 + 切状态),独立文档页 = 深度管理
// (排序、字段编辑、批量操作)。同一份 renderDocList 服务两处,靠这个判断分流。
function docSurface(listEl) {
  return String(listEl?.id ?? "").startsWith("documents-") ? "documents" : "sidebar";
}

// 批量操作选中集:id → kind。跨重绘保留,否则 agent 的一次刷新就把选择清空了。
const batchSelection = new Map();

function syncBatchBar() {
  const bar = $("documents-batch-bar");
  if (!bar) return;
  // 条目可能因筛选/归档而消失,选中集要随之收敛,不然会对不存在的条目发批量请求。
  const alive = new Set(
    [...document.querySelectorAll(".documents-list .doc-item[data-doc-id]")].map((n) => n.dataset.docId),
  );
  for (const id of [...batchSelection.keys()]) if (!alive.has(id)) batchSelection.delete(id);
  bar.classList.toggle("hidden", batchSelection.size === 0);
  $("documents-batch-count").textContent = `${t("已选")} ${batchSelection.size}`;
  // 状态选项按选中集的类型给:需求与缺陷状态机不同,混选时只允许改标签。
  const kinds = new Set(batchSelection.values());
  const statusSelect = $("documents-batch-status");
  const options = kinds.size === 1 ? documentStatusOptions[[...kinds][0]].slice(1) : [];
  statusSelect.innerHTML =
    `<option value="">${kinds.size > 1 ? t("混选类型,仅可改标签") : t("改状态…")}</option>` +
    options.map(([value, label]) => `<option value="${value}">${localizeDynamic(label)}</option>`).join("");
  statusSelect.disabled = kinds.size !== 1;
}

async function applyBatch() {
  const status = $("documents-batch-status").value;
  const tag = $("documents-batch-tag").value;
  if (!status && !tag) {
    toast(t("先选择要改的状态或标签"));
    return;
  }
  const targets = [...batchSelection.entries()];
  let ok = 0;
  const failures = [];
  for (const [id, kind] of targets) {
    try {
      await invoke("docs_update", {
        projectDir: currentProject,
        kind,
        action: "update",
        id,
        ...(status ? { status } : {}),
        ...(tag ? { fields: { "标签": tag } } : {}),
      });
      ok += 1;
    } catch (error) {
      // 逐条独立提交:一条失败(比如状态机不允许后退)不该把整批回滚掉,
      // 但必须逐条报出来,否则用户以为全成功了。
      failures.push(`${id}: ${error}`);
    }
  }
  batchSelection.clear();
  if (failures.length) toastError(`${t("批量操作部分失败")}(${ok}/${targets.length}):${failures.join(";")}`);
  else toast(`${t("批量操作完成")}:${ok}`);
  refreshDocs();
}

function docDragEnabled(kind, listEl, filterState) {
  // 拖拽改序属深度管理,只在独立文档页提供:侧栏因此不再承担改序,行也轻了。
  if (docSurface(listEl) !== "documents") return false;
  if (kind === "req") return reqDragEnabled(filterState);
  if (kind !== "defect") return false;
  return filterState.status === "all" && filterState.priority === "all";
}
async function commitDocOrder(listEl, kind) {
  const order = [...listEl.querySelectorAll(".doc-item[data-doc-id]")].map((el) => el.dataset.docId);
  try {
    const msg = await invoke("docs_update", {
      projectDir: currentProject,
      kind,
      action: "reorder",
      id: "",
      order,
    });
    log(msg);
    refreshDocs();
  } catch (err) {
    toastError(`排序保存失败:${err}`);
    refreshDocs();
  }
}

// 引用跳转。目标可能被筛选藏起来、在折叠分区里、在收起的侧栏里,或者已经归档——
// 旧实现只认当前可见节点(offsetParent !== null),这四种情况一律静默失败:点了没反应,
// 也没有任何提示,看起来就是"引用是死链"(D-166)。
function jumpToEntry(ref) {
  const matches = [...document.querySelectorAll("[data-doc-id]")].filter(
    (item) => item.dataset.docId === ref,
  );
  if (!matches.length) {
    toast(`${t("找不到")} ${ref}`);
    return;
  }
  // 同一条目可能同时存在于侧栏和独立文档页:优先跳当前视图里那个,都不可见就取第一个
  // 并把挡住它的容器逐层打开。
  const target = matches.find((item) => item.offsetParent) ?? matches[0];
  if (sidebarCollapsed && target.closest("#sidebar")) {
    sidebarCollapsed = false;
    localStorage.setItem("kz-sidebar-collapsed", "0");
    syncSidebar();
  }
  // 只掀开确实会藏住条目的两类容器,不对任意祖先去 hidden——那会顺手展开整个视图。
  for (let node = target; node; node = node.parentElement) {
    if (node.classList?.contains("doc-archive-list")) node.classList.remove("hidden");
    if (node.classList?.contains("sidebar-section")) node.classList.remove("collapsed");
  }
  target.scrollIntoView({ behavior: "smooth", block: "center" });
  target.classList.add("ref-highlight");
  setTimeout(() => target.classList.remove("ref-highlight"), 1200);
}

function renderDocList(el, entries, kind, archivedCount = 0, reqFilterState = reqFilters, archivedEntries = []) {
  const surface = docSurface(el);
  // 筛掉了多少条:用于"被筛空"时说清原因。列表凭空变空是最容易被当成数据丢失的
  // 一类现象,必须给出条数与一键清除,而不是留一片空白(D-169)。
  const totalBeforeFilter = entries.length;
  // 筛选一律在这里做,调用方不得再预筛一遍——两处口径必须同源,否则侧栏与文档页
  // 会在同一筛选条件下给出不同的条目集合(R-123 验收 ④)。
  if (kind === "req") entries = filterRequirements(entries, reqFilterState);
  if (kind === "defect") {
    entries = entries
      .filter((entry) => reqFilterState.status === "all" || entry.status === reqFilterState.status)
      .filter((entry) => reqFilterState.priority === "all" || entry.priority === reqFilterState.priority)
      .filter((entry) => reqFilterState.tag === "all" || entryTags(entry).includes(reqFilterState.tag))
      .filter((entry) => matchesBlockedFilter(entry, reqFilterState.blocked ?? "all"));
  }
  // 分组视图(用户定调):按受控词表分组展示;组内保持文件顺序。
  // 分组改变了视觉顺序≠文件顺序,拖拽在分组视图下必须禁用(否则会提交错乱顺序)。
  const groupHeaders = new Map();
  const isGrouped =
    (kind === "req" || kind === "defect") &&
    reqFilterState.grouped &&
    (reqFilterState.tag ?? "all") === "all";
  if (isGrouped) {
    const buckets = new Map();
    for (const entry of entries) {
      const tag = docGroupTag(entry);
      if (!buckets.has(tag)) buckets.set(tag, []);
      buckets.get(tag).push(entry);
    }
    const ordered = [];
    for (const tag of [...DOC_TAG_ORDER, "其他"]) {
      const bucket = buckets.get(tag);
      if (!bucket || !bucket.length) continue;
      groupHeaders.set(ordered.length, `${tag} · ${bucket.length}`);
      ordered.push(...bucket);
    }
    entries = ordered;
  }
  // 展开状态是 DOM 局部的,重绘会全部收起;运行中会频繁重绘,必须跨重绘保留,
  // 否则用户刚展开的条目会被 agent 的一次状态更新弹回去。
  const expandedIds = new Set(
    [...el.querySelectorAll(".doc-item[data-doc-id]")]
      .filter((item) => {
        const detail = item.querySelector(".doc-detail");
        return detail && !detail.classList.contains("hidden");
      })
      .map((item) => item.dataset.docId)
  );
  el.innerHTML = "";
  // 被筛空:必须说清"有多少条被藏起来了"并给一键清除。此前这种情况下如果还有
  // 归档条目,连"(空)"都不显示——纯一片空白,看起来就是需求全没了。
  if (entries.length === 0 && totalBeforeFilter > 0) {
    const hint = document.createElement("div");
    hint.className = "doc-empty doc-filtered-empty";
    hint.append(`${totalBeforeFilter} ${t("条被当前筛选隐藏")} · `);
    const clear = document.createElement("button");
    clear.type = "button";
    clear.className = "ghost mini";
    clear.textContent = t("清除筛选");
    clear.addEventListener("click", () => {
      for (const key of ["status", "priority", "complexity", "tag", "blocked"]) {
        if (key in reqFilterState) reqFilterState[key] = "all";
      }
      saveDocFilters();
      refreshDocs();
    });
    hint.appendChild(clear);
    el.appendChild(hint);
    return;
  }
  if (entries.length === 0 && archivedCount === 0) {
    const empty = document.createElement("div");
    empty.className = "doc-empty";
    empty.textContent = `(${t("空")})`;
    el.appendChild(empty);
    return;
  }
  let position = 0;
  for (const entry of entries) {
    if (groupHeaders.has(position)) {
      const head = document.createElement("div");
      head.className = "doc-group-head";
      const [groupTag, groupCount] = groupHeaders.get(position).split(" · ");
      head.textContent = `${localizeDynamic(groupTag)} · ${groupCount}`;
      el.appendChild(head);
    }
    position += 1;
    const item = document.createElement("div");
    // 优先级着色(pri-P0 红 / P1 黄 / P2 蓝 / P3 灰):扫一眼就知道轻重。
    const pri = (entry.priority || "").toUpperCase();
    // 阻塞状态由后端调度器计算,前端只负责展示,保证列表顺序与 agent 取活一致。
    const blockedReasons = Array.isArray(entry.block_reasons) ? entry.block_reasons : [];
    const blocked = entryBlocked(entry);
    const externalBlocked = (entry.fields ?? []).some(([key, value]) =>
      ["阻塞", "blocked", "blocking"].includes(String(key).toLowerCase())
      && /外部|external|blocked/i.test(String(value))
    );
    item.className = `doc-item${entry.closed ? " closed" : ""}${blocked ? " blocked" : ""}${externalBlocked ? " external-blocked" : ""}${/^P[0-3]$/.test(pri) ? ` pri-${pri}` : ""}`;
    item.dataset.docId = entry.id;

    const row = document.createElement("div");
    row.className = "doc-row";
    row.setAttribute("role", "button");
    row.tabIndex = 0;
    // 批量操作只在文档页:侧栏一行要尽量轻,多一个勾选框就多一层视觉噪音。
    if (surface === "documents" && (kind === "req" || kind === "defect") && !entry.closed) {
      const pick = document.createElement("input");
      pick.type = "checkbox";
      pick.className = "doc-pick";
      pick.checked = batchSelection.has(entry.id);
      pick.setAttribute("aria-label", `${t("选择")} ${entry.id}`);
      pick.addEventListener("click", (event) => event.stopPropagation());
      pick.addEventListener("change", () => {
        if (pick.checked) batchSelection.set(entry.id, kind);
        else batchSelection.delete(entry.id);
        syncBatchBar();
      });
      row.appendChild(pick);
    }
    row.setAttribute("aria-label", `${entry.id} ${entry.title}，${t("按 Enter 展开详情")}`);
    row.title = `${entry.id} ${entry.title}(${t("点击展开")})`;
    const id = document.createElement("span");
    id.className = "id";
    // R-054(用户拍板):需求行内不显示 R-xxx(乱序观感),只显示位置序号;身份收进展开详情。
    if (kind === "req") {
      id.className = "pos";
      id.textContent = `#${position}`;
    } else {
      id.textContent = entry.id;
    }
    const st = document.createElement("span");
    st.className = `st st-${entry.status || "todo"}`;
    st.textContent = localizedDocStatus(entry.status || "todo") + (entry.severity ? `/${entry.severity}` : "");
    // 复杂度(R-051):侧栏用三格电量图标表达体量，与左侧优先级色带同色并放在最前面。
    const cx = (entry.complexity || "").trim();
    if (["小", "中", "大"].includes(cx)) {
      item.classList.add(`cx-${cx === "小" ? "s" : cx === "中" ? "m" : "l"}`);
      row.title = `${row.title} · ${t("复杂度")}:${t(cx)}`;
    }
    if (kind === "req" && el.id === "req-list") {
      const levels = { "小": 1, "中": 2, "大": 3 };
      const meter = document.createElement("span");
      const level = levels[cx] || 0;
      meter.className = `complexity-meter complexity-level-${level}`;
      meter.setAttribute("role", "img");
      meter.setAttribute("aria-label", `${t("复杂度")}:${cx ? t(cx) : t("未评估")}`);
      meter.title = `${t("复杂度")}:${cx ? t(cx) : t("未评估")}`;
      for (let i = 1; i <= 3; i += 1) {
        const cell = document.createElement("span");
        cell.className = `complexity-cell${i <= level ? " filled" : ""}`;
        cell.setAttribute("aria-hidden", "true");
        meter.appendChild(cell);
      }
      row.appendChild(meter);
    }
    if (blocked || externalBlocked) {
      const blockedBadge = document.createElement("span");
      blockedBadge.className = "blocked-badge";
      blockedBadge.textContent = t("阻塞");
      blockedBadge.title = blockedReasons.length ? blockedReasons.join("；") : t("阻塞原因");
      row.appendChild(blockedBadge);
    }
    // 拖拽重排:需求仅手动且无筛选；缺陷仅完整列表，避免提交不完整顺序。
    // 分组视图下禁用(视觉顺序≠文件顺序);关掉分组开关即恢复拖拽。
    // 松手落在行间隙时 drop 不触发,只靠 drop 会静默丢单。
    if (!isGrouped && docDragEnabled(kind, el, reqFilterState)) {
      item.draggable = true;
      item.addEventListener("dragstart", (e) => {
        dragReqId = entry.id;
        item.classList.add("dragging");
        el.dataset.orderBefore = [...el.querySelectorAll(".doc-item[data-doc-id]")]
          .map((n) => n.dataset.docId)
          .join(",");
        e.dataTransfer.effectAllowed = "move";
        e.dataTransfer.setData("text/plain", entry.id);
      });
      item.addEventListener("dragend", () => {
        item.classList.remove("dragging");
        dragReqId = null;
        const now = [...el.querySelectorAll(".doc-item[data-doc-id]")]
          .map((n) => n.dataset.docId)
          .join(",");
        if (now !== el.dataset.orderBefore) commitDocOrder(el, kind);
      });
      item.addEventListener("dragover", (e) => {
        e.preventDefault();
        const dragging = el.querySelector(".doc-item.dragging");
        if (!dragging || dragging === item) return;
        const rect = item.getBoundingClientRect();
        const before = e.clientY < rect.top + rect.height / 2;
        el.insertBefore(dragging, before ? item : item.nextSibling);
      });
    }
    if (kind === "req" || kind === "defect") {
      const badge = document.createElement("button");
      badge.className = `pri-badge ${/^P[0-3]$/.test(pri) ? pri : "unset"}`;
      badge.textContent = /^P[0-3]$/.test(pri) ? pri : "P?";
      badge.title = t("点击循环调整优先级");
      badge.addEventListener("click", async (event) => {
        event.stopPropagation();
        const order = ["P0", "P1", "P2", "P3"];
        const next = order[(order.indexOf(pri) + 1) % order.length];
        try {
          await invoke("docs_update", { projectDir: currentProject, kind, action: "update", id: entry.id, priority: next });
          toast(`${entry.id} ${t("优先级已调整为")} ${next}`);
          refreshDocs();
        } catch (error) {
          toastError(`优先级保存失败:${error}`);
        }
      });
      row.appendChild(badge);
    }
    if (kind === "req" && el.id !== "req-list") {
      const complexityBadge = document.createElement("span");
      complexityBadge.className = "complexity-badge";
      complexityBadge.textContent = cx === "小" || cx === "中" || cx === "大" ? `${t("复杂度")}:${t(cx)}` : t("未评估");
      row.appendChild(complexityBadge);
    }
    const title = document.createElement("span");
    title.className = "title";
    title.textContent = entry.title;
    row.appendChild(title);
    item.appendChild(row);

    // 展开面板:完整标题、字段、合法的状态流转按钮(与硬门禁同一套规则)。
    const detail = document.createElement("div");
    detail.className = expandedIds.has(entry.id) ? "doc-detail" : "doc-detail hidden";
    const full = document.createElement("div");
    full.className = "doc-full-title";
    // 行内不显示需求 ID(R-054),身份收进展开详情——这里必须给全。
    full.textContent = kind === "req" ? `${entry.id} · ${entry.title}` : entry.title;
    detail.appendChild(full);
    // R-123:字段编辑只在独立文档页。侧栏展开详情因此退回只读呈现,高度显著下降,
    // 回到"浏览与取活"的本职;编辑能力没有丢,在文档页有完整入口。
    const deepManage = surface === "documents" && (kind === "req" || kind === "defect") && !entry.closed;
    if (deepManage) {
      const editBox = document.createElement("div");
      editBox.className = "doc-edit";
      // 每格都带可见字段名:字段集是自由的,只靠顺序或 tooltip 认不出改的是哪一条。
      // 段落型字段(内容/验收/复现/进展)按值长度自动升级为 textarea——不硬编码字段名。
      const addRow = (label, value, hint) => {
        const row = document.createElement("label");
        row.className = "doc-edit-row";
        const name = document.createElement("span");
        name.className = "doc-edit-key";
        name.textContent = label;
        const multiline = value.length > 60 || value.includes("\n");
        const control = document.createElement(multiline ? "textarea" : "input");
        control.value = value;
        control.title = hint;
        if (multiline) control.rows = Math.min(10, Math.max(3, Math.ceil(value.length / 42)));
        row.append(name, control);
        editBox.appendChild(row);
        return control;
      };
      const titleInput = addRow(t("标题"), entry.title, t("编辑标题"));
      const fieldInputs = [];
      for (const [key, value] of entry.fields ?? []) {
        fieldInputs.push([key, addRow(key, value, `${t("编辑字段")}: ${key}`)]);
      }
      const save = document.createElement("button");
      save.type = "button";
      save.className = "primary mini";
      save.textContent = t("保存修改");
      save.addEventListener("click", async (event) => {
        event.stopPropagation();
        try {
          await invoke("docs_update", {
            projectDir: currentProject,
            kind,
            action: "update",
            id: entry.id,
            title: titleInput.value,
            fields: Object.fromEntries(fieldInputs.map(([key, input]) => [key, input.value])),
          });
          toast(t("已保存"));
          refreshDocs();
        } catch (error) {
          toastError(`记录保存失败:${error}`);
        }
      });
      const editActions = document.createElement("div");
      editActions.className = "doc-edit-actions";
      editActions.appendChild(save);
      editBox.appendChild(editActions);
      editBox.addEventListener("click", (event) => event.stopPropagation());
      detail.appendChild(editBox);
    }
    // 编辑表单已经把每个字段连名带值摆出来了,再渲染一遍只读列表就是同一份内容显示两遍;
    // 「阻塞字段: X」这条理由更是直接重复 阻塞 字段的原文(D-165)。有编辑表单时:
    // 阻塞原因只留调度器推导出来的理由(依赖/阶段/循环),只读列表只留 refs(它是可跳转的链接)。
    const hasEditor = deepManage;
    if (blocked || externalBlocked) {
      const reasons = hasEditor
        ? blockedReasons.filter((reason) => !String(reason).startsWith("阻塞字段:"))
        : blockedReasons;
      const shown = reasons.length ? reasons : hasEditor ? [] : [t("缺少阻塞原因")];
      if (shown.length) {
        const blockBox = document.createElement("div");
        blockBox.className = "doc-blocked-detail";
        const blockTitle = document.createElement("strong");
        blockTitle.textContent = t("阻塞原因");
        blockBox.appendChild(blockTitle);
        for (const reason of shown) {
          const line = document.createElement("div");
          line.textContent = `• ${reason}`;
          blockBox.appendChild(line);
        }
        detail.appendChild(blockBox);
      }
    }
    for (const [key, value] of entry.fields ?? []) {
      const isRefs = key.toLowerCase() === "refs";
      if (hasEditor && !isRefs) continue;
      const f = document.createElement("div");
      f.className = "doc-field";
      if (isRefs) {
        f.append(`${key}: `);
        for (const ref of String(value).split(/[\s,]+/).filter(Boolean)) {
          const link = document.createElement("button");
          link.className = "ref-link";
          link.textContent = ref;
          link.addEventListener("click", (event) => {
            event.stopPropagation();
            jumpToEntry(ref);
          });
          f.appendChild(link);
          f.append(" ");
        }
      } else {
        f.textContent = `${key}: ${value}`;
      }
      detail.appendChild(f);
    }
    // 复杂度是元数据编辑,同样归文档页;侧栏保留三格电量图标做只读呈现。
    if (deepManage) {
      const complexityRow = document.createElement("div");
      complexityRow.className = "doc-progress";
      const complexitySelect = document.createElement("select");
      complexitySelect.innerHTML = `<option value="">${t("未评估")}</option><option>小</option><option>中</option><option>大</option>`;
      complexitySelect.value = cx;
      complexitySelect.title = kind === "defect" ? t("设置缺陷复杂度") : t("设置需求复杂度");
      complexitySelect.addEventListener("click", (event) => event.stopPropagation());
      complexitySelect.addEventListener("change", async () => {
        try {
          await invoke("docs_update", { projectDir: currentProject, kind, action: "update", id: entry.id, fields: { "复杂度": complexitySelect.value } });
          toast(t("复杂度已保存"));
          refreshDocs();
        } catch (error) {
          toastError(`复杂度保存失败:${error}`);
        }
      });
      complexityRow.append(`${t("复杂度")}: `, complexitySelect);
      detail.appendChild(complexityRow);
    }
    // 目标专属:状态速记(写入 fields.状态,同时保留计划字段用于展示)。
    if (kind === "goal" && !entry.closed) {
      const progressRow = document.createElement("div");
      progressRow.className = "doc-progress";
      const input = document.createElement("input");
      input.placeholder = t("记录状态/调整方向,回车保存");
      input.addEventListener("click", (e) => e.stopPropagation());
      input.addEventListener("keydown", async (e) => {
        if (e.key !== "Enter" || !input.value.trim()) return;
        try {
          const msg = await invoke("docs_update", {
            projectDir: currentProject,
            kind,
            action: "update",
            id: entry.id,
            fields: { "状态": input.value.trim() },
          });
          log(msg);
          refreshDocs();
        } catch (err) {
          toastError(String(err));
        }
      });
      progressRow.appendChild(input);
      detail.appendChild(progressRow);
    }
    if ((entry.nextStatuses ?? []).length > 0) {
      const actions = document.createElement("div");
      actions.className = "doc-actions";
      for (const next of entry.nextStatuses) {
        const btn = document.createElement("button");
        btn.className = "ghost mini";
        btn.textContent = `→ ${t("转")} ${localizedDocStatus(next)}`;
        btn.addEventListener("click", async (e) => {
          e.stopPropagation();
          try {
            const msg = await invoke("docs_update", {
              projectDir: currentProject,
              kind,
              action: "update",
              id: entry.id,
              status: next,
            });
            log(msg);
            refreshDocs();
          } catch (err) {
            toastError(String(err));
            log(`状态流转失败:${err}`, "warn");
          }
        });
        actions.appendChild(btn);
      }
      detail.appendChild(actions);
    }
    item.appendChild(detail);
    row.addEventListener("keydown", (event) => {
      if (event.key !== "Enter" && event.key !== " ") return;
      event.preventDefault();
      detail.classList.toggle("hidden");
      row.setAttribute("aria-expanded", String(!detail.classList.contains("hidden")));
    });
    row.setAttribute("aria-expanded", String(!detail.classList.contains("hidden")));
    row.addEventListener("click", () => {
      detail.classList.toggle("hidden");
      row.setAttribute("aria-expanded", String(!detail.classList.contains("hidden")));
    });
    el.appendChild(item);
  }
  // 已完成项归档在 *-archive.md,不占侧边栏;一行入口可翻历史。
  if (archivedCount > 0) {
    const foot = document.createElement("button");
    foot.type = "button";
    foot.className = "doc-archive-toggle";
    foot.setAttribute("aria-label", `${t("展开已归档条目")}，共 ${archivedCount} ${t("条")}`);
    foot.setAttribute("aria-expanded", "false");
    foot.title = `${t("展开已归档条目")};${t("双击打开归档文件")}`;
    foot.textContent = `${archivedCount} ${t("条")} ${t("已归档")} ▸`;
    const archive = document.createElement("div");
    archive.className = "doc-archive-list hidden";
    for (const entry of archivedEntries) {
      const row = document.createElement("div");
      row.className = "archived-entry";
      // 归档条目也要挂 id:被引用的条目多半正是已经做完归档的那些,没有它跳转必然落空。
      row.dataset.docId = entry.id;
      row.textContent = `${entry.id} ${entry.title} [${entry.status}]`;
      archive.appendChild(row);
    }
    foot.addEventListener("click", () => {
      archive.classList.toggle("hidden");
      const expanded = !archive.classList.contains("hidden");
      foot.setAttribute("aria-expanded", String(expanded));
      foot.textContent = `${archivedCount} ${t("条")} ${t("已归档")} ${expanded ? "▾" : "▸"}`;
    });
    foot.addEventListener("dblclick", () => openDocViewer(`${kind}-archive`));
    el.append(foot, archive);
  }
}

function formatWorkspaceTime(value) {
  if (!value) return t("暂无时间");
  return new Date(Number(value)).toLocaleString();
}

async function selectWorkspaceProject(path) {
  try {
    const previous = currentProject;
    renderProjects(await invoke("projects_select", { path }));
    if (previous !== path) {
      setRunning(false, "空闲");
      clearChat();
      bgClear();
      renderTodoPanel([], 0, 0);
      await loadConversation();
      await refreshDocs();
      await loadModels();
      refreshGit();
      await refreshPendingInputs();
      await refreshProcesses();
    }
    refreshWorkspace();
  } catch (error) {
    toastError(`切换项目失败:${error}`);
  }
}

let lastWorkspaceSnapshot = null;
function renderWorkspace(snapshot) {
  lastWorkspaceSnapshot = snapshot;
  const root = $("workspace-projects");
  root.replaceChildren();
  for (const project of snapshot.projects ?? []) {
    const card = document.createElement("section");
    card.className = `workspace-card${project.current ? " current" : ""}`;
    card.setAttribute("role", "button");
    card.tabIndex = 0;
    card.setAttribute("aria-label", `${t("选择工作区项目")} ${project.name}`);
    if (project.current) card.setAttribute("aria-current", "page");
    const head = document.createElement("div");
    head.className = "workspace-card-head";
    const title = document.createElement("strong");
    title.textContent = project.name;
    const status = document.createElement("span");
    status.className = `workspace-status ${project.status}`;
    status.textContent = project.status === "running" ? t("运行中") : project.status === "failed" ? t("失败") : t("空闲");
    head.append(title, status);
    const path = document.createElement("div");
    path.className = "dim workspace-path";
    path.textContent = project.path;
    const conversation = project.conversation;
    const summary = document.createElement("div");
    summary.className = "workspace-summary";
    summary.textContent = conversation
      ? `${t("当前对话")}: ${conversation.title} · ${conversation.message_count} ${t("条")}`
      : `${t("当前对话")}: ${t("暂无")}`;
    const activity = document.createElement("div");
    activity.className = "workspace-activity dim";
    const trace = (project.recent_activity ?? []).flatMap((item) => item.events ?? []);
    activity.textContent = trace.length
      ? `${t("最近活动")}: ${trace.slice(0, 3).map((item) => item.text || item.name || t("运行事件")).join(" · ")}`
      : `${t("最近活动")}: ${t("暂无")}`;
    const queue = document.createElement("div");
    queue.className = "workspace-meta dim";
    queue.textContent = `${t("排队")} ${project.pending_count ?? 0} ${t("条")} · ${t("更新于")} ${formatWorkspaceTime(project.updated_at)}`;
    card.append(head, path, summary, activity, queue);
    card.addEventListener("keydown", (event) => {
      if (event.key !== "Enter" && event.key !== " ") return;
      event.preventDefault();
      card.click();
    });
    card.addEventListener("click", () => selectWorkspaceProject(project.path));
    root.appendChild(card);
  }
}

async function refreshWorkspace() {
  try {
    renderWorkspace(await invoke("workspace_snapshot"));
  } catch (error) {
    toastError(`工作区刷新失败:${error}`, { retry: refreshWorkspace });
  }
}
let documentsKind = "req";
let latestDocsSnapshot = null;
const documentFilters = {
  req: { status: "all", priority: "all", complexity: "all", tag: "all", blocked: "all", sort: "manual", grouped: localStorage.getItem("kz-grouped-docs") !== "0" },
  defect: { status: "all", priority: "all", tag: "all", blocked: "all", grouped: localStorage.getItem("kz-grouped-docs") !== "0" },
};
const documentStatusOptions = {
  req: [["all", "全部状态"], ["todo", "todo"], ["doing", "doing"], ["done", "done"], ["dropped", "dropped"]],
  defect: [["all", "全部状态"], ["open", "open"], ["fixing", "fixing"], ["fixed", "fixed"], ["wontfix", "wontfix"]],
};
// 「对照」模式下筛选同时作用于两个队列——并排看的前提就是同一套条件,
// 各筛各的等于没在对照。单类型模式只作用于当前那一个。
function docFilterTargets() {
  return documentsKind === "both" ? ["req", "defect"] : [documentsKind];
}
function syncDocumentFilters(snapshot) {
  const statusFilter = $("documents-status-filter");
  const priorityFilter = $("documents-priority-filter");
  const tagFilter = $("documents-tag-filter");
  const blockedFilter = $("documents-blocked-filter");
  const primary = docFilterTargets()[0];
  const filters = documentFilters[primary];
  const entries =
    documentsKind === "both"
      ? [...(snapshot.requirements ?? []), ...(snapshot.defects ?? [])]
      : documentsKind === "req"
        ? (snapshot.requirements ?? [])
        : (snapshot.defects ?? []);
  // 对照模式下两个队列的状态机不同,状态筛选只提供"全部",避免给出对另一边无意义的值。
  const statusOptions =
    documentsKind === "both" ? [["all", "全部状态"]] : documentStatusOptions[documentsKind];
  statusFilter.innerHTML = statusOptions
    .map(([value, label]) => `<option value="${value}">${localizeDynamic(label)}</option>`)
    .join("");
  statusFilter.disabled = documentsKind === "both";
  statusFilter.value = documentsKind === "both" ? "all" : filters.status;
  priorityFilter.value = filters.priority ?? "all";
  blockedFilter.value = filters.blocked ?? "all";
  // 回落必须写回状态,不能只改下拉的显示值。
  filters.tag = syncTagFilter(tagFilter, entries, filters.tag ?? "all");
}
function renderDocuments(snapshot) {
  latestDocsSnapshot = snapshot;
  const reqList = $("documents-req-list");
  const defectList = $("documents-defect-list");
  if (!reqList || !defectList) return;
  syncDocumentFilters(snapshot);
  // 两处都把原始条目交给 renderDocList 自己筛:这里曾经预筛一遍缺陷再传进去,
  // 等于同一套筛选写了两份,改一处漏一处就会两边对不上(R-123 验收 ④)。
  renderDocList(reqList, snapshot.requirements ?? [], "req", snapshot.archived?.req ?? 0, documentFilters.req, snapshot.archived_entries?.req ?? []);
  renderDocList(defectList, snapshot.defects ?? [], "defect", snapshot.archived?.defect ?? 0, documentFilters.defect, snapshot.archived_entries?.defect ?? []);
  // 「对照」把两个队列并排摆出来:需求与缺陷互相引用,分成两个标签页时对不起来。
  const both = documentsKind === "both";
  reqList.classList.toggle("hidden", !both && documentsKind !== "req");
  defectList.classList.toggle("hidden", !both && documentsKind !== "defect");
  $("documents-scroll")?.classList.toggle("compare", both);
  $("documents-tab-req").className = documentsKind === "req" ? "primary" : "ghost";
  $("documents-tab-defect").className = documentsKind === "defect" ? "primary" : "ghost";
  const compareTab = $("documents-tab-both");
  if (compareTab) compareTab.className = both ? "primary" : "ghost";
  syncBatchBar();
}
/// 只重绘文档列表与计数(不含历史/测试/工作树):供运行中高频刷新使用。
function renderDocsSnapshot(snapshot) {
  reqFilters.tag = syncTagFilter($("req-tag-filter"), snapshot.requirements ?? [], reqFilters.tag);
  defectFilters.tag = syncTagFilter($("defect-tag-filter"), snapshot.defects ?? [], defectFilters.tag);
  renderDocList($("req-list"), snapshot.requirements, "req", snapshot.archived?.req ?? 0, reqFilters, snapshot.archived_entries?.req ?? []);
  renderDocList($("defect-list"), snapshot.defects, "defect", snapshot.archived?.defect ?? 0, defectFilters, snapshot.archived_entries?.defect ?? []);
  renderDocList($("goal-list"), snapshot.goals ?? [], "goal", snapshot.archived?.goal ?? 0, reqFilters, snapshot.archived_entries?.goal ?? []);
  renderDocuments(snapshot);
  renderDocList($("source-list"), snapshot.sources ?? [], "source", snapshot.archived?.source ?? 0, reqFilters, snapshot.archived_entries?.source ?? []);
  renderDocList($("finding-list"), snapshot.findings ?? [], "finding", snapshot.archived?.finding ?? 0, reqFilters, snapshot.archived_entries?.finding ?? []);
  $("research-count").textContent = `${(snapshot.sources ?? []).length + (snapshot.findings ?? []).length}`;
  $("req-count").textContent = `${snapshot.requirements.filter((r) => !r.closed).length}`;
  $("defect-count").textContent = `${snapshot.defects.filter((d) => !d.closed).length}`;
  $("goal-count").textContent = `${(snapshot.goals ?? []).filter((g) => g.status === "active").length}`;
  renderConventions(snapshot.conventions);
  applyLanguage();
}

// ---------- 记忆页(R-107):透明化——架构总览/条目/账单/检索/整理 ----------
let memorySelection = null;
async function refreshMemory() {
  if (!currentProject) {
    $("memory-arch").innerHTML = `<p class="dim">${t("先在左侧「项目」里添加并选择一个目录")}</p>`;
    return;
  }
  try {
    const [overview, billData, recallData, candidates] = await Promise.all([
      invoke("memory_overview", { projectDir: currentProject }),
      invoke("memory_context_bill", { projectDir: currentProject }),
      invoke("memory_recalls", { projectDir: currentProject, limit: 20 }),
      invoke("memory_note_candidates", { projectDir: currentProject }),
    ]);
    renderMemoryArch(overview);
    renderMemoryBill(billData);
    renderMemoryRecalls(recallData);
    renderMemoryCandidates(candidates);
    if (memorySelection) await loadMemoryList(memorySelection.scope, memorySelection.category);
  } catch (err) {
    toastError(`${t("记忆页加载失败")}:${err}`, { retry: refreshMemory });
  }
}

// ---------- R-127 运行画像面板 ----------
// 判断 agent 跑得好不好此前全靠翻轨迹。数据源早就有(RunSummary 的 context_report、
// summarize_tools、summarize_metrics),缺的只是把它们汇到一处。
async function refreshMetrics() {
  if (!currentProject) {
    $("metrics-rounds").innerHTML = `<p class="dim">${t("先在左侧「项目」里添加并选择一个目录")}</p>`;
    return;
  }
  try {
    const data = await invoke("run_metrics", { projectDir: currentProject, limit: 20 });
    renderMetrics(data?.rounds ?? []);
  } catch (err) {
    toastError(`${t("运行画像加载失败")}:${err}`, { retry: refreshMetrics });
  }
}

function renderMetrics(rounds) {
  const trend = $("metrics-trend");
  const list = $("metrics-rounds");
  trend.innerHTML = "";
  list.innerHTML = "";
  if (!rounds.length) {
    list.innerHTML = `<p class="dim">${t("还没有轮次记录:跑一轮后这里会出现画像")}</p>`;
    return;
  }
  // 趋势:只统计确实度量过的轮次。把"早于度量落地"的轮次算成 0 会把趋势整体压低,
  // 得出"冗余在下降"的假结论。
  const measured = rounds.filter((r) => r.measured);
  if (measured.length) {
    const avg = (pick) => measured.reduce((sum, r) => sum + (pick(r) || 0), 0) / measured.length;
    const cells = [
      [t("平均终端调用"), avg((r) => r.metrics.terminal_calls).toFixed(1)],
      [t("平均 git 查询组"), avg((r) => r.metrics.git_groups).toFixed(1)],
      [t("edit 未命中率"), `${(avg((r) => (r.metrics.edit_calls ? r.metrics.edit_misses / r.metrics.edit_calls : 0)) * 100).toFixed(0)}%`],
      [t("平均步数"), avg((r) => r.steps).toFixed(1)],
      [t("平均输出 token"), Math.round(avg((r) => r.outputTokens))],
    ];
    trend.innerHTML =
      `<div class="metrics-trend-head dim">${t("近")} ${measured.length} ${t("轮均值")}</div>` +
      cells
        .map(([name, value]) => `<div class="metrics-cell"><span class="dim">${escapeHtml(name)}</span><strong>${escapeHtml(String(value))}</strong></div>`)
        .join("");
  }
  for (const round of rounds) {
    const item = document.createElement("div");
    item.className = `metrics-round${round.outcome === "halted" ? " halted" : ""}`;
    const m = round.metrics || {};
    const contextTotal = Object.values(round.context || {}).reduce(
      (sum, entry) => sum + (Array.isArray(entry) ? entry[1] : Number(entry) || 0),
      0,
    );
    const head = document.createElement("div");
    head.className = "metrics-round-head";
    head.innerHTML =
      `<span>${escapeHtml(new Date(round.at).toLocaleString())}</span>` +
      `<span class="dim">${escapeHtml(round.outcome)} · ${round.steps} ${t("步")} · ↑${round.inputTokens} ↓${round.outputTokens}</span>`;
    const prompt = document.createElement("div");
    prompt.className = "metrics-round-prompt dim";
    prompt.textContent = round.prompt;
    const stats = document.createElement("div");
    stats.className = "metrics-round-stats dim";
    stats.textContent = round.measured
      ? `${t("终端")} ${m.terminal_calls ?? 0} · git ${m.git_calls ?? 0}(${m.git_groups ?? 0} ${t("组")}) · edit ${m.edit_misses ?? 0}/${m.edit_calls ?? 0} ${t("未命中")} · ${t("子代理")} ${m.subagent_calls ?? 0} · ${t("失败")} ${m.failed_calls ?? 0}/${m.total_calls ?? 0} · ${t("上下文")} ${contextTotal}`
      : t("该轮早于度量落地,无画像");
    const tools = document.createElement("div");
    tools.className = "metrics-round-tools dim";
    tools.textContent = Object.entries(round.tools || {})
      .sort((a, b) => b[1] - a[1])
      .map(([name, count]) => `${name}×${count}`)
      .join("  ");
    item.append(head, prompt, stats, tools);
    list.appendChild(item);
  }
}

// ---------- R-126 UI 自查探针:在真实运行中的窗口里取样 ----------
// 后端工具发 kz:ui-probe,这里取样后用 ui_probe_result 回传。取的是用户眼前这个
// 窗口的实际渲染结果——不是重新起一个空白页,那样查不出任何真实的渲染问题。
const UI_PROBE_NODE_LIMIT = 60;

function describeNode(el, depth) {
  const indent = "  ".repeat(depth);
  const cls = el.className && typeof el.className === "string" ? `.${el.className.trim().split(/\s+/).join(".")}` : "";
  const id = el.id ? `#${el.id}` : "";
  // 只取本节点的直接文本,不含子节点——否则每层都把整棵子树的文字重复一遍。
  const own = [...el.childNodes]
    .filter((n) => n.nodeType === 3)
    .map((n) => n.nodeValue.trim())
    .filter(Boolean)
    .join(" ")
    .slice(0, 80);
  const box = el.getBoundingClientRect?.();
  const hidden = box && box.width === 0 && box.height === 0 ? " [不可见]" : "";
  return `${indent}<${el.tagName.toLowerCase()}${id}${cls}>${hidden}${own ? ` "${own}"` : ""}`;
}

function probeDom(selector) {
  const roots = [...document.querySelectorAll(selector)];
  if (!roots.length) return `没有匹配 \`${selector}\` 的元素(选择器写错,或该区域此刻未渲染)。`;
  const lines = [];
  let truncated = false;
  const walk = (el, depth) => {
    if (lines.length >= UI_PROBE_NODE_LIMIT) {
      truncated = true;
      return;
    }
    lines.push(describeNode(el, depth));
    for (const child of el.children) walk(child, depth + 1);
  };
  for (const root of roots.slice(0, 5)) walk(root, 0);
  const head = `匹配 ${roots.length} 个${roots.length > 5 ? "(只展开前 5 个)" : ""}:`;
  // 截断必须可见:静默截断会让 agent 以为看到了全部(既有 conventions 的教训)。
  return `${head}\n${lines.join("\n")}${truncated ? `\n… 已截断(上限 ${UI_PROBE_NODE_LIMIT} 个节点)` : ""}`;
}

function probeConsole() {
  if (!uiConsoleLog.length) return "自加载以来没有 console 错误或警告。";
  return uiConsoleLog
    .map((e) => `[${e.level}] ${new Date(e.at).toLocaleTimeString()} ${e.text}`)
    .join("\n");
}

function probeStyle(selector) {
  const els = [...document.querySelectorAll(selector)].slice(0, 5);
  if (!els.length) return `没有匹配 \`${selector}\` 的元素。`;
  // 只给与"为什么没显示/为什么挤成一团"相关的属性,不倾倒整个 computed style。
  const keys = [
    "display", "position", "visibility", "opacity", "overflow",
    "flexDirection", "gridTemplateColumns", "width", "height", "maxHeight",
    "margin", "padding", "whiteSpace", "textOverflow", "zIndex",
  ];
  return els
    .map((el, index) => {
      const style = window.getComputedStyle(el);
      const box = el.getBoundingClientRect();
      const props = keys.map((k) => `${k}=${style[k]}`).join(" ");
      return `#${index + 1} ${describeNode(el, 0).trim()}\n  盒模型: ${Math.round(box.width)}×${Math.round(box.height)} @ (${Math.round(box.left)},${Math.round(box.top)})\n  ${props}`;
    })
    .join("\n");
}

on("kz:ui-probe", (event) => {
  const { id, kind, arg } = event.payload ?? {};
  let result;
  try {
    if (kind === "dom") result = probeDom(arg);
    else if (kind === "console") result = probeConsole();
    else if (kind === "style") result = probeStyle(arg);
    else result = `未知探针类型: ${kind}`;
  } catch (err) {
    // 探针自身出错也要如实回传,不能让后端悬到超时。
    result = `探针执行失败: ${err}`;
  }
  invoke("ui_probe_result", { id, result }).catch(() => {});
});

// R-124:待确认候选。SOP 是用户的常用模板,不能由 agent 自己决定入库——
// 所以候选只停在这里,采纳/丢弃都是用户一键的事。
function renderMemoryCandidates(list) {
  const box = $("memory-candidates");
  const count = $("memory-candidate-count");
  if (!box) return;
  box.innerHTML = "";
  const items = Array.isArray(list) ? list : [];
  count.textContent = items.length ? `· ${items.length}` : "";
  if (!items.length) {
    box.innerHTML = `<p class="dim">${t("暂无待确认候选")}</p>`;
    return;
  }
  for (const item of items) {
    const row = document.createElement("div");
    row.className = `memory-candidate${item.hint === "sop" ? " sop" : ""}`;
    row.dataset.fingerprint = item.fingerprint || "";
    const head = document.createElement("div");
    head.className = "memory-candidate-head";
    head.innerHTML =
      `<span class="memory-candidate-hint">${escapeHtml(item.hint || "note")}</span>` +
      `<span class="memory-candidate-summary">${escapeHtml(item.summary || "")}</span>`;
    const detail = document.createElement("pre");
    detail.className = "memory-candidate-detail dim";
    detail.textContent = item.detail || "";
    const actions = document.createElement("div");
    actions.className = "memory-candidate-actions";
    const adopt = document.createElement("button");
    adopt.type = "button";
    adopt.className = "primary mini";
    adopt.textContent = t("采纳");
    adopt.title = t("交给记忆管理子代理提炼成条目");
    adopt.addEventListener("click", async () => {
      adopt.disabled = true;
      try {
        await invoke("memory_consolidate", { projectDir: currentProject });
        toast(t("已交给记忆管理子代理提炼"));
        refreshMemory();
      } catch (err) {
        adopt.disabled = false;
        toastError(`${t("提炼失败")}:${err}`);
      }
    });
    const drop = document.createElement("button");
    drop.type = "button";
    drop.className = "ghost mini danger";
    drop.textContent = t("丢弃");
    drop.title = t("直接移出候选箱,不再进入提炼范围");
    drop.addEventListener("click", async () => {
      try {
        await invoke("memory_note_discard", {
          projectDir: currentProject,
          scope: item.scope,
          fingerprint: item.fingerprint,
        });
        toast(t("已丢弃"));
        refreshMemory();
      } catch (err) {
        toastError(`${t("丢弃失败")}:${err}`);
      }
    });
    actions.append(adopt, drop);
    row.append(head, detail, actions);
    box.appendChild(row);
  }
}

// R-125:召回明细。没有这块界面就没有任何评估手段——记忆有没有用只能凭感觉。
function renderMemoryRecalls(data) {
  const box = $("memory-recalls");
  const rate = $("memory-recall-rate");
  if (!box) return;
  box.innerHTML = "";
  const rounds = data?.rounds ?? [];
  const total = data?.rounds_total ?? rounds.length;
  const used = data?.rounds_with_fetch ?? 0;
  // 采纳率放在标题上:一眼就能看出"召回了但没人用"是不是常态。
  rate.textContent = total ? `· ${t("采纳")} ${used}/${total}` : "";
  if (!rounds.length) {
    box.innerHTML = `<p class="dim">${t("还没有召回记录:开跑时若无记忆命中,这里就是空的")}</p>`;
    return;
  }
  for (const round of rounds) {
    const item = document.createElement("div");
    item.className = "memory-recall";
    const head = document.createElement("div");
    head.className = "memory-recall-head";
    const when = new Date(round.at).toLocaleString();
    const adopted = round.hits.filter((h) => h.fetched).length;
    head.innerHTML =
      `<span class="memory-recall-when">${escapeHtml(when)}</span>` +
      `<span class="dim">${round.hits.length} ${t("条命中")} · ${t("已采纳")} ${adopted} · ${t("注入")} ${round.injected_bytes}B</span>`;
    const prompt = document.createElement("div");
    prompt.className = "memory-recall-prompt dim";
    prompt.textContent = round.prompt_head;
    prompt.title = round.prompt_head;
    item.append(head, prompt);
    for (const hit of round.hits) {
      const row = document.createElement("div");
      row.className = `memory-recall-hit${hit.fetched ? " adopted" : ""}`;
      // 得分与片段一起给:「为什么召回这一条」必须能看出来,否则调不了检索。
      row.innerHTML =
        `<span class="memory-recall-id">${escapeHtml(hit.id)}</span>` +
        `<span class="memory-recall-title">${escapeHtml(hit.title)}</span>` +
        `<span class="dim">${hit.score.toFixed(2)}</span>` +
        `<span class="memory-recall-flag">${hit.fetched ? t("已采纳") : t("未拉取")}</span>`;
      const snip = document.createElement("div");
      snip.className = "memory-recall-snippet dim";
      snip.textContent = hit.snippet.replace(/\n/g, " ");
      row.appendChild(snip);
      item.appendChild(row);
    }
    box.appendChild(item);
  }
}

function renderMemoryArch(overview) {
  const arch = $("memory-arch");
  arch.innerHTML = "";
  let inboxPending = 0;
  for (const scope of overview.scopes || []) {
    inboxPending += scope.inboxPending || 0;
    const card = document.createElement("div");
    card.className = "memory-scope-card";
    const head = document.createElement("div");
    head.className = "memory-scope-head";
    const label = scope.scope === "global" ? t("全局记忆") : t("项目记忆");
    head.innerHTML = `<strong>${label}</strong> <span class="dim">${scope.total} ${t("条")} · ${t("命中")} ${scope.hitsTotal} · ${escapeHtml(scope.root)}</span>`;
    card.appendChild(head);
    const grid = document.createElement("div");
    grid.className = "memory-cat-grid";
    for (const [cat, info] of Object.entries(scope.categories || {})) {
      const cell = document.createElement("button");
      cell.type = "button";
      cell.className = "memory-cat-cell";
      cell.setAttribute("aria-label", `${label} ${cat}`);
      const staleNote = info.stale ? `${info.stale} stale` : "";
      cell.innerHTML = `<span class="memory-cat-name">${escapeHtml(cat)}</span><span class="memory-cat-count">${info.active}</span><span class="dim">${staleNote} ${escapeHtml(info.last || "")}</span>`;
      cell.addEventListener("click", () => {
        memorySelection = { scope: scope.scope, category: cat };
        loadMemoryList(scope.scope, cat);
      });
      grid.appendChild(cell);
    }
    card.appendChild(grid);
    if ((scope.integrity || []).length) {
      const warn = document.createElement("p");
      warn.className = "memory-warn";
      warn.textContent = `⚠ ${scope.integrity.join("; ")}`;
      card.appendChild(warn);
    }
    arch.appendChild(card);
  }
  $("memory-inbox-badge").textContent = inboxPending ? `inbox ${inboxPending} ${t("条待整理")}` : "";
}

async function loadMemoryList(scope, category) {
  try {
    const list = await invoke("memory_entries", { projectDir: currentProject, scope, category });
    const container = $("memory-list");
    container.innerHTML = "";
    if (!list.length) {
      container.innerHTML = `<p class="dim">${t("该分类暂无记忆")}</p>`;
      return;
    }
    for (const entry of list) {
      const row = document.createElement("button");
      row.type = "button";
      // R-125:零命中要一眼看得出来——这是判断某条记忆该不该留的直接依据。
      // 只在条目有一定年纪时才标,刚写下来还没被检索过不算"没用"。
      const ageDays = memoryAgeDays(entry.updated);
      const dormant = (entry.hits ?? 0) === 0 && ageDays >= 3 && entry.status !== "stale";
      row.className = `memory-row${entry.status === "stale" ? " stale" : ""}${dormant ? " dormant" : ""}`;
      row.dataset.memoryId = entry.id;
      const lastHit = entry.lastHitAt
        ? `${t("最近命中")} ${new Date(entry.lastHitAt).toLocaleDateString()}`
        : t("从未命中");
      row.innerHTML =
        `<span class="memory-row-id">${escapeHtml(entry.id)}</span>` +
        `<span class="memory-row-title">${escapeHtml(entry.title)}</span>` +
        `<span class="dim">${escapeHtml(entry.description)}</span>` +
        `<span class="memory-row-meta dim">${escapeHtml(entry.status)} · ${t("命中")} ${entry.hits} · ${lastHit} · ${escapeHtml(entry.updated)}` +
        `${dormant ? ` · <em class="memory-dormant-flag">${t("长期零命中")}</em>` : ""}</span>`;
      row.addEventListener("click", () => showMemoryDetail(scope, entry));
      container.appendChild(row);
    }
  } catch (err) {
    toastError(`${t("记忆条目加载失败")}:${err}`);
  }
}

function showMemoryDetail(scope, entry) {
  const box = $("memory-detail");
  box.classList.remove("hidden");
  box.innerHTML = "";
  const meta = document.createElement("p");
  meta.className = "dim";
  meta.textContent = `${entry.id} · ${entry.status} · ${t("来源")} ${entry.source} · ${entry.path}`;
  const title = document.createElement("input");
  title.value = entry.title;
  title.setAttribute("aria-label", t("记忆标题"));
  const desc = document.createElement("input");
  desc.value = entry.description;
  desc.setAttribute("aria-label", t("召回钩子"));
  const body = document.createElement("textarea");
  body.value = entry.body;
  body.rows = 8;
  body.setAttribute("aria-label", t("记忆正文"));
  const save = document.createElement("button");
  save.type = "button";
  save.className = "primary";
  save.textContent = t("保存修改");
  save.addEventListener("click", async () => {
    try {
      await invoke("memory_entry_save", {
        projectDir: currentProject,
        scope,
        id: entry.id,
        title: title.value,
        description: desc.value,
        body: body.value,
        status: null,
      });
      toast(t("记忆已保存"));
      refreshMemory();
    } catch (err) {
      toastError(`${t("记忆保存失败")}:${err}`);
    }
  });
  const staleBtn = document.createElement("button");
  staleBtn.type = "button";
  staleBtn.className = "ghost";
  staleBtn.textContent = entry.status === "active" ? t("标记失效") : t("恢复启用");
  staleBtn.addEventListener("click", async () => {
    try {
      await invoke("memory_entry_save", {
        projectDir: currentProject,
        scope,
        id: entry.id,
        title: null,
        description: null,
        body: null,
        status: entry.status === "active" ? "stale" : "active",
      });
      box.classList.add("hidden");
      refreshMemory();
    } catch (err) {
      toastError(`${t("记忆保存失败")}:${err}`);
    }
  });
  // 零命中的条目要能直接删掉,不能只有"标记失效"——stale 仍占索引与列表。
  const deleteBtn = document.createElement("button");
  deleteBtn.type = "button";
  deleteBtn.className = "ghost danger";
  deleteBtn.textContent = t("删除");
  deleteBtn.title = t("从磁盘删除该记忆文件,不可撤销");
  deleteBtn.addEventListener("click", async () => {
    if (!window.confirm(`${t("确认删除")} ${entry.id}?${t("此操作不可撤销")}`)) return;
    try {
      await invoke("memory_entry_delete", { projectDir: currentProject, scope, id: entry.id });
      toast(t("已删除"));
      box.classList.add("hidden");
      refreshMemory();
    } catch (err) {
      toastError(`${t("删除失败")}:${err}`);
    }
  });
  // 效果画像:这条到底被用过没有,直接摆在详情里,不用去列表里对。
  const profile = document.createElement("p");
  profile.className = "dim memory-profile";
  const lastHit = entry.lastHitAt ? new Date(entry.lastHitAt).toLocaleString() : t("从未命中");
  profile.textContent = `${t("累计命中")} ${entry.hits ?? 0} · ${t("最近命中")} ${lastHit}`;
  const actions = document.createElement("div");
  actions.className = "memory-detail-actions";
  actions.append(save, staleBtn, deleteBtn);
  box.append(meta, profile, title, desc, body, actions);
}

// 条目"年纪":用于零命中判定——刚写下来还没被检索过不算没用。
function memoryAgeDays(updated) {
  const stamp = Date.parse(`${updated}T00:00:00Z`);
  if (Number.isNaN(stamp)) return 0;
  return Math.max(0, Math.floor((Date.now() - stamp) / 86_400_000));
}

function renderMemoryBill(data) {
  const bill = $("memory-bill");
  bill.innerHTML = "";
  const entries = Array.isArray(data.bill) ? data.bill : [];
  if (!entries.length) {
    bill.innerHTML = `<p class="dim">${t("暂无账单数据(跑一轮后生成)")}</p>`;
  } else {
    const total = entries.reduce((sum, item) => sum + (item[1] || 0), 0);
    for (const [name, chars] of entries) {
      const pct = total ? Math.round((chars / total) * 100) : 0;
      const row = document.createElement("div");
      row.className = "memory-bill-row";
      row.innerHTML = `<span class="memory-bill-name">${escapeHtml(name)}</span><span class="dim">${chars} · ${pct}%</span><span class="memory-bill-bar" style="width:${Math.max(pct, 2)}%"></span>`;
      bill.appendChild(row);
    }
  }
  const eps = $("memory-episodes");
  eps.innerHTML = "";
  const episodes = data.episodes || [];
  if (!episodes.length) {
    eps.innerHTML = `<p class="dim">${t("暂无轮次记录")}</p>`;
    return;
  }
  for (const ep of episodes) {
    const tools = Object.entries(ep.tools || {})
      .map(([name, count]) => `${name}×${count}`)
      .join(" ");
    const row = document.createElement("div");
    row.className = "memory-episode";
    row.innerHTML = `<span class="memory-episode-prompt">${escapeHtml(ep.prompt)}</span><span class="dim">${escapeHtml(ep.outcome)} · ${ep.steps} steps${tools ? " · " + escapeHtml(tools) : ""}</span>`;
    eps.appendChild(row);
  }
}

$("memory-search-input").addEventListener("keydown", async (event) => {
  if (event.key !== "Enter") return;
  const query = event.target.value.trim();
  if (!query || !currentProject) return;
  try {
    const hits = await invoke("memory_search_page", { projectDir: currentProject, query });
    memorySelection = null;
    const container = $("memory-list");
    container.innerHTML = hits.length ? "" : `<p class="dim">${t("没有命中的记忆")}</p>`;
    for (const hit of hits) {
      const row = document.createElement("div");
      row.className = "memory-row";
      row.innerHTML = `<span class="memory-row-id">${escapeHtml(hit.id)}</span><span class="memory-row-title">${escapeHtml(hit.title)}</span><span class="dim">${escapeHtml(hit.snippet)}</span><span class="memory-row-meta dim">${escapeHtml(hit.scope)}/${escapeHtml(hit.category)} · ${t("命中")} ${hit.hits}</span>`;
      container.appendChild(row);
    }
  } catch (err) {
    toastError(`${t("记忆检索失败")}:${err}`);
  }
});

$("memory-consolidate-btn").addEventListener("click", async () => {
  if (!currentProject) return;
  try {
    const result = await invoke("memory_consolidate", { projectDir: currentProject });
    toast(result.pending ? t("inbox 尚有草稿未消化") : t("inbox 已整理完毕"));
    refreshMemory();
  } catch (err) {
    toastError(`${t("整理失败")}:${err}`);
  }
});

async function refreshDocs() {
  if (!currentProject) return;
  try {
    const snapshot = await invoke("docs_snapshot", { projectDir: currentProject });
    renderDocsSnapshot(snapshot);
    await refreshConversationList();
    await refreshTests();
    await refreshWorktrees();
  } catch (err) {
    toastError(`项目文档刷新失败:${err}`, { retry: refreshDocs });
  }
}

// agent 在运行中改需求/缺陷/目标时,侧栏必须跟着动:否则状态、计数和状态流转按钮
// 会一直停在开跑前的样子,要等本轮结束才更新(D-098)。合并 400ms 内的连续变更。
let docsLiveTimer = null;
function refreshDocsSoon() {
  if (!currentProject) return;
  clearTimeout(docsLiveTimer);
  docsLiveTimer = setTimeout(async () => {
    docsLiveTimer = null;
    // 重绘会清空列表容器:用户正在写快记或正在拖拽排序时先让路,稍后再刷。
    if (document.querySelector(".quickreq-form") || document.querySelector(".doc-item.dragging")) {
      refreshDocsSoon();
      return;
    }
    try {
      renderDocsSnapshot(await invoke("docs_snapshot", { projectDir: currentProject }));
    } catch (err) {
      console.error(err);
    }
  }, 400);
}

$("documents-project-select").addEventListener("change", (event) => {
  if (event.target.value && event.target.value !== currentProject) selectWorkspaceProject(event.target.value);
});

$("documents-tab-req").addEventListener("click", () => { documentsKind = "req"; if (latestDocsSnapshot) renderDocuments(latestDocsSnapshot); });
$("documents-tab-defect").addEventListener("click", () => { documentsKind = "defect"; if (latestDocsSnapshot) renderDocuments(latestDocsSnapshot); });
$("documents-tab-both").addEventListener("click", () => { documentsKind = "both"; if (latestDocsSnapshot) renderDocuments(latestDocsSnapshot); });

async function runDefectReview() {
  if (!currentProject) {
    toast(t("先在左侧「项目」里添加并选择一个目录"));
    return;
  }
  const button = $("defect-review");
  const status = $("defect-review-status");
  button.disabled = true;
  status.textContent = t("正在审查缺陷…");
  try {
    const result = await invoke("defect_review", { projectDir: currentProject });
    if (result.empty) {
      status.textContent = t("当前没有活动缺陷");
      toast(t("当前没有活动缺陷"));
      return;
    }
    status.textContent = t("审查完成");
    openRuntimeMarkdown(t("缺陷自动审查报告"), result.report);
  } catch (err) {
    status.textContent = t("审查失败");
    toastError(`${t("审查失败")}:${err}`, { retry: runDefectReview });
  } finally {
    button.disabled = false;
  }
}
$("defect-review").addEventListener("click", runDefectReview);

$("bg-type-filter").addEventListener("change", (e) => {
  bgFilters.type = e.target.value;
  localStorage.setItem("kz-bg-type", bgFilters.type);
  applyBgFilters();
});
$("bg-status-filter").addEventListener("change", (e) => {
  bgFilters.status = e.target.value;
  localStorage.setItem("kz-bg-status", bgFilters.status);
  applyBgFilters();
});
$("bg-type-filter").value = bgFilters.type;
$("bg-status-filter").value = bgFilters.status;

$("documents-batch-apply").addEventListener("click", applyBatch);
$("documents-batch-clear").addEventListener("click", () => { batchSelection.clear(); if (latestDocsSnapshot) renderDocuments(latestDocsSnapshot); });
// 筛选写进 docFilterTargets() 给出的每个队列:对照模式下两边共用同一套条件。
function applyDocFilter(field, value) {
  for (const kind of docFilterTargets()) documentFilters[kind][field] = value;
  saveDocFilters();
  if (latestDocsSnapshot) renderDocuments(latestDocsSnapshot);
}
// 交付方式(插入/排队)是个人习惯,全局记一份即可,不按项目分。
$("delivery-select").addEventListener("change", (event) => {
  localStorage.setItem("kz-delivery", event.target.value);
});
$("documents-status-filter").addEventListener("change", (e) => applyDocFilter("status", e.target.value));
$("documents-priority-filter").addEventListener("change", (e) => applyDocFilter("priority", e.target.value));
$("documents-tag-filter").addEventListener("change", (e) => applyDocFilter("tag", e.target.value));
$("documents-blocked-filter").addEventListener("change", (e) => applyDocFilter("blocked", e.target.value));
// 分组开关(用户定调:按受控标签分组展示,含侧边栏):关掉即回纯开发顺序+拖拽。
function bindGroupToggle(id, storageKey, apply) {
  const btn = $(id);
  if (!btn) return;
  const sync = (on) => {
    btn.setAttribute("aria-pressed", String(on));
    btn.classList.toggle("active", on);
  };
  sync(apply(null));
  btn.addEventListener("click", () => {
    const on = apply("toggle");
    localStorage.setItem(storageKey, on ? "1" : "0");
    sync(on);
    if (latestDocsSnapshot) renderDocsSnapshot(latestDocsSnapshot);
  });
}
bindGroupToggle("req-group-toggle", "kz-grouped-req", (op) => {
  if (op === "toggle") reqFilters.grouped = !reqFilters.grouped;
  return reqFilters.grouped;
});
bindGroupToggle("defect-group-toggle", "kz-grouped-defect", (op) => {
  if (op === "toggle") defectFilters.grouped = !defectFilters.grouped;
  return defectFilters.grouped;
});
bindGroupToggle("documents-group-toggle", "kz-grouped-docs", (op) => {
  if (op === "toggle") {
    const next = !documentFilters.req.grouped;
    documentFilters.req.grouped = next;
    documentFilters.defect.grouped = next;
  }
  return documentFilters.req.grouped;
});
// 筛选折叠(用户定调:侧边栏筛选再收一层):默认收起,状态持久化。
for (const [btnId, rowId, storageKey] of [
  ["req-filter-toggle", "req-filter-row", "kz-filters-req"],
  ["defect-filter-toggle", "defect-filter-row", "kz-filters-defect"],
]) {
  const btn = $(btnId);
  const row = $(rowId);
  if (!btn || !row) continue;
  const apply = (open) => {
    row.classList.toggle("hidden", !open);
    btn.setAttribute("aria-expanded", String(open));
    btn.classList.toggle("active", open);
  };
  apply(localStorage.getItem(storageKey) === "1");
  btn.addEventListener("click", () => {
    const open = row.classList.contains("hidden");
    localStorage.setItem(storageKey, open ? "1" : "0");
    apply(open);
  });
}
for (const [id, key] of [["req-status-filter", "status"], ["req-priority-filter", "priority"], ["req-complexity-filter", "complexity"], ["req-tag-filter", "tag"], ["req-blocked-filter", "blocked"], ["req-sort", "sort"]]) {
  $(id).addEventListener("change", (event) => {
    reqFilters[key] = event.target.value;
    if (key === "sort") localStorage.setItem("kz-req-sort", event.target.value);
    saveDocFilters();
    refreshDocs();
  });
}
$("defect-tag-filter").addEventListener("change", (event) => {
  defectFilters.tag = event.target.value;
  saveDocFilters();
  refreshDocs();
});
$("defect-blocked-filter").addEventListener("change", (event) => {
  defectFilters.blocked = event.target.value;
  saveDocFilters();
  refreshDocs();
});

// ---------- R-053 快速记录:独立子代理结构化落库(需求/缺陷通用),不打断主对话 ----------
function quickCaptureForm(kind, sectionId, noun) {
  const section = $(sectionId);
  const title = section.querySelector(".section-title");
  if (title.querySelector(".quickreq-form")) return;
  const form = document.createElement("div");
  form.className = "goal-add-form quickreq-form";
  const input = document.createElement("textarea");
  input.rows = 3;
  input.placeholder = `${t("自然语言描述")}${t(noun)};Ctrl+Enter 或点${t("提交")},Esc ${t("取消")}。${t("独立子代理后台进行")},不打断当前对话。`;
  const submit = async () => {
    const text = input.value.trim();
    if (!text) {
      toast(t("先写点描述"));
      return;
    }
    // 失败时表单必须还在:提交前销毁会让用户写的描述无处可寻。
    submitBtn.disabled = true;
    cancelBtn.disabled = true;
    input.disabled = true;
    toast(`${t("记录中")}${t(noun)}…(${t("独立子代理后台进行")})`);
    try {
      const msg = await invoke("quick_req", { projectDir: currentProject, description: text, kind });
      form.remove();
      toast(`${t("已记录")}:${msg}`);
      refreshDocs();
    } catch (err) {
      submitBtn.disabled = false;
      cancelBtn.disabled = false;
      input.disabled = false;
      toastError(`${t("记录失败(内容已保留,可重试)")}:${err}`);
    }
  };
  input.addEventListener("keydown", (e) => {
    if (e.key === "Escape") form.remove();
    else if (e.key === "Enter" && (e.ctrlKey || e.metaKey)) submit();
  });
  const bar = document.createElement("div");
  bar.className = "quickreq-bar";
  const cancelBtn = document.createElement("button");
  cancelBtn.className = "ghost mini";
  cancelBtn.textContent = t("取消");
  cancelBtn.addEventListener("click", () => form.remove());
  const submitBtn = document.createElement("button");
  submitBtn.className = "primary mini";
  submitBtn.textContent = t("提交");
  submitBtn.addEventListener("click", submit);
  bar.append(cancelBtn, submitBtn);
  form.append(input, bar);
  title.append(form);
  input.focus();
}
$("req-quick").addEventListener("click", () => quickCaptureForm("req", "req-section", "需求"));
$("defect-quick").addEventListener("click", () => quickCaptureForm("defect", "defect-section", "缺陷"));

function renderConventions(conv) {
  const el = $("conv-list");
  el.innerHTML = "";
  if (!conv || !conv.exists) {
    const empty = document.createElement("div");
    empty.className = "doc-empty";
    empty.textContent = t("未创建,点 ＋ 生成模板;agent 会自动遵守此文件");
    el.appendChild(empty);
    return;
  }
  // 规范不再铺开章节列表占满侧边栏:一行入口,点开进应用内 MD 查看器。
  const item = document.createElement("button");
  item.type = "button";
  item.className = "doc-item conv-entry";
  item.setAttribute("aria-label", `${t("打开开发规范")}，${conv.headings.length}${t("个章节")}`);
  item.textContent = `${conv.headings.length}${t("个章节")} · ${t("点击查看")}`;
  item.title = conv.headings.slice(0, 12).join("\n");
  item.addEventListener("click", () => openDocViewer("conventions"));
  el.appendChild(item);
}

// 新建目标:内联输入(webview 无 window.prompt)。
$("goal-add").addEventListener("click", () => {
  const list = $("goal-list");
  if (list.querySelector(".goal-add-form")) return;
  const form = document.createElement("div");
  form.className = "goal-add-form";
  const input = document.createElement("input");
  input.placeholder = t("目标描述,回车创建(Esc 取消)");
  input.addEventListener("keydown", async (e) => {
    if (e.key === "Escape") {
      form.remove();
      return;
    }
    if (e.key !== "Enter" || !input.value.trim()) return;
    try {
      const msg = await invoke("docs_update", {
        projectDir: currentProject,
        kind: "goal",
        action: "add",
        id: "",
        title: input.value.trim(),
      });
      log(msg);
      form.remove();
      refreshDocs();
    } catch (err) {
      toastError(String(err));
    }
  });
  form.appendChild(input);
  list.prepend(form);
  input.focus();
});

$("conv-init").addEventListener("click", async () => {
  try {
    const path = await invoke("conventions_init", { projectDir: currentProject });
    toast(`规范文件已就绪:${path}`);
    refreshDocs();
  } catch (err) {
    toastError(String(err), { retry: () => $("conv-init").click() });
  }
});
$("conv-open").addEventListener("click", () => openDocViewer("conventions"));

// ---------- 应用内文档查看器:markdown/代码直接渲染,外部打开是兜底 ----------
let viewerKind = null;
function openRuntimeMarkdown(title, content) {
  viewerKind = null;
  $("viewer-title").textContent = title;
  const body = $("viewer-body");
  body.className = "md";
  body.innerHTML = renderMarkdown(content ?? "");
  body.scrollTop = 0;
  $("viewer-external").classList.add("hidden");
  $("viewer-overlay").classList.remove("hidden");
  $("viewer-close").focus();
}
async function openDocViewer(kind) {
  try {
    const doc = await invoke("docs_read", { projectDir: currentProject, kind });
    viewerKind = kind;
    $("viewer-external").classList.remove("hidden");
    $("viewer-title").textContent = doc.name;
    const body = $("viewer-body");
    if (doc.name.endsWith(".md")) {
      body.className = "md";
      body.innerHTML = renderMarkdown(doc.content);
    } else {
      body.className = "";
      body.innerHTML = `<pre class="code">${escapeHtml(doc.content)}</pre>`;
    }
    body.scrollTop = 0;
    $("viewer-overlay").classList.remove("hidden");
    $("viewer-close").focus();
  } catch (err) {
    toastError(String(err), { retry: () => openDocViewer(kind) });
  }
}
$("viewer-close").addEventListener("click", () => $("viewer-overlay").classList.add("hidden"));
$("viewer-overlay").addEventListener("click", (e) => {
  if (e.target === $("viewer-overlay")) $("viewer-overlay").classList.add("hidden");
});
$("viewer-external").addEventListener("click", () => {
  if (viewerKind) invoke("docs_open", { projectDir: currentProject, kind: viewerKind }).catch((e) => toastError(String(e), { retry: () => $("viewer-external").click() }));
});

// ---------- git 状态 ----------
async function refreshGit() {
  if (!currentProject) return;
  try {
    const g = await invoke("git_status", { projectDir: currentProject });
    $("status-git").textContent = g.branch
      ? `⎇ ${g.branch}${g.changes ? ` +${g.changes}` : ""}`
      : "";
    $("status-git").title = g.last ? `${t("最近提交")}:${g.last}` : "";
  } catch {
    $("status-git").textContent = "";
  }
}

// 运行中改文件/跑命令后刷新工作区徽章,合并 600ms 内的连续变更。
let gitLiveTimer = null;
function refreshGitSoon() {
  clearTimeout(gitLiveTimer);
  gitLiveTimer = setTimeout(() => {
    gitLiveTimer = null;
    refreshGit();
  }, 600);
}

function renderRecoveredMessages(items) {
  followLatest = true;
  messages.innerHTML = "";
  currentAssistant = null;
  currentReasoning = null;
  currentReasoningHead = null;
  // 调用与结果按 call_id 配对成一块渲染:原先每个 part 各占一行,
  // 结果行只显示原始 call id,对人毫无信息量(用户 2026-08-08 反馈"太丑")。
  const pending = new Map();
  for (const message of items ?? []) {
    for (const part of message.parts ?? []) {
      if (part.type === "tool_call") {
        const block = buildToolBlock(part.name || "tool", part.input);
        messages.appendChild(block.wrap);
        if (part.id) pending.set(part.id, { block, input: part.input });
        continue;
      }
      if (part.type === "tool_result") {
        const entry = pending.get(part.call_id);
        if (entry) {
          pending.delete(part.call_id);
          fillToolBlock(entry.block, {
            ok: !part.is_error,
            content: part.content,
            input: entry.input,
          });
        } else {
          // 配对不上(历史被压缩过):独立成块,总比丢掉强。
          const orphan = buildToolBlock("tool result", {});
          messages.appendChild(orphan.wrap);
          fillToolBlock(orphan, { ok: !part.is_error, content: part.content });
        }
        continue;
      }
      if (part.type !== "text" || !part.text?.trim()) continue;
      const el = addMessage(message.role === "assistant" ? "assistant md" : "user", "");
      if (message.role === "assistant") {
        el.dataset.raw = part.text;
        el.querySelector(".message-body").innerHTML = renderMarkdown(part.text);
      } else {
        el.querySelector(".message-body").textContent = part.text;
      }
    }
  }
  // 没等到结果的调用(轮次被中断):标出来,不要停在"运行中"的假象上。
  for (const { block } of pending.values()) {
    block.wrap.classList.remove("running");
    block.result.textContent = `⎿ ${t("无结果(轮次中断)")}`;
    block.result.classList.remove("hidden");
  }
  if (!items?.length) {
    messages.innerHTML = `<div id="empty-state"><div class="logo-mark">K</div><div class="hint">${t("输入任务开始 · 权限请求会弹窗询问 · Ctrl+Enter 发送")}</div></div>`;
  }
  scrollBottom(true);
}

async function loadConversation(sequence = null) {
  if (!currentProject) return;
  try {
    bgClear();
    renderTodoPanel([], 0, 0);
    const history = await invoke("conversation_get", { projectDir: currentProject, processId: activeProcessId, sequence });
    renderRecoveredMessages(history);
    const traces = await invoke("conversation_trace_get", { projectDir: currentProject, processId: activeProcessId, sequence });
    renderRecoveredTraces(traces);
    log(`${t("已恢复")} ${history.length} ${t("条")} ${t("历史消息")} ${traces.length} ${t("组工具轨迹")}`);
  } catch (err) {
    addMessage("error", `${t("历史消息恢复失败")}:${err}`);
    toastError(`${t("历史消息恢复失败")}:${err}`, { retry: () => loadConversation(sequence) });
  }
}

let conversationItems = [];
function renderConversationList(items) {
  conversationItems = items ?? [];
  const el = $("conversation-list");
  el.innerHTML = "";
  $("chat-select-all").checked = false;
  $("conversation-count").textContent = items.length;
  if (!items.length) {
    el.textContent = t("暂无历史对话");
    return;
  }
  for (const item of [...items].reverse()) {
    const row = document.createElement("div");
    row.className = "doc-item conv-row";
    row.title = t("点击打开 · 勾选后点标题栏的删除图标批量删除");
    const check = document.createElement("input");
    check.type = "checkbox";
    check.className = "chat-check";
    check.dataset.seqs = JSON.stringify(item.sequences ?? [item.sequence]);
    check.addEventListener("click", (e) => e.stopPropagation());
    check.addEventListener("change", () => {
      const checks = [...document.querySelectorAll(".chat-check")];
      $("chat-select-all").checked = checks.length > 0 && checks.every((item) => item.checked);
    });
    const title = document.createElement("span");
    title.className = "title";
    title.textContent = `${item.title || t("新对话")} (${item.message_count} ${t("条")})`;
    row.append(check, title);
    row.addEventListener("click", async () => {
      if (running) {
        toast(t("运行中请先完成或停止当前任务，再打开历史对话"));
        return;
      }
      try {
        await loadConversation(item.sequence);
        addMessage("notice", `${t("已打开历史对话")} #${item.sequence}`);
      } catch (err) {
        toastError(String(err));
      }
    });
    el.appendChild(row);
  }
}

$("chat-select-all").addEventListener("change", (event) => {
  document.querySelectorAll(".chat-check").forEach((check) => { check.checked = event.target.checked; });
});

$("chat-del").addEventListener("click", async () => {
  const sequences = [...document.querySelectorAll(".chat-check:checked")]
    .flatMap((c) => JSON.parse(c.dataset.seqs));
  if (!sequences.length) {
    toast(t("先勾选要删除的历史对话"));
    return;
  }
  try {
    const n = await invoke("conversation_delete", { projectDir: currentProject, processId: activeProcessId, sequences });
    toast(`${t("已删除")} ${n}${t("份对话快照")}`);
    await refreshConversationList();
  } catch (err) {
    toastError(String(err), { retry: () => $("chat-del").click() });
  }
});

async function refreshConversationList() {
  if (!currentProject) return;
  try {
    renderConversationList(await invoke("conversation_list", { projectDir: currentProject, processId: activeProcessId }));
  } catch (err) {
    $("conversation-list").textContent = `${t("历史对话加载失败")}:${err}`;
    toastError(`${t("历史对话加载失败")}:${err}`, { retry: refreshConversationList });
  }
}

// ---------- 新对话 ----------
function clearChat(noticeText) {
  messages.innerHTML = "";
  currentAssistant = null;
  currentReasoning = null;
  currentReasoningHead = null;
  ctxTokens = 0;
  renderTokens();
  if (noticeText) addMessage("notice", noticeText);
}

$("new-chat").addEventListener("click", async () => {
  if (running) {
    toast(t("任务运行中,先停止再开新对话"));
    return;
  }
  try {
    await invoke("conversation_clear", { projectDir: currentProject, processId: activeProcessId });
    clearChat(t("已开启新对话(历史已清空)"));
    await refreshConversationList();
    log(t("新对话:多轮历史已清空"));
  } catch (err) {
    toastError(String(err), { retry: () => $("new-chat").click() });
  }
});

// ---------- 对话总结 ----------
$("summarize-btn").addEventListener("click", async () => {
  if (!currentProject) {
    toast(t("先选择一个项目"));
    return;
  }
  const transcript = [...messages.querySelectorAll(".msg, .tool-chip")]
    .map((el) => el.textContent.trim())
    .filter(Boolean)
    .join("\n\n")
    .slice(0, 60000);
  if (!transcript) {
    toast(t("当前没有可总结的对话"));
    return;
  }
  $("summarize-btn").disabled = true;
  setStatus(`${t("总结中")}(fast model)`, true);
  log(t("开始总结当前对话…"));
  try {
    const r = await invoke("summarize_chat", { projectDir: currentProject, transcript });
    addSummaryEntry(r.summary, r.path);
    toast(t("小总结已收纳到活动面板"));
    log(`总结完成,已收纳并存档:${r.path}`);
  } catch (err) {
    toastError(`总结失败:${err}`, { retry: () => $("summarize-btn").click() });
  } finally {
    $("summarize-btn").disabled = false;
    setStatus(running ? t("运行中") : t("空闲"), running);
  }
});

for (const [btn, kind] of [["req-open", "req"], ["defect-open", "defect"], ["goal-open", "goal"], ["report-open", "report"]]) {
  $(btn).addEventListener("click", () => openDocViewer(kind));
}

// ---------- 设置 ----------
let settingsProviders = [];

async function testProvider(provider) {
  try {
    const mode = $("set-proxy-mode")?.value;
    const proxy = mode === "custom" ? $("set-proxy-url")?.value.trim() : mode;
    return await invoke("provider_test", {
      protocol: provider.protocol,
      baseUrl: provider.baseUrl,
      apiKeyEnv: provider.apiKeyEnv || null,
      apiKey: provider.apiKey || null,
      auth: provider.auth || null,
      proxy: proxy || null,
    });
  } catch (err) {
    return `${t("测试失败")}:${err}`;
  }
}

function renderProviders() {
  const tbody = document.querySelector("#providers-table tbody");
  tbody.innerHTML = "";
  settingsProviders.forEach((p, index) => {
    const tr = document.createElement("tr");

    const tdName = document.createElement("td");
    const nameInput = document.createElement("input");
    nameInput.value = p.name;
    nameInput.addEventListener("input", () => (p.name = nameInput.value));
    tdName.appendChild(nameInput);

    const tdProtocol = document.createElement("td");
    const protocolSelect = document.createElement("select");
    for (const proto of ["anthropic", "openai", "openai-responses"]) {
      const opt = document.createElement("option");
      opt.value = proto;
      opt.textContent = proto;
      if (p.protocol === proto) opt.selected = true;
      protocolSelect.appendChild(opt);
    }
    protocolSelect.addEventListener("change", () => (p.protocol = protocolSelect.value));
    tdProtocol.appendChild(protocolSelect);

    const tdUrl = document.createElement("td");
    const urlInput = document.createElement("input");
    urlInput.value = p.baseUrl;
    urlInput.addEventListener("input", () => (p.baseUrl = urlInput.value));
    tdUrl.appendChild(urlInput);

    const tdKey = document.createElement("td");
    if (p.auth) {
      // 特殊认证(codex 订阅登录态):只展示,不可编辑成 key。
      const badge = document.createElement("span");
      badge.className = "key-state key-ok";
      badge.textContent = `${t("订阅登录态")}(${p.auth})`;
      tdKey.appendChild(badge);
    } else {
      const envInput = document.createElement("input");
      envInput.value = p.apiKeyEnv ?? "";
      envInput.placeholder = t("环境变量名(可选)");
      envInput.title = t("读取该环境变量作为 key");
      envInput.addEventListener("input", () => (p.apiKeyEnv = envInput.value));
      const keyInput = document.createElement("input");
      keyInput.type = "password";
      keyInput.value = p.apiKey ?? "";
      keyInput.placeholder = t("或直接粘贴 key");
      keyInput.title = t("直填优先于环境变量;明文存 kanzei.toml");
      keyInput.addEventListener("input", () => (p.apiKey = keyInput.value));
      tdKey.append(envInput, keyInput);
      if (p.keyPresent !== null && p.keyPresent !== undefined) {
        const state = document.createElement("span");
        state.className = `key-state ${p.keyPresent ? "key-ok" : "key-missing"}`;
        state.textContent = p.keyPresent ? t("已设") : t("缺失");
        tdKey.appendChild(state);
      }
    }
    // 当场探测:401/超时都给可操作提示,不用跑一轮对话才发现 key 坏了。
    {
      const testBtn = document.createElement("button");
      testBtn.className = "ghost mini";
      testBtn.textContent = t("测试");
      testBtn.setAttribute("aria-label", `${t("测试")} ${p.name || "provider"} ${t("连接")}`);
      const result = document.createElement("div");
      result.className = "key-test-result";
      testBtn.addEventListener("click", async () => {
        testBtn.disabled = true;
        result.textContent = `${t("测试中")}…`;
        try {
          result.textContent = await testProvider(p);
        } finally {
          testBtn.disabled = false;
        }
      });
      tdKey.append(testBtn, result);
    }

    // D-015:context_limit 必须在表单可见可编辑,保存不许丢字段。
    const tdCtx = document.createElement("td");
    const ctxInput = document.createElement("input");
    ctxInput.type = "number";
    ctxInput.value = p.contextLimit ?? "";
    ctxInput.placeholder = `(${t("不限")})`;
    ctxInput.addEventListener("input", () => {
      const n = parseInt(ctxInput.value, 10);
      p.contextLimit = Number.isFinite(n) && n > 0 ? n : null;
    });
    tdCtx.appendChild(ctxInput);

    const tdRemove = document.createElement("td");
    const removeBtn = document.createElement("button");
    removeBtn.className = "icon-btn";
    removeBtn.textContent = "×";
    removeBtn.setAttribute("aria-label", `${t("移除 provider")} ${p.name || index + 1}`);
    removeBtn.addEventListener("click", () => {
      settingsProviders.splice(index, 1);
      renderProviders();
    });
    tdRemove.appendChild(removeBtn);

    tr.append(tdName, tdProtocol, tdUrl, tdKey, tdCtx, tdRemove);
    tbody.appendChild(tr);
  });
}

async function deletePermissionRule(rule) {
  try {
    await invoke("permission_rule_delete", { projectDir: currentProject, index: rule.index });
    toast(t("已删除权限规则"));
    await loadPermissionRules();
  } catch (err) {
    toastError(`删除失败: ${err}`, { retry: () => deletePermissionRule(rule) });
  }
}

function renderPermissionRules(data) {
  const tbody = $("permission-rules-table").querySelector("tbody");
  tbody.replaceChildren();
  const rules = data?.rules ?? [];
  $("permission-rules-empty").classList.toggle("hidden", rules.length > 0);
  $("permission-rules-path").textContent = data?.path ? `${t("配置")}: ${data.path}` : "";
  for (const rule of rules) {
    const row = document.createElement("tr");
    const action = document.createElement("td");
    action.textContent = rule.action;
    const resource = document.createElement("td");
    resource.textContent = rule.resource;
    const controls = document.createElement("td");
    const remove = document.createElement("button");
    remove.className = "icon-btn";
    remove.title = t("删除规则");
    remove.setAttribute("aria-label", `${t("删除权限规则")} ${rule.action} ${rule.resource}`);
    remove.textContent = "×";
    remove.addEventListener("click", async () => {
      if (!confirm(`${t("删除")} ${rule.action} / ${rule.resource}？`)) return;
      await deletePermissionRule(rule);
    });
    controls.appendChild(remove);
    row.append(action, resource, controls);
    tbody.appendChild(row);
  }
}

async function loadPermissionRules() {
  if (!currentProject) {
    renderPermissionRules({ rules: [] });
    return;
  }
  try {
    renderPermissionRules(await invoke("permission_rules_get", { projectDir: currentProject }));
  } catch (err) {
    renderPermissionRules({ rules: [] });
    toastError(`读取权限规则失败: ${err}`, { retry: loadPermissionRules });
  }
}
// D-157:设置页是一张表单,填了不点保存不生效。此前没有任何提示,于是界面显示
// deepseek、运行却用 anthropic,而报错只说"provider anthropic 需要环境变量",
// 完全看不出"你以为改了的那个根本没生效"。这里做脏状态可见。
const SETTINGS_FORM_IDS = [
  "set-primary", "set-fast", "set-profile", "set-reasoning",
  "set-proxy-mode", "set-proxy-url",
];
let settingsSnapshot = "";
function settingsFingerprint() {
  // provider 表格是动态行,单独序列化;它和标量字段一起构成"这张表单当前的样子"。
  const scalars = SETTINGS_FORM_IDS.map((id) => `${id}=${$(id)?.value ?? ""}`).join("|");
  const providers = JSON.stringify(
    settingsProviders.map((p) => [p.name, p.protocol, p.baseUrl, p.apiKeyEnv, p.apiKey, p.contextLimit]),
  );
  return `${scalars}||${providers}`;
}
function syncSettingsDirty() {
  const badge = $("settings-dirty");
  if (!badge) return;
  badge.classList.toggle("hidden", settingsFingerprint() === settingsSnapshot);
}
function markSettingsSaved() {
  settingsSnapshot = settingsFingerprint();
  syncSettingsDirty();
}

// 生效值与全局值不一致 = 项目级 kanzei.toml 覆盖了。必须明说,否则用户会
// 一直在改一个不生效的值(D-168)。
function renderEffectiveNotice(s) {
  const box = $("settings-effective");
  if (!box) return;
  const diffs = [];
  for (const [key, label] of [["primary", "primary"], ["fast", "fast"], ["reasoning", "思考强度"]]) {
    const global = key === "reasoning" ? (s.reasoning === "off" ? null : s.reasoning) : s[key];
    const eff = s.effective?.[key] ?? null;
    if (s.effective && (eff ?? null) !== (global ?? null)) {
      diffs.push(`${label}:本页 ${global ?? "(未设)"} → 实际生效 ${eff ?? "(未设)"}`);
    }
  }
  box.classList.toggle("hidden", diffs.length === 0);
  if (diffs.length) {
    box.textContent =
      `${t("以下项被项目级配置覆盖,本页的改动不会生效")}:${diffs.join("；")}` +
      (s.projectConfig ? `(${s.projectConfig})` : "");
  }
}

// 模型角色改成真下拉:自由文本框要手打 `provider:model`,拼错一个字母要到运行时
// 才炸,而那时人早已离开设置页。这里从各 provider 探测到的清单里选,手填只作兜底。
let knownModelIds = [];
async function fillKnownModels(preserve = true) {
  const roles = [$("set-primary"), $("set-fast")].filter(Boolean);
  if (!roles.length) return;
  const current = roles.map((el) => el.value);
  try {
    const models = await invoke("models_list", { projectDir: currentProject });
    // 角色不能再指向角色(primary → primary 会绕成死循环)。
    knownModelIds = models.map((m) => m.id).filter((id) => id !== "primary" && id !== "fast");
  } catch {
    knownModelIds = [];
  }
  roles.forEach((select, index) => {
    const keep = preserve ? current[index] : select.value;
    select.innerHTML = "";
    const none = document.createElement("option");
    none.value = "";
    none.textContent = t("(未设 · 用内置默认)");
    select.appendChild(none);
    for (const id of knownModelIds) {
      const opt = document.createElement("option");
      opt.value = id;
      opt.textContent = id;
      select.appendChild(opt);
    }
    // 已保存的值可能来自探测不到的端点(没实现 /models、key 未配):必须保留,
    // 否则一进设置页就被下拉悄悄改成别的值,保存一次就把配置改坏了。
    if (keep && !knownModelIds.includes(keep)) {
      const opt = document.createElement("option");
      opt.value = keep;
      opt.textContent = `${keep}(手填)`;
      select.appendChild(opt);
    }
    const manual = document.createElement("option");
    manual.value = MANUAL_MODEL_SENTINEL;
    manual.textContent = t("＋ 手填模型…");
    select.appendChild(manual);
    select.value = keep ?? "";
  });
}

// 手填分支:两个角色下拉共用。选中哨兵值时弹输入,校验格式后插回列表。
function wireManualModelRole(id) {
  const select = $(id);
  if (!select) return;
  let last = select.value;
  select.addEventListener("change", () => {
    if (select.value !== MANUAL_MODEL_SENTINEL) {
      last = select.value;
      return;
    }
    const input = (window.prompt(t("填 provider:model,例如 deepseek:deepseek-chat")) || "").trim();
    if (!/^[\w.-]+:.+$/.test(input)) {
      if (input) toast(t("格式应为 provider:model"));
      select.value = last;
      return;
    }
    const opt = document.createElement("option");
    opt.value = input;
    opt.textContent = `${input}(手填)`;
    select.insertBefore(opt, select.lastElementChild);
    select.value = input;
    last = input;
    syncSettingsDirty();
  });
}
wireManualModelRole("set-primary");
wireManualModelRole("set-fast");
$("models-refresh")?.addEventListener("click", async () => {
  await fillKnownModels();
  toast(`${t("已重新探测")}:${knownModelIds.length}`);
});

async function loadSettings() {
  let s;
  try {
    s = await invoke("settings_get", { projectDir: currentProject });
  } catch (err) {
    // 配置损坏时不能留一张空白表单让用户无从下手(保存会把默认值写回,反而丢配置)。
    $("settings-path").textContent = t("配置读取失败");
    toastError(`设置读取失败:${err}`, { retry: loadSettings });
    return;
  }
  $("settings-path").textContent = s.path;
  // 先把已保存的值塞进去,再让 fillKnownModels 以它为基准建选项:
  // 顺序反了的话,探测不到的已存值会在建表时丢失,一保存就把配置改坏。
  $("set-primary").value = s.primary ?? "";
  $("set-fast").value = s.fast ?? "";
  await fillKnownModels();
  $("set-primary").value = s.primary ?? "";
  $("set-fast").value = s.fast ?? "";
  $("set-profile").value = s.profileDefault;
  $("set-reasoning").value = s.reasoning || "off";
  const proxy = s.proxy;
  if (proxy === "env" || proxy === "off") {
    $("set-proxy-mode").value = proxy;
    $("set-proxy-url").classList.add("hidden");
  } else {
    $("set-proxy-mode").value = "custom";
    $("set-proxy-url").value = proxy;
    $("set-proxy-url").classList.remove("hidden");
  }
  settingsProviders = s.providers;
  renderProviders();
  renderEffectiveNotice(s);
  // 刚从磁盘读回来 = 干净态,以此为基准比对后续改动。
  markSettingsSaved();
  loadPermissionRules();
}
for (const id of SETTINGS_FORM_IDS) {
  $(id)?.addEventListener("input", syncSettingsDirty);
  $(id)?.addEventListener("change", syncSettingsDirty);
}
// provider 表格是动态重建的,逐行绑会随重绘丢失;在容器上做事件委托一次覆盖全表。
// 委托要在捕获阶段之后跑——行内的 input 监听器先把值写回 settingsProviders,
// 我们才比对得到新指纹。
for (const event of ["input", "change"]) {
  $("providers-table")?.addEventListener(event, () => setTimeout(syncSettingsDirty, 0));
}

$("set-proxy-mode").addEventListener("change", () => {
  $("set-proxy-url").classList.toggle("hidden", $("set-proxy-mode").value !== "custom");
});

$("mobile-service-start").addEventListener("click", async () => {
  try {
    const info = await invoke("mobile_service_start", { projectDir: currentProject, port: null });
    $("mobile-service-status").textContent = `${info.address} · token ${info.token}`;
    $("mobile-service-start").classList.add("hidden");
    $("mobile-service-stop").classList.remove("hidden");
    toast(t("移动端本机桥接已启动"));
  } catch (error) {
    toastError(`启动移动端桥接失败:${error}`, { retry: () => $("mobile-service-start").click() });
  }
});
$("mobile-service-stop").addEventListener("click", async () => {
  try {
    await invoke("mobile_service_stop");
    $("mobile-service-status").textContent = t("移动端本机桥接已停止");
    $("mobile-service-start").classList.remove("hidden");
    $("mobile-service-stop").classList.add("hidden");
  } catch (error) {
    toastError(`停止移动端桥接失败:${error}`, { retry: () => $("mobile-service-stop").click() });
  }
});
async function agentContainerAction(action) {
  const agentId = $("agent-container-id").value.trim();
  if (!agentId) return toast(t("先填写 agent id"));
  try {
    const command = action === "create" ? "agent_container_create" : action === "upgrade" ? "agent_container_upgrade" : "agent_container_rollback";
    const args = { agentId };
    if (action === "upgrade") args.version = "2";
    const manifest = await invoke(command, args);
    const actionLabel = action === "rollback" ? t("回滚") : action === "create" ? t("创建") : t("升级");
    toast(`${t("代理容器")} ${manifest.agent_id} v${manifest.version} ${actionLabel}`);
  } catch (error) {
    toastError(String(error), { retry: () => agentContainerAction(action) });
  }
}
$("agent-container-create").addEventListener("click", () => agentContainerAction("create"));
$("agent-container-upgrade").addEventListener("click", () => agentContainerAction("upgrade"));
$("agent-container-rollback").addEventListener("click", () => agentContainerAction("rollback"));

$("provider-add").addEventListener("click", () => {
  settingsProviders.push({ name: "", protocol: "openai", baseUrl: "http://", apiKeyEnv: "" });
  renderProviders();
});

$("providers-test").addEventListener("click", async () => {
  const button = $("providers-test");
  const result = $("providers-test-result");
  if (!settingsProviders.length) {
    result.textContent = t("没有可测试的 provider");
    return;
  }
  button.disabled = true;
  result.textContent = `${t("测试中")}(0/${settingsProviders.length})…`;
  try {
    let passed = 0;
    for (const [index, provider] of settingsProviders.entries()) {
      const status = await testProvider(provider);
      if (status.startsWith("✓")) passed += 1;
      result.textContent = `${t("测试中")}(${index + 1}/${settingsProviders.length})…`;
    }
    result.textContent = `${t("连通性检查完成")}: ${passed}/${settingsProviders.length} ${t("可用")}`;
  } finally {
    button.disabled = false;
  }
});

$("settings-save").addEventListener("click", async () => {
  const mode = $("set-proxy-mode").value;
  const proxy = mode === "custom" ? $("set-proxy-url").value.trim() : mode;
  try {
    await invoke("settings_save", {
      payload: {
        primary: $("set-primary").value,
        fast: $("set-fast").value,
        proxy,
        profileDefault: $("set-profile").value,
        reasoning: $("set-reasoning").value,
        providers: settingsProviders.map((p) => ({
          name: p.name,
          protocol: p.protocol,
          baseUrl: p.baseUrl,
          apiKeyEnv: p.apiKeyEnv || null,
          apiKey: p.apiKey || null,
          auth: p.auth || null,
          contextLimit: p.contextLimit ?? null,
        })),
      },
    });
    toast(t("已保存"));
    loadSettings();
  } catch (err) {
    toastError(`保存失败: ${err}`, { retry: () => $("settings-save").click() });
  }
});

$("settings-open").addEventListener("click", () => invoke("settings_open").catch((e) => toastError(String(e), { retry: () => $("settings-open").click() })));

$("export-pick-dir").addEventListener("click", async () => {
  try {
    const path = await invoke("export_pick_dir");
    if (path) $("export-output-dir").value = path;
  } catch (error) {
    toastError(`选择导出目录失败:${error}`);
  }
});
$("export-project").addEventListener("click", async () => {
  if (!currentProject) return toast(t("先在左侧「项目」里添加并选择一个目录"));
  const outputDir = $("export-output-dir").value.trim();
  if (!outputDir) return toast(t("选择导出目录"));
  const button = $("export-project");
  button.disabled = true;
  $("export-result").textContent = `${t("导出工作资料")}…`;
  try {
    const result = await invoke("export_project_data", {
      options: {
        projectDir: currentProject,
        outputDir,
        includeMemory: $("export-memory").checked,
        includeRequirements: $("export-requirements").checked,
        includeDefects: $("export-defects").checked,
        includeConfig: $("export-config").checked,
      },
    });
    $("export-result").textContent = `${t("导出完成")}: ${result.path} (${result.files.length} ${t("条")})`;
    toast(t("导出完成"));
  } catch (error) {
    $("export-result").textContent = String(error);
    toastError(`导出失败:${error}`);
  } finally {
    button.disabled = false;
  }
});

// ---------- 版本与更新(GitHub Releases 为源) ----------
let updateUrl = null;
$("update-check").addEventListener("click", async () => {
  $("update-result").textContent = t("检查中…");
  $("update-install").classList.add("hidden");
  updateUrl = null;
  try {
    const r = await invoke("update_check");
    if (r.current) $("update-current").textContent = r.current;
    if (r.status === "none") {
      $("update-result").textContent = r.message;
    } else if (r.newer && r.url) {
      updateUrl = r.url;
      $("update-result").textContent = `${t("发现新版本")}:${r.latest}`;
      $("update-install").classList.remove("hidden");
    } else {
      $("update-result").textContent = `${t("已是最新")}(${r.latest || r.current})`;
    }
  } catch (err) {
    $("update-result").textContent = `${t("检查失败")}:${err}`;
  }
});
$("update-install").addEventListener("click", async () => {
  if (!updateUrl) return;
  $("update-result").textContent = t("下载中…(安装器就绪后会自动弹出)");
  $("update-install").disabled = true;
  try {
    $("update-result").textContent = await invoke("update_install", { url: updateUrl });
  } catch (err) {
    $("update-result").textContent = String(err);
  } finally {
    $("update-install").disabled = false;
  }
});

// ---------- 侧边栏分区折叠:标题文字收/展,记忆到 localStorage ----------
document.querySelectorAll(".sidebar-section").forEach((section) => {
  const title = section.querySelector(".section-title > span:first-child");
  if (!title) return;
  const collapseKey = section.dataset.collapseKey || title.textContent.replace(/[\d\s]/g, "").slice(0, 8);
  const key = `kz-collapse-${collapseKey}`;
  const legacyKey = `kz-collapse-${title.textContent.replace(/[\d\s]/g, "").slice(0, 8)}`;
  const saved = localStorage.getItem(key) ?? (legacyKey === key ? null : localStorage.getItem(legacyKey));
  if (saved === "1") {
    section.classList.add("collapsed");
    if (legacyKey !== key) localStorage.setItem(key, "1");
  }
  title.setAttribute("role", "button");
  title.setAttribute("tabindex", "0");
  const syncExpanded = () => title.setAttribute("aria-expanded", String(!section.classList.contains("collapsed")));
  const toggle = () => {
    const collapsed = section.classList.toggle("collapsed");
    localStorage.setItem(key, collapsed ? "1" : "0");
    syncExpanded();
  };
  syncExpanded();
  title.addEventListener("click", toggle);
  title.addEventListener("keydown", (event) => {
    if (event.key !== "Enter" && event.key !== " ") return;
    event.preventDefault();
    toggle();
  });
});

// ---------- 启动 ----------
(async () => {
  try {
    const info = await invoke("app_info");
    $("status-version").textContent = `v${info.version} (${info.build})`;
    $("update-current").textContent = String(info.build).split(" ")[0];
    log(`kanzei 桌面端启动 · v${info.version} (${info.build})`);
  } catch (err) {
    log(`获取版本失败:${err}`, "warn");
  }
  // 启动静默检查更新(安装版通道):有新包只弹一条 toast,不打断;失败不打扰。
  setTimeout(async () => {
    try {
      const r = await invoke("update_check");
      if (r.newer && r.url) toast(`发现新版本 ${r.latest} — 设置页「检查更新」可一键安装`);
    } catch {}
  }, 3000);
  // 启动链任一步失败都不能静默中断后半段(否则界面停在初始态,用户看不到任何原因)。
  for (const [label, step] of [
    ["项目列表", async () => renderProjects(await invoke("projects_get"))],
    ["历史对话", loadConversation],
    ["项目文档", refreshDocs],
    ["模型列表", loadModels],
    ["git 状态", refreshGit],
    ["排队输入", refreshPendingInputs],
  ]) {
    try {
      await step();
    } catch (err) {
      log(`启动步骤「${label}」失败:${err}`, "err");
      toastError(`${label}加载失败:${err}`);
    }
  }
  setStatus("空闲", false);
})();
