# Requirements

## R-018 对话结束时播放提示音并显示完成提示 [todo]
- priority: P3
- 归属: kanzei
- 验收: 成功/失败/停止均提示;失焦可感知;通知失败不影响对话结果

## R-024 输入体验:提示词历史(上下箭头)、@文件引用补全 [todo]
- priority: P2
- 归属: kanzei
- 备注: 粘贴/拖拽附件已随 R-014 完成,本条剩输入框体验

## R-025 权限规则管理:设置页查看/删除已记住的放行规则 [todo]
- priority: P2
- 归属: kanzei

## R-030 进程与项目解耦:多进程并行,每进程独立模型选择与子代理开关 [todo]
- priority: P0
- 归属: Claude
- 设计: docs/design/r030-process-decoupling.md
- 验收: 多进程可并行运行,各自拥有模型选择与子代理开关;前端以进程页签呈现;默认进程兼容既有历史
- 备注: 大手术,与 R-037 的渲染层重构一起做

## R-031 子代理轨迹透视:task 块可展开子代理完整工具轨迹,后台面板历史可回看 [todo]
- priority: P1
- 归属: kanzei
- 验收: task 块可展开查看子代理完整工具轨迹,后台面板条目可回看,不因短时超时消失

## R-032 队列可视化:排队输入列表(内容/交付方式)+ 单条撤销 [doing]
- priority: P1
- 归属: kanzei
- 验收: 运行中的 queue 输入显示内容与交付方式,支持单条撤销,状态与后端 admission 同步
- 进展: 开始检查后端 admission 队列事件、停止/取消接口和桌面端运行状态，优先落地队列可视化与单条撤销的最小闭环。

## R-034 research 模式前端:来源/发现侧边栏、引用跳转、报告入口 [todo]
- priority: P2
- 归属: kanzei
- 验收: research 模式显示来源/发现侧边栏,引用可跳转,支持报告入口,与后端 sources/findings 对齐

## R-035 diff 查看器升级:语法高亮、并排视图、多文件改动汇总 [todo]
- priority: P3
- 归属: kanzei
