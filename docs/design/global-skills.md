# 全局 Skills

Skills 是独立的全局模块，入口位于左侧收起侧栏按钮正下方。管理页不会创建对话，也不会向当前对话插入消息。主对话不再显示技能选择器或逐对话绑定菜单。

## 使用与管理

管理页支持搜索、按来源筛选、全局启停、查看指令、创建、编辑、文件夹导入、复制和删除。AI 生成使用全局主模型，先返回可编辑的草稿，用户保存后安装。生成失败、格式错误或响应截断时保留输入并显示错误。

内置 9 个基础技能：技能创建、代码审查、问题调试、网页设计、资料研究、Word 文档、电子表格、演示文稿、PDF 处理。它们提供工作流程与验证指导，文件制作使用当前运行环境可用的工具。内置技能可停用或复制为自定义，应用更新维护其正文。

目录及状态统一由 `kanzei-harness::skills` 读取。个人技能保存为 `<KANZEI_HOME>/skills/<name>/SKILL.md`，未设置环境变量时使用 `~/.kanzei`。内置文件放在 `builtin-skills/`，启停状态写入 `skills.json`。个人目录、内置目录和兼容的用户级 `.agents/skills`、`.claude/skills`、`.codex/skills` 在所有项目及无项目对话中使用同一清单。项目目录不参与发现；已有项目技能可通过导入加入全局库。

只将启用技能的名称、描述和文件路径加入自动选择清单，匹配任务后按需读取正文。手动调用仍支持 `$skill-name`，遵守全局停用状态和原技能的调用规则。旧的逐对话绑定事件不再影响运行。修改设置从下一次运行生效。

导入与复制保留 `scripts/`、`references/`、`assets/` 等资源。编辑保留原 YAML 中的其他字段，并检查内容版本，避免覆盖另一窗口的新修改。删除将个人技能移入 `skill-trash/`；Windows 使用目录外的写锁，保证目录移动可以完成。

设计参考：[OpenAI Skills 文档](https://learn.chatgpt.com/docs/build-skills)中的渐进加载、技能创建与标准目录结构，以及 [Claude Skills 管理](https://support.claude.com/en/articles/12512180-use-skills-in-claude)中的独立管理、启停和内置文档技能。内置正文由 Kanzei 编写。

## 验证记录（2026-10-04）

- 9 个内置 `SKILL.md` 通过 skill-creator 格式校验。
- `cargo test -p kanzei-harness -- --test-threads=1`：204 项通过。
- `cargo test -p kanzei-app skills::tests -- --test-threads=1`：3 项通过，覆盖全局启停、显式调用、编辑冲突、资源导入和删除。
- `node scripts/ui-skills-smoke.mjs`：18 项通过，涵盖位置、跨对话状态、生成与保存、失败反馈、中英文及窄屏；没有浏览器运行时错误。
- `node scripts/ui-skills-native-smoke.mjs target/debug/kzapp.exe`：18 项通过，使用真实 WebView2、Rust IPC 和文件存储，模型由本地 HTTP 服务返回测试响应。验证生成不立即安装、不新增对话，导入保留资源与 YAML，拒绝过期编辑，重启恢复状态；实际对话收到显式调用的技能正文，停用技能不会进入发现清单。
- 开发目录在 2026-10-04 的全量测试尚未通过：桌面端 600 项通过、3 项失败，涉及并行修改中的会话契约、构造点和锁检查。当日结果不代表发布门禁通过。
- 当日并行线路、无障碍静态、i18n、Markdown 和运行时检查通过；弹层浏览器检查报告 52 处子代理输入栏问题。该记录保留为开发快照。

浏览器记录：`output/playwright/skills/checks.json`。桌面记录：`output/playwright/skills-native/checks.json`。全量日志：`output/playwright/skills/workspace-tests.log`。

## 发布整理（2026-10-05）

发布使用已验证的 `origin/main` 基线 `2e1777b4`，在独立工作树中只迁入上述 Skills 实现和关联回归；保留已经发布的三档子代理实现，不带入开发目录的其他未完成改动和运行数据。

Skills 浏览器回归已接入正式 `ui_runtime` 门禁。最终提交须通过 `scripts/verify.ps1 -Full`，发布后独立核对安装器大小、SHA256 和 HTTP Range。实际验证及发布回执保存到 `output/release-skills-20261005/`，以绑定发布提交的 `verification.json` 和 `release-receipt.json` 为准。
