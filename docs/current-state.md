# Kanzei 当前实现与文档口径

核对日期：2026-10-03。当前开发基线已收拢此前未提交的通用对话、开发工作区、Harness 修复与侧栏管理。项目与无项目对话使用统一入口，已关闭历史在搜索/历史入口支持重命名和删除，侧栏只展示活动对话；使用右键菜单，不显示行尾三个点。

桌面角色动画、播放器、素材及制作工具已移除；语音和背景反馈不再依赖人物模块。研究模式的重设计暂缓，先打磨主对话、无项目模式、开发模式与基础设施。

提交历史、工作目录和恢复方式见 [开发基线整理](cleanup-2026-10-03.md)。新发行版的全量门禁以绑定提交的 `dist/verification.json` 为准；安装版状态以对应发布记录为准。下列历史验收文档只证明各自版本。

| 主题 | 当前实现 | 源码 / 说明 |
| --- | --- | --- |
| 无项目对话 | 无需添加项目；完整通用 Harness，含文件产出、命令、子代理、技能、记忆、压缩、回退、分叉与定时任务；关闭项目队列、阶段、批次与交付门禁；独立历史与产物目录 | [实现与边界](design/general_chat.md) |
| 模型认证 | Codex 登录、Anthropic API Key、OpenAI 兼容 API、本地 Ollama；Claude 订阅登录已停用 | `crates/kanzei-core/src/assemble.rs`、`crates/kanzei-app/src/settings.rs` |
| 前端 | 静态 HTML/CSS 与原生 ESM；没有应用打包器或前端框架 | `crates/kanzei-app/ui/index.html`、`scripts/ui-runtime-smoke.mjs` |
| Harness | agents/tools/skills/context/permissions 五类注册表；延迟工具目录影响首轮展示，完整执行表仍保留 | `crates/kanzei-harness/src/harness.rs` |
| 工具流水线 | 普通与委派工具共用执行包装，panic 转为明确失败并释放进度；登记校验错误分类统一。工具自身的参数校验仍有各自实现 | `crates/kanzei-harness/src/tool_pipeline.rs`、`crates/kanzei-core/src/runner/tool_exec.rs` |
| 写后校验 | Rust edition 取自 Cargo；单文件快速检查与批次 Cargo/UI 回归分开；环境、超时与代码失败分别报告 | `crates/kanzei-tools/src/local_validation.rs` |
| 长运行收口 | 无限主代理在专用文件写入后，每 32 步或 15 分钟进入最多 8 步的收口；验证、归属提交、记录进展或保存未完成检查点后自动继续 | `crates/kanzei-core/src/runner/drive/batch.rs` |
| 上下文压缩 | 手动 `/compact` 与自动压缩共用；独立 compact 模型、完整中段归档、近期原文与约束保留、摘要质量/实际节省检查、并发 CAS 和重启恢复。真实任务质量对照尚未完成 | [Harness 修复](reports/2026-10-02-harness-repair.md) |
| 长期记忆 | Markdown 是记忆内容真源；`index.db` 是可重建检索索引；实现已经独立到 `kanzei-memory` crate | `crates/kanzei-memory/src/memory/` |
| 检索 | fingerprint/lexical 与可选 dense/hybrid；向量存于 SQLite 表，由 Rust 余弦扫描并用 RRF 融合；不是 sqlite-vec 扩展 | `crates/kanzei-memory/src/memory/index.rs` |
| 检索反馈 | 召回、注入、读取分别记账；读取或注入不证明采纳，不作为在线采纳率排序依据 | [记忆反馈契约](design/memory_feedback_reliability.md) |
| Global 偏好 | 决策复核能向 global store 保存；当前 Dev 常驻偏好和 memory_search 仍以 project 为主，不能把保存成功说成跨项目生效 | `crates/kanzei-app/src/decisions.rs`、`crates/kanzei-tools/src/profiles/dev.rs`、`crates/kanzei-memory/src/memory/tools.rs` |
| 并发 | 文档与记忆写入使用 FileLock；源码通过 worktree 隔离；进程内读槽/写槽和跨进程可见性各有职责 | `crates/kanzei-base/src/atomic_file.rs`、`crates/kanzei-core/src/runner/subagent.rs` |
| 基础文件设施 | FileLock 统一使用标准库 OS 锁；CAS 在同一锁内校验并替换；日志区分正文、仅指纹、删除，查询错误显式返回；源码写入凭据按物理工作树隔离 | [基础设施重构与影响](reports/2026-10-03-base-refactor.md) |
| 内容指纹 | 字节指纹统一使用 base 的 FNV-1a；architecture/conventions 在工具层先归一 CRLF/LF。旧规范建议稿保留原文与身份，基础版本无法核实时提示人工对照，保存仍校验本次读取的当前规范 | `crates/kanzei-tools/src/lib.rs`、`crates/kanzei-tools/src/conventions/drafts.rs` |
| 会话与工作 | `state.db` 保存会话原始事件、运行状态、Work Unit 事件；这些不能从记忆 Markdown 重建。typed 与 legacy 恢复路径仍并存 | `crates/kanzei-core/src/store/`、`crates/kanzei-app/src/projection_gate.rs` |
| 子代理 | task 支持持续团队：持久 ID、消息、继续/停止/重启、独立可写工作树、diff/adopt；自定义 Markdown persona 与步数保留，协作快照包含 children。应用重启后不自动重放写操作 | `crates/kanzei-tools/src/team/`、`crates/kanzei-app/src/agent_team.rs` |
| 回退与分叉 | 用户消息旁 ↶：对话/代码/两者回退、外部变化预览与留证后强制还原、对话前缀分叉。Shell/Git/tracker/服务副作用不自动撤销，分叉不复制原工作树代码 | [入口与边界](reports/2026-10-02-harness-repair.md) |
| 定时唤醒 | app、Windows system 与 server 注册/执行；独立记录、同槽去重、过期定义保护、超时和三次失败停用。Windows 原生计时触发与文件回写已实测，真实 SSH 服务端尚未端到端验收 | `crates/kanzei-tools/src/schedules/`、`crates/kanzei-app/src/schedules.rs` |
| 文件与结果保护 | 会话/子代理分开的读取收据；写前及改名前核对现有内容；普通工具结果超过 32 KiB 归档完整原文并保留头尾/回取指针。已修复 FileLock 超时重入死锁 | `crates/kanzei-base/src/atomic_file.rs`、[Harness 修复](reports/2026-10-02-harness-repair.md) |
| 交互式命令 | bash interactive 保持 stdin，process input 写入/关闭输入，归属及时间预算检查；目前为管道输入，没有完整终端控制台 | `crates/kanzei-tools/src/bash.rs`、[Harness 修复](reports/2026-10-02-harness-repair.md) |
| 网页检索 | Codex 搜索有限重试/冷却，失败回退 DuckDuckGo，查询并发有界、来源与失败可追踪。本地故障模拟通过，不能据此保证外部服务实时可用 | [Harness 修复](reports/2026-10-02-harness-repair.md) |
| 后台验证 | 冻结源码快照与独立 worker 已接线；不等于完整四角色独立裁决体系 | `crates/kanzei-tools/src/verification/`、`crates/kanzei-app/src/verification_monitor.rs` |
| 研究存储 | 新课题有独立存储根和登记表；旧项目中的课题原地登记。不能把所有课题都描述为当前开发项目的子目录 | [研究库](design/research_library.md) |
| 研究对象 | topic 下是探索，探索下挂实验结果；路线图是 Markdown 的只读投影 | [实验运行合同](design/research_experiment_runner.md) |
| 研究参数 | 通用 runner 原样记录 params；AUTO 完整实验另要求 experiment_id/role/seed 等协议并核对实验矩阵 | `crates/kanzei-tools/src/research_workflow/lifecycle.rs` |
| CI | 当前 workflow 仅由 workflow_dispatch 手动触发；不能依赖每次 push 自动兜底。CI 与本地 ESM smoke 启动参数已同步 | `.github/workflows/ci.yml`、`scripts/verify.ps1` |
| 正式发布 | package 校验 HEAD 绑定的全量验证证据与提交范围；具体检查清单以 verify-policy 和脚本为准，不固定为旧文档的“十步” | `scripts/verify-policy.mjs`、`scripts/verify.ps1`、`scripts/package.ps1` |

当前接线与后续设计分开阅读：

- [决策台接线](design/decision_console_integration.md)记录已经接入的决定、复核、偏好与 worker 路径及其验证范围。
- [工作台优先](design/workbench_first.md)、[Softwire 操作台](design/softwire_context_workbench.md)、[总体重构](design/softwire_runtime_refactor.md)仍是后续方案；独立原型使用示例数据，不能作为正式运行器的完成证明。
- [完整代码地图](reports/2026-10-02-code-map/README.md)保留实现导航；网页九项整理已落实，默认待整理为零；“已整理”可筛选九项改动和验证。旧问题记录仍按用户反馈归档，个人备注保留。本轮实现见 [整理验收](reports/2026-10-02-web-cleanup.md)。

文档中的历史测量、历史测试、旧提交只说明当时版本。新的事实校正不自动改变 tracker 状态，也不把 draft 决策转为 accepted。

工作目录清理和恢复位置见 [本次清理记录](cleanup-2026-10-03.md)。

D-772、D-773、D-774 的具体修复和验收见 [自举修复记录](reports/2026-10-02-bootstrap-fixes.md)。

桌面与独立后台按编入二进制的构建标识握手；升级时等待旧任务结束，再退出旧后台并连接新版本。当前发布仅支持 Windows x64 NSIS，CLI 与桌面端同批构建，重复打包复用提交时间标识。细节见 [桌面热修正](reports/2026-10-03-desktop-hotfix.md)。

底层工作树已整合，后续后台基线与恢复失败路径修复见 [整合与运行状态修复](reports/2026-10-03-integration-and-runtime-fixes.md)。本轮底层到前端状态审查已统一验证；最新用户授权先发布已完成范围，实际发布版本与下载校验以版本页和发布回执为准。详见 [文件审查发布解读](reports/2026-10-03-file-audit-release.md)。
