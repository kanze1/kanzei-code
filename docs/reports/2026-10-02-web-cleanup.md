# 网页整理项实现与验收

核对日期：2026-10-02。范围是网页保留的 C01–C09 共九项；用户确认已登记，沿用已有记录。网页的完成记录与个人标注分开保存。

| 项目 | 实际改动 | 定向验证 |
| --- | --- | --- |
| C01 | AUTO 作为研究主控制面；plan/loop 复用其阶段、暂停与预算，旧课题保留兼容入口。 | AUTO 暂停和阶段拦截、旧预算不能覆盖主预算的回归通过。 |
| C02 | 分节写作与 AUTO 使用同一论文工件和编译服务；修复稿件必须重新核验。 | 同一路径写作、数值核验不能绕过、修复后回到核验、旧 PDF 不能冒充新编译的回归通过。 |
| C03 | task 与勘察复核共用子代理执行、超时和取消处理；固定角色与屏障保留为策略。 | 共享超时会取消未完成任务；子代理和流水线回归通过。 |
| C04 | 移除只保存 JSON 清单的容器注册、升级、回滚入口；实际 Agent 配置与快照继续保留。 | 无剩余生产引用；Rust 编译、IPC 契约和 UI 检查通过。 |
| C05 | Vega-Lite 为默认绘图入口；PGFPlots 和 matplotlib 抽到显式选择的适配器。 | 未知引擎和未选择适配器的输入被明确拒绝；绘图回归通过。 |
| C06 | 记忆专用对话与图谱通过共享加载器首次打开时加载；核心编辑和检索继续常驻。 | 真实浏览器确认默认未加载、首次对话和图谱入口可用。 |
| C07 | 移动端 UI 从主设置模块拆出，按需加载；KDE Connect 通知单独启用并检查依赖。 | 真实浏览器确认默认不调用手机接口、展开设置才加载；通知默认关闭。 |
| C08 | 语音界面与运行时首次使用才加载；关闭后停止会话归属轮询。 | 真实浏览器确认默认没有语音加载和设置调用，首次检查入口可用。 |
| C09 | 保留轻量状态反馈；复杂视觉统一开关，默认关闭；素材制作脚本移到 extras。 | 真实浏览器确认默认不加载渲染器，开关与关闭销毁通过；既有视觉回归通过。 |

源码保留旧研究课题的兼容数据、真实 Agent 配置、核心记忆编辑和用户的视觉偏好。容器清单仅移除无运行效果的代码和按钮；既有用户文件保留。素材制作目录可单独使用，生产资源继续在 ui/assets/oc。

主工作目录原有未提交改动单独保留。本次在当前源码副本上修改和验证，再按文件指纹合回，未将其它开发中的模型接口功能混入发布范围。测试记录和交付版本在下方随实际验收补入。

当前源码副本验收：cargo test --workspace --all-targets 为 1,974 passed、4 ignored、0 failed；cargo clippy --workspace --all-targets -- -D warnings 通过。网页真实浏览器 18 项检查通过，静态地图的 11,107 个链接与 19 条决策对照通过。

正式版本 `build-59f17d67`（提交 `59f17d6798ed4fb2fff488e0e60b8e1cbe78a5d7`）已发布：[GitHub Release](https://github.com/kanze1/kanzei-code/releases/tag/build-59f17d67)。范围为 build-c5bf3e32 之后一个整理提交；主目录其它未提交开发未发布。15 项 full 验证全部通过，skipped_steps 为空；Rust 测试为 2,143 passed、2 ignored、0 failed，两个既有忽略项分别是弹出测试窗口和文档示例。另跑全目标 Clippy 通过。

安装器 `kanzei-setup-59f17d67.exe` 为 41,952,829 字节，SHA256：`18259183f3b70222ac2c2a0a24d833e0c6214dbe3b4c5c170014f0e1eed4194e`。远端 tag 指向、GitHub 资产大小与摘要、独立重新下载和 HTTP 206 / 单字节 Range 全部核对通过。证据保存在 dist/verification.json、dist/release-receipt.json 与 dist/installation-proof.json。

本机交付：桌面安装位仍是旧版，新版已放入 pending 更新文件，两个 CLI 安装位均已同步；用户下次启动桌面时完成替换。 未启动桌面应用或自举。手机配对实机、语音模型/音频、PGFPlots 与 matplotlib 外部工具链未做本机运行验收；对应代码和模拟/浏览器行为已通过所列回归。

本轮两个临时工作树、临时分支和构建辅助目录在归档核验后移除；主工作目录、共享 target 与原有三个本地分支保留。旧稳定 Release 的元数据和安装器归档后移除旧页面及资产，所有 Git tags 保留。归档位置：`C:/Users/kanzei/Documents/kanzei-archives/2026-10-02-web-cleanup/`，包括 cleanup.bundle、evidence.zip、源码指纹、旧版和新版安装器与验收证据。恢复 bundle 需要仓库已有的 21c36e9d 和 c5bf3e32 基线；此前完整仓库备份仍在 2026-10-02-cleanup/repository.bundle。

2026-10-02 发布保留更新：原 `build-59f17d67` 的 Release 页面及下载资产已校验归档并退役；源码仍可查看 [保留的标签](https://github.com/kanze1/kanzei-code/tree/build-59f17d67)。当前安装版为 [build-2173dd60](https://github.com/kanze1/kanzei-code/releases/tag/build-2173dd60)。原九项整理的完成状态与个人标注保留。
