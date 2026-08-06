# Defects

## D-030 R-035 diff 查看器新增结构缺少对应样式与行号回归覆盖 [fixed] (medium)
- 复现: 打开活动面板查看 write/edit diff，检查新生成的 diff 结构及多文件汇总。
- 影响: 升级后的统一/并排视图及语法高亮无法达到预期视觉效果，后续改动容易回归。
- 现象: 前端已生成 diff-file-header、diff-row、diff-split-row、syntax-* 等节点，但 style.css 尚无对应规则；后端 diff 行号与多行变更行为也没有测试覆盖。
- 计划: 补齐 CSS，并为 diff_display 增加结构化字段、语言识别、行号和截断测试。
- 优先级: P1
- 修复: 补齐 diff 查看器及多文件汇总 CSS；新增 diff_display 结构字段、语言识别、行号与大输出截断测试。
- 验证: cargo test -p kanzei-tools、cargo test --workspace、node --check crates/kanzei-app/ui/main.js 均通过。
