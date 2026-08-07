# Goals

## G-001 把 kanzei 打磨成日常主力开发工具 [active]
- 类型: 长期
- 说明: 方向性目标,不主动关闭;好用压倒一切(上下文透明/多agent协作/快/少打断/信息清晰)
- 进展: D-050 已关闭。D-051 继续 fixing：CLI 真实 AlwaysAllow→本地 SSE→bash→marker E2，桌面失败/成功局部回归，旧裸 bash 规则识别并在 CLI/桌面提示降级 Ask；桌面真实 UI E2 因无前端 harness 暂缓。D-054 继续 fixing：runner 拒绝权限补齐当前/后续 ToolResult，保留同批真实结果；CLI 真实 E2 覆盖拒绝后第二次对话恢复；新增 kanzei-core 共享 history filter，接入 CLI prior、桌面 recover_messages_at/conversation_get；抽出桌面 conversation_prior，验证内存历史与持久化快照交界。core/app/CLI 回归持续通过。
- 当前判断: 距离"日常主力开发工具"的差距不在功能数量而在可靠性与验收质量——上一轮的验证手段几乎清一色是语法检查,导致带病能力被判定为完成。近期重心应是 R-083(收口 P0 缺陷)、R-084(建立能捕获运行时错误的验收手段)、R-085(完成判定的执行约束),之后再谈新能力。
