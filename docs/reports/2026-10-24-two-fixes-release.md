# 2026-10-24 目标地图与图片回复合版候选

## 授权与基线

用户要求将需求目标地图与“回复只能回复文字不能回复图片”的修复结合发版。确认图片问题的作答凭证为 `question:run_1792776910750920000:call_5b769c186f8a45de9160b99c1d9cd20a`。

候选从最新已发布及已安装的 `ff40bbae68de83623ae7335c1addc7488c8ed254` 建立干净临时工作树，分支 `kanzei/release-two-fixes-20261024`。原主根 `dev=b7591aa3` 及其此前未提交源码保留，不把整份混合工作树作为发布输入，不读取被权限拒绝的外部工作树。

## 两项修复

### 图片回复

在原主根的未提交草稿中识别附件转换、问题回复、决策/事实回复及浏览器回归的最小差异，前向移入 ff40。附件保存在原项目的内容寻址目录，通过现有 `read` 图像能力接收；无附件的旧文字回复保持兼容。相同内容重试复用请求身份，附件或问题版本改变后使用新身份；失败与刷新保留完整草稿。

实跑发现并补齐了草稿遗漏：

- 回复态的附件选择按钮原来仍隐藏，现保持可用并按忙碌、只读及完成状态禁用。
- 生成的 IPC 参数清单原来未接收可选 attachments，已同步 `ipc-commands.json` 和 `00-command-contract.js`；沿用原已注册命令。
- 刷新完整草稿所需的 composer.restore 接口原来缺失，已补齐文字与附件恢复。

没有改变权限配置、信任状态或模型协议；也没有把图片内容当成指令。未把终端中被拒绝的额外大小限制测试插入改用其他途径执行。

### 目标地图

语义来源为原提交 `b7591aa3db0577695a42e0abb44f987002176027`，其历史报告仍保留，但旧报告的测试不作为本候选已通过的证据。

- 保留 ff40 默认目标树、旧 `management_work_view` 偏好，以及规划、裂变、调整方向与精确节点推进；增量增加“目标地图”只读视图，与目标树和列表并列。
- 层级归属与依赖分开绘制，共享结果去重，未知引用不推断完成或阻塞。
- 复用目标树的 review 状态及当前结果判据，历史 done 在 pending/reverify/replace/retire 时不再着当前成功色，并展示方向和后继信息。
- 同一验收条款汇总全部证据引用并去重；样式只影响新图，往返不污染原工具栏；样式间距遵循已有 token。
- 新地图回归登记进全量 verification-checks，并增加路径裁剪守护测试。

## 本候选实际定向验证

以下均在 ff40 临时发布树及上述实际修改上执行，并非旧 b759 的回执复用。

| 命令/范围 | 实际结果 |
| --- | --- |
| `node scripts/ui-softwire-choice-smoke.mjs` | 15 项通过：文字+图、仅图、失败保留、重试身份、附件字节变化、问题版本刷新。 |
| `cargo test -p kanzei-app reply_` | 6 项通过，包含图片持久化及 read 返回图像、live/durable答案与去重。 |
| `cargo test -p kanzei-app attachment_` | 2 项通过，包含附件事实回复恢复至原对话。 |
| `cargo clippy -p kanzei-app --all-targets -- -D warnings` | 通过，覆盖新增 Rust 测试的 lint。以上四项最终回执为原主根 `.kanzei/artifacts/test-runs/T-1786922727168.json`。 |
| `node --test scripts/ui-work-map-smoke.mjs` | 3 项通过，其中生产 ESM/Edge 场景 42 项断言，含 review 各处置、全部证据、偏好、旧树样式、响应式及导航。 |
| `node scripts/ui-browser-contracts-smoke.mjs` | 10 个域场景全部通过，其中工作树 63、管理树 22 项断言；同批 verify-policy-smoke 通过。该批随后因新脚本浏览器全局声明遗漏在 lint 失败，不把整批称为通过。 |
| `node scripts/ui-lint-smoke.mjs` | 最终 303 文件 ESLint 与 ESM、间距、hidden 守护全过。初次发现 getComputedStyle 全局声明和 5 处已有 token 写死值，均修正后重跑。 |
| `ui-familiar-workspace-smoke` / `ui-backlog-maintenance-smoke` | 最终 82 / 37 项通过。 |
| `ui-runtime-smoke` | 最终 145 模块、3446 invoke 初始化及 11 个视图切换，0 运行时错误。以上最终前端批回执为原主根 `.kanzei/artifacts/test-runs/T-1786922727170.json`。 |
| `kz metrics --top 30` | 已实际执行并更新观测表 `docs/design/metrics_baseline.md`，未改机器基线或放宽阈值。 |

候选内 `output/playwright/work-map/acceptance.json` 包含地图源文件 SHA-256 及断言，浅/深色宽屏和窄屏截图已实际读取核对。该工件仍明确是生产 ESM + 隔离 IPC，不是原生模型/安装验收。图片浏览器工件同样使用隔离 IPC，Rust 定向用例另行覆盖真实保存、读取与投递逻辑。

## 首次完整门禁的实际阻断与修复

最终两项提交后的首轮 Full 在 `546145a6` 上实际执行：地图、图片回复、原目标树及其余多数检查通过，但 `artifact_lifecycle` 在 Windows PowerShell 5.1 的日志收尾处因 `Get-FileHash` 无法解析而失败；另因前台命令 600 秒时限，在工作区测试仍进行时被终止。此轮无正式全绿证据，不能发布。单独运行原 `artifact-lifecycle-smoke.mjs` 再次复现同一错误（原主根回执 `T-1786922727172`）。

为解除已复现的发布阻断，只将 `scripts/artifact-lifecycle.ps1` 的日志 SHA-256 计算改为 PowerShell 5.1/7 都支持的 .NET SHA256 流式实现，仍针对落盘原始日志字节，并释放资源；没有跳过测试或改动门禁阈值。原生命周期用例重跑 **32 项全部通过**（回执 `T-1786922727173`），覆盖原生退出 0/7、两条输出流及原日志哈希的一致性。正式 Full 必须在包含该修复的新提交重跑，不能复用 `546145a6` 的部分结果。

## 交付边界

本报告提交时只证明两项修复及上述定向回归。最终干净提交仍须执行 `scripts/verify.ps1 -Full`，使用绑定最终 SHA 的正式证据再打包、推送及发布；不使用 SkipTests，不运行 GitHub Actions。安装包、公开 Release、CLI/桌面实际或 pending 安装的版本和哈希分别验收，不强杀运行窗口。正式发布结果以后续版本绑定回执为准，不由本报告预先宣称完成。
