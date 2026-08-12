# research 模式设计:从网络调研壳到「先计划后自举」的勘察载体

- 状态: 设计基线草案(2026-08-12,八维度审计维度8 产出;定调点待用户逐项确认后转正)
- 实施条目: R-221(分批见 §6)
- 前置文档: [interaction_modes.md](interaction_modes.md)(模式体系)、harness_m1.md §5(research 原始承诺)、[memory_control_plane.md](memory_control_plane.md)(记忆边界)、[parallel_read_serial_write_orchestration.md](parallel_read_serial_write_orchestration.md)(SCOUT_ROLES)
- 审计证据: [audit_20260812_eight_dimensions.md](audit_20260812_eight_dimensions.md) §8

## 0. 现状定性(全部有证据)

**骨架早已完整,但形态错位且零使用。**

- 骨架存在:独立 ResearchProfile(profiles.rs:536-624)、S-/F- 来源与发现 DocKind(docstore.rs:65-89)、websearch/webfetch 放行、写权收窄 `.kanzei/research/**`(含 `..` 穿越防护)、前端来源/发现页与 report.md 入口。
- 形态是**网络调研**:agent 系统提示只讲「记来源、findings 引用来源、报告写 report.md」,专属工具面(source/finding/websearch)全部面向外部资料。
- **零使用**:state.db 266 条 episodes 零调用 websearch/source/finding;processes 表只有 dev;`.kanzei/research/` 全部 git 历史只有一个空模板 memory.md,sources.md/findings.md/report.md 从未产生过。
- 真实的研究(代码勘察)全部发生在 dev/dev-auto:模型自派只读 task 子代理(R-200 进展是活例),或 phase_pipeline 的 SCOUT_ROLES 编排派发。两者的结论都**没有固定工件落点**:勘察简报只存活在当轮对话里,勘察报告被 D-294 单行不变式折成数百字单行塞进 tracker 进展字段(R-200 的进展字段就是实物)。
- 证据等级**双重语义**:E0-E4 在验证体系文档里定义为测试证据等级(E1=单元测试),条目里却被挪用为勘察证据口径(E1=读码核实),conventions 无定义、代码零校验。
- `.kanzei/research/memory.md` 是绕开记忆控制平面的第二套自由文本记忆:自我声明「无来源不得注入」,注入实现却原样读文件截 5000 字符,无任何校验。
- harness_m1.md §5 承诺 `.kanzei/research/<topic>/` 按主题分目录,实现是全局平铺单文件——两个课题互相覆盖。
- readonly 档位桌面端不可达(装配线不注册 ReadonlyProfile),档位矩阵需一并收口。

## 1. 重定位(核心命题)

**research 模式 = 「先计划后自举」工作流的正式载体。** 主形态从网络调研改为**代码库勘察**:产出带 file:line 证据的勘察报告与结构化发现,网络检索(websearch/webfetch)降级为辅助工具。研究的终点不是报告本身,而是**可被 dev 轮直接消费的计划**——设计文档草稿、需求/缺陷草稿、以及可回溯的证据链。

## 2. 定调点(逐项待用户确认;括号内为本设计的默认建议)

1. **主形态**=代码库勘察,网络调研为辅(建议:是)。
2. **工件落点**=`.kanzei/research/<topic>/report.md`,兑现 harness_m1 §5 的 topic 维度;tracker 条目进展字段只写一行摘要+报告路径引用(建议:是——这同时是 D-276/D-294 一系的根治面:多行内容有了合法去处,进展字段回归单行摘要)。
3. **勘察证据等级单列 V 表**,与验证体系 E0-E4 彻底分家:V0=目录/命名推测、V1=读码核实(file:line)、V2=运行时实测、V3=用户复现。写进 conventions,tracker 条目「证据等级」字段按新口径标注(建议:是;存量条目不回改,新条目起用)。
4. **回流通道**:research 档注入 backlog 只读索引与 conventions、放行 memory_search,并提供 finding→req/defect 的草稿转化(给 research 档 req/defect 的 get+add 子集,add 产物默认 [todo] 待 dev 轮确认)(建议:是)。
5. **记忆一元化**:废弃 research/memory.md 的自由文本注入,研究结论统一走 memory_note→manager 晋升;来源约束复用 validate_source_refs 的 S-/F- 硬校验(机制已在 memory/mod.rs:255-296)(建议:是)。
6. **档位矩阵**:research=只读勘察档——read/glob/grep/files/git 只读/webfetch/websearch 放行,write 仅 `.kanzei/research/**`,bash 硬 deny 并带替代指引(复用 ReadonlyProfile 的 managed hard-deny 手法,profiles.rs:652-658 先例)。readonly 档与 research 并存:readonly=纯只读,research=只读+研究工件写权;桌面端补注册 ReadonlyProfile 或在文档明示 CLI-only(建议:并存,桌面补注册)。
7. **research 是否可写 docs/design/*.md**:不可。设计文档草稿落 `.kanzei/research/<topic>/`,经用户或 dev 轮验收后转正到 docs/design 与 backlog——转正是 dev 的活,保住「research 无副作用」的档位语义(建议:是)。
8. **三形态收敛**:research 模式、模型自派 task 勘察、SCOUT_ROLES 编排勘察,三者产出同一工件格式、同一证据口径、同一落点(后两者可选落盘)(建议:是,分批最后做)。

## 3. 工件设计

```text
.kanzei/research/
  <topic>/                 # kebab-case 课题名,如 r206-frontend-state
    report.md              # 勘察报告(多行 markdown,自由结构)
    sources.md             # 外部来源(S-,仅涉外部资料时)
    findings.md            # 结构化发现(F-)
```

report.md 契约(轻约定,不做 schema 校验):

- 头部:课题、日期、关联条目(R-/D- 列表)、总体证据等级(V 表取最低)。
- 每条结论:**结论一句话 + 证据锚(file:line 或 S-id)+ V 等级**。无锚结论必须显式标 V0。
- 结尾:「建议登记」段——可直接改写成 R-/D- 条目的草稿(含验收思路)。

tracker 衔接契约:条目进展字段只写 `一行摘要 + 报告路径`;refs 可引用 topic 名。这条写进 dev 与 research 两边的提示词登记契约。

## 4. 档位与工具面(目标态)

| 能力 | research(目标) | 现状 | 变更 |
| --- | --- | --- | --- |
| read/glob/grep | 放行 | 放行 | 不变 |
| files(目录地图) | 放行 | 无 | 新增 |
| git(status/diff/log 只读子命令) | 放行 | 无 | 新增 |
| webfetch/websearch | 放行 | 放行 | 不变 |
| source/finding | 放行(挂 topic) | 放行(平铺) | 改造 |
| write | 仅 .kanzei/research/** | 同 | 不变 |
| bash | 硬 deny+替代指引 | deny(无指引) | 改造 |
| req/defect | get + add(产物 [todo]) | 无 | 新增 |
| memory_search | 放行 | 无 | 新增 |
| memory_note | 放行(替代 memory.md) | 无 | 新增 |
| 上下文注入 | research-docs + backlog 只读索引 + conventions | 仅 research-docs | 新增 |

## 5. 与既有系统的关系

- **interaction_modes.md**:前端模式三选一(结伴/自主/research)不变;research 下连跑仍禁用(研究不自动推进,推进是 dev 的事)。
- **memory_control_plane.md**:研究结论进记忆走统一管线(定调 §2.5),不开第二套;S-/F- 引用作为记忆 provenance 的合法来源类型。
- **phase_pipeline(SCOUT_ROLES)**:保持轮内简报职责不变;批6 给它可选的「落盘为 research 工件」出口,使勘察结论可被条目 refs 引用。
- **D-276/R-201(游离文本)**:report.md 是多行内容的合法落点,tracker 字段回归单行摘要——从源头减少往进展字段塞报告的动机。

## 6. 分批实施(R-221)

- 批1 档位收口:桌面注册 ReadonlyProfile(或明示 CLI-only);research 档 bash 换硬 deny+替代指引;工具面加 files/git 只读。验收:桌面/CLI 档位表一致,research 会话内 bash 被拒时有替代指引文案。
- 批2 topic 工件:source/finding/report 落 `.kanzei/research/<topic>/`;前端研究页按 topic 分组。验收:两个课题的工件互不覆盖,report 可从条目 refs 跳转。
- 批3 证据口径:V 表写进 conventions;tracker 新条目按 V 标注;dev/research 提示词同步。验收:新条目标注可查到权威定义,E/V 两表互不混用。
- 批4 回流通道:backlog 只读索引+conventions 注入;req/defect get+add 子集;finding→草稿动作。验收:一次研究会话能引用 R- 条目并产出一条 [todo] 草稿。
- 批5 记忆一元化:memory_search/memory_note 进 research 档;memory.md 停止注入(文件保留为历史)。验收:research 档无第二套无校验记忆注入点。
- 批6 三形态收敛:SCOUT_ROLES 简报与 task 勘察结论可选落盘为同格式工件。验收:三种形态产出同一格式落同一落点,全仓只有一套勘察工件写入口。

## 7. 验收总则(整条 R-221 的终局)

用 research 模式对一条真实 R- 条目完成一次现状勘察:产出 `<topic>/report.md`(带 V 等级与 file:line 证据)→ 条目 refs 引用它、进展字段只有一行摘要 → 报告的「建议登记」段转成正式条目 → dev 轮按报告实施——**勘察→计划→登记→自举执行的完整链路有轨迹可查**。

## 8. 边界(本设计不做)

- 不做全文检索/知识库引擎;不做勘察报告的 schema 校验(轻约定+提示词纪律)。
- 不改 dev 模式的取活纪律与工具面;不动 E0-E4 在验证体系里的语义。
- research 不可写 docs/design、不可提交 git、不可动 tracker 既有条目的状态(add 草稿除外)。
- 不为 research 单独造记忆存储——一切走既有控制平面。
