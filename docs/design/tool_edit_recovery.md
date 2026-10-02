# 工具编辑恢复与结果语义

## 背景

`edit` 的安全门禁会拒绝三种常见调用：插入形状却覆盖锚点、明显净删除但未确认、`old_string` 与 `new_string` 相同。旧链路把这些预期拒绝与磁盘写入失败统一压成 `is_error=true`，导致 UI 全部显示红叉、运行指标虚高，并把保护门禁错误地沉淀成失败记忆。复合测试命令和 tracker `add` schema 另有两个恢复摩擦：覆盖面只认最后一条记录，必填字段只在执行后报错。

## 结果契约

`ToolOutput` 保留 provider 协议所需的 `is_error`，并新增正交的机器终态：

- `success`：执行并生效；
- `noop`：调用合法但没有工作可做；
- `needs_correction`：参数或锚点必须重建后重试；
- `needs_confirmation`：操作具有明确删除风险，需显式确认；
- `failed`：I/O、权限、运行时等真实执行故障。

受控拒绝继续以 tool error 回喂模型，防止模型把未落盘误判为成功；同时携带稳定 `code`，供 UI、指标和恢复策略使用。只有 `failed` 进入 `failed_calls`、失败信号和在线失败召回。旧轨迹没有结构化头时保持原有统计语义。

## 编辑恢复

`edit` 专用于精确替换，`insert` 专用于在唯一锚点前后追加文本。`insert` 的锚点只参与定位，生成结果时始终原样保留；文本按调用方提供的换行原样插入。两个工具都容忍 CRLF/LF 差异，并在第一次缺失或非唯一锚点时返回带行号的实际文件片段。

恢复映射固定为：

- `EDIT_INSERTION_WOULD_REPLACE_ANCHOR`：改用 `insert`；
- `*_ANCHOR_NOT_FOUND` / `*_ANCHOR_NOT_UNIQUE`：重新读取并重建锚点；
- `EDIT_IDENTICAL_INPUT`：停止重试，重新判断是否仍有未完成工作；
- `EDIT_NET_DELETION_REQUIRES_CONFIRMATION`：确认确需删除后才设置 `allow_deletion=true`。

## 相关修复

- 测试记录解析 `;`、`&&`、`||` 连接的复合命令；同一源码指纹下的 passed 记录按 crate 覆盖面取并集，workspace 覆盖优先。
- requirement/defect/finding 的 `add` schema 通过条件 `required` 暴露真实必填字段；requirement 可直接传顶层 `complexity`、`tag`，defect 可直接传顶层 `severity`、`priority`、`tag`。
- 中/大型改动在首次写入前输出四项设计冻结：不变量、权威数据源、预计修改文件、最小测试；只有新读码或测试证据推翻事实时才修订。

## 验证边界

定向测试覆盖结构化结果、编辑/插入保护、指标与召回过滤、复合测试命令、同指纹覆盖并集、tracker 条件 schema、顶层字段落盘、设计冻结提示。前端运行时冒烟验证成功/no-op/受控拒绝/真实故障四态渲染。正式发版仍须运行 `scripts/release.ps1` 的完整门禁并核对安装后的 `kz --version` 与目标提交一致。
