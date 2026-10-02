# 手机 Markdown 阅读器：后台验证与自主决策实验

日期：2026-09-27。用户授权把现有手机阅读器作为实验项目，检验 Kanzei 的多任务推进、验证解耦和自主决定。代码尚未发布，也未替换安装版。

## 实际完成的闭环

阅读器真实源码 → 内容指纹快照 → 后台验证 → 第一项仍运行时领取第二项 → 同一构建资源排队 → 通过后解锁下游。真机 OCR 单元独立保留设备阻塞，其余工作继续。

最新台账：[`output/reader-lab-06052e3f/result.json`](../../output/reader-lab-06052e3f/result.json)。来源为 `C:\Users\kanzei\Desktop\MD文件保存`，实验使用独立项目根，不改原 R-001 执行模型或批次。

| 场景 | 实测结果 |
| --- | --- |
| 提交数据测试后继续工作 | 提交 378 ms 返回；第一项仍在运行时 `work next` 选中独立 W2 |
| 第二项界面测试 | 提交 407 ms 返回；与 W1 共用 `flutter-reader-lab` 资源，排队后运行 |
| 数据与分析 | Windows 上 ASCII 快照执行 `flutter analyze --no-pub` 无问题，20 项单元测试通过 |
| 阅读界面 | 2 项 widget 测试通过，合计 22 项 Flutter 测试 |
| 依赖与设备条件 | W4 在 W1/W2 通过前不可领取，通过后可领取；W3 真机 OCR 继续单独阻塞 |
| 取消与 worker 崩溃 | 真正启动专用 worker，分别取消和终止 worker；子进程结束、无成功证据、其他单元仍可领取 |
| 快照、失败和迟到结果 | 7 项 Rust 验证测试覆盖源码隔离、局部失败、取消 / 被替代、源码被测试修改、孤儿恢复、事务回滚、超时 |

378 / 407 ms 是本次提交延迟，不是测试总时长，也不代表任意仓库的快照成本。源文件指纹为 `ba567aed906cd6e90c3e7521dec0fd237f2c63ef14f50a117ea24324d9eceeb6`，覆盖当时已有未提交内容和本轮新增测试。

本次保留的日志：

- 数据 / analyzer：`C:\Users\kanzei\AppData\Local\kanzei\verification\v-43d5c7e8437f04b67b0610720da2e8aa\output.log`
- 界面：`C:\Users\kanzei\AppData\Local\kanzei\verification\v-7e1078925cce71dae153b09a816464d1\output.log`
- 进程生命周期：[`worker-lifecycle.json`](../../output/reader-lab-814f6b2a/worker-lifecycle.json)。这是存储路径调整前的实测，文件中保留当时 TEMP 日志地址。

复跑脚本为 `scripts/reader-verification-lab.mjs`、`scripts/verification-worker-smoke.mjs`。前者读真实阅读器源码，用真正的 kz CLI 生成工作单元和验证；后者只操作实验台账创建的专用 worker。

## 实验发现与修复

1. **“后台”命令实际仍等测试结束。** 首次真命令提交耗时约 29 秒，新增的“第一项必须仍在运行”断言失败，暴露了假异步。Windows 子进程继承调用方捕获管道的句柄，即使标准流设为 null，父调用仍等不到 EOF。改用禁止句柄继承的原生创建流程后，两次提交稳定在本机约 0.35–0.41 秒。该次失败没有计为并行成功。Windows 接口依据：[Microsoft process inheritance](https://learn.microsoft.com/en-us/windows/win32/procthread/inheritance)。
2. **附件可挂到不存在的文档。** 新增的无父文档场景复现成功，原件和数据库记录变成无入口的孤立附件。表有外键声明，但 AppDatabase 未在连接时启用约束，旧迁移测试单独开启约束掩盖了差异。已在 `beforeOpen` 启用外键；服务原有失败清理路径随即生效，回归测试确认无孤立数据库行和原件。登记为阅读器 D-010。
3. **CLI 无自动决策入口。** 原装配固定使用 Interactive，补 `--autonomous` 连接共享协议；只读 agent 的“完全不能修改任何东西”提示又让真实模型拒绝调用 question，已明确允许 runner 的决策审计记录，源文件权限保持只读。

新增 7 个附件场景：取消选择、不支持格式、中途读取失败、同名 / 大写扩展名、空 OCR、删除附件清理搜索与原件、无父文档。连同已有 PDF 提取、OCR 失败保留、迁移、搜索与界面测试一起通过。

依赖准备时，既有 `flutter_secure_storage` 声明尚未解析，离线实验先失败。当前进程的旧代理阻止 Pub 访问，使用进程内直接连接完成既有依赖解析，更新 `pubspec.lock`；后续快照实验均离线恢复依赖。没有改变全局代理或 Android SDK。原中文工作目录 analyzer 曾有 LSP 解析错误，本报告的通过证据来自 ASCII 快照。

## 自主决策真实模型场景

脚本 `scripts/reader-autonomous-lab.py` 使用已配置模型，独立项目根与独立 Kanzei 全局运行目录，避免实验记录进入用户真实偏好库。测试先发送不含 decision 的旧式提问，再读取附件实现，补充决定并继续。详细结果写在实验目录 `decision-model.json` 与 `decision-model.log`，首次只读提示冲突保存在 `decision-model-before.*`。

修正后真实模型 `codex:gpt-6-luna` 在 4 个模型步骤、约 23.7 秒内完成：首次 question 记录 deciding；读取真实附件实现；第二次 question 将同一决策变成 decided；给出结论并结束。默认选项被设为“覆盖旧附件”，模型根据实现选择“保留两份原件”，保存理由和影响。没有等待用户输入，review 仍为空，没有将决定冒充用户认可。原始事件见 [`decision-model.json`](../../output/reader-lab-06052e3f/decision-model.json)。这是一个短场景的真实模型验证，不外推为长期自主开发或偏好采用已验证。

## 与现有系统的接入

复用 Bash 权限入口、Work Unit 事件、project root 归属与 workspace_snapshot，不增加另一套项目或工作状态数据库。后台验证只释放执行占用，未通过的依赖不会提前解锁。原同步验证仍可用；人工接受交付仍是单独记录。

结果和证据以事务提交；取消、崩溃、超时及迟到结果不能伪造完成。App 的真实 IPC 服务测试覆盖取消命令，前端真实浏览器通过模拟 IPC 验证详情与动作归属。Windows worker 崩溃后的子进程收口有真实进程证据；其他操作系统未实测。

本轮运行了 core 337 项、tools 669 项通过 / 1 项忽略、app 379 项回归；随后新增事务用例及小修改的定向验证为 7 项后台验证测试、49 项 CLI 测试，以及真实 App 服务契约和 UI 浏览器回归。前一批完整证据见 [第一批报告](2026-09-27-decision-console-integration.md)。

## 尚未覆盖

- 真实手机 OCR、iOS 构建、Android 新构建、阅读器 DeepSeek Agent 与完整备份交付。本轮未进行 SDK 或许可操作，原阅读器 R-001 仍为 2/5。
- 所有单元都在等后台验证时的自动休眠和完成后唤醒；闲置原线收到用户纠正后的自动启动。
- 长时间多项目运行、后续真正采用用户偏好、可直接启动的产物入口、安装版验收。

当前提供的是已通过实际阅读器实验的源码闭环，不将测试通过等同于手机端或整套产品交付完成。
