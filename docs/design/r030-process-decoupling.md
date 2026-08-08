# R-030 进程与项目解耦(多进程并行)设计

> **文档状态(2026-08-08 整理):历史设计。** 对应 R-030 已 done(P1 后端/P2 前端已落地);残余 P3「进程列表持久化」已并入 `deep-parallel-dev.md` 的 P2 阶段(该文 D1~D7 定案后动工)。本文保留为设计决策记录。

## 现状问题

- kzapp 全局只有一个运行位:`AppState.running` 单布尔,跨项目直接报错"已有其他项目的任务在运行"。
- 对话历史是全局单份(`conversation` + `conversation_project`),切项目即清空。
- 模型选择、子代理能力都是全局的,无法按任务差异化。

对标 codex:每个目录可开独立进程,互不阻塞;kanzei 还要更进一步——统一 UI 内多进程并行、每进程可选模型、可开关子代理。

## 概念模型

**Process(进程)** = 绑定一个项目目录的独立 agent 实例:

- 独立:对话历史、运行状态、排队队列(steer/queue)、模型选择、子代理开关、连跑状态
- 共享(同项目的进程之间):项目文档(需求/缺陷/目标)、开发规范、权限规则、状态库 state.db——这些经 harness 从项目文件读取,天然共享,即"共享上下文"
- 并行:不同进程完全并行(各自 tokio task);同一进程内部仍按 R-003 的 queue/steer 串行

## 后端设计(kanzei-app)

```rust
struct ProcessHandle {
    id: String,                    // "d|<root>"(默认)或 "p<seq>|<root>"
    project_dir: String,           // 创建时绑定,不可变
    model: Mutex<Option<String>>,  // None = agent 默认(primary)
    profile: Mutex<Option<String>>,
    subagent_enabled: AtomicBool,  // false → run_once 收到 None,task 工具不注册
    running: AtomicBool,
    lifecycle: Mutex<()>,          // per-process 边界串行化(取代全局 lifecycle)
    current_run: Mutex<Option<JoinHandle<()>>>,
    conversation: Mutex<Vec<Message>>,
}
// AppState: processes: Arc<Mutex<HashMap<String, Arc<ProcessHandle>>>>
```

### session_id 规则(兼容现有历史)

- 默认进程:沿用 `project_session_id(root)` —— 已有的持久化对话/事件日志直接继续可用
- 额外进程:`format!("{}#p{n}", project_session_id(root))` —— 各自独立的队列与事件流
- admit_input / promote_next_input 改为按 session_id 操作(现在按 project_dir 推导,改参即可复用 R-003 全部逻辑)

### 命令面

- `process_list(project_dir)` → 该项目的进程列表(缺省自动建默认进程);返回 {id, model, subagent, running, label}
- `process_create(project_dir, model?, subagent?)` → 新进程
- `process_update(process_id, model?, profile?, subagent?)` → 改设置(运行中允许,下轮生效)
- `process_close(process_id)` → 中止并移除(默认进程只清不删)
- `run_prompt(process_id, prompt, delivery?)` → 原 project_dir 参数改由进程携带
- `stop_run(process_id)` → 只停该进程:abort + 清该进程 pending asks + 取消其队列

### 事件与权限

- 所有 kz:* 事件 payload 增加 `processId`;前端按激活进程过滤渲染,后台进程事件进各自缓冲页
- PendingAsk 增加 process_id;kz:ask 带 processId(弹窗标注来源进程);stop 只作废本进程的询问

## 前端设计(v1)

- topbar 下加进程页签条:`[默认] [进程2 ●] [＋]`;● = running;右键/×关闭
- 每进程独立渲染状态:currentAssistant/currentReasoning/currentTool/toolChips/runTokens 收进 `procState: Map<pid, {...}>`;#messages 内每进程一个 pane,切换页签只切可见性——后台进程的输出持续写入自己的 pane,切回即见全量
- 模型下拉 + 「子代理」开关成为**进程属性**(改动调 process_update)
- 连跑/自动放行按进程记;live-status 卡与状态栏跟随激活进程;后台任务面板保持全局(条目带进程标签)
- 历史对话区(R-013)按进程过滤

## 分阶段

- **P1 后端**:ProcessHandle + 命令面 + 事件带 processId(旧前端兼容:process_id 缺省→默认进程)
- **P2 前端**:页签 + per-process 渲染状态(与 R-013 回放共用 pane 机制)
- **P3 持久化**:进程列表存 app.json,重启恢复(默认进程天然恢复,额外进程恢复其 session)

## 风险与边界

- 与 R-003 的 queue/drain 逻辑交界最深:改造原则是"参数从 project 推导改为 session_id 直给",不动其语义
- 并行进程写同一项目文件(docstore/git)存在竞争:docstore 整文件重写有 lost-update 风险,后续需要文件锁(记 R-030 P3 或独立缺陷)
- ollama 单实例串行:多进程共用 fast 模型时子代理会排队,属预期
