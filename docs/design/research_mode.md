# research 模式设计:从网络调研壳到「先计划后自举」的勘察载体

- 状态: **重写中**(2026-08-14 用户定调否决 §1 原命题——research 是独立深度研究模式,不是「先计划后自举」载体;§2 逐条标注 作废/已定/待重推,详见该节。§3 之后各节按旧命题写成,**未同步**,重推前不得据其实施)
- 排期: 实施在 dev 稳定之后(2026-08-14 用户定调);本轮只定性不开工
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

## 1. 定位(核心命题,2026-08-14 用户定调)

**research 模式 = 独立的深度研究模式,不是「先计划后自举」的载体。**

- **主形态**=深度分析调研(autoresearch 形态):对**文献**与**仓库**两类对象做深度检索、交叉验证与综述。
- **产出物**=论文级工件:正文、LaTeX 源码、图表、参考文献。研究的终点是可发布、可归档的成果本身,不是给 dev 轮消费的计划。
- **网络检索(websearch/webfetch)是主力工具**,不是辅助。
- **绝对独立**:不与 dev 侧的代码库勘察合并。模型自派 task 勘察与 SCOUT_ROLES 编排勘察是 dev 的能力,各归各,不收敛到本模式。

> **作废声明(2026-08-14)**:本节此前版本(2026-08-12 草案)主张「research = 先计划后自举的正式载体,主形态从网络调研改为代码库勘察,网络检索降级为辅助工具」。该命题已被用户明确否定,连带 §2 定调点 1 与 8 作废。
>
> 需要留意:dev 侧「先计划后自举」的**勘察工件无固定落点**这个问题(§0 记录的现状——勘察结论只活在当轮对话里,或被 D-294 单行不变式折进 tracker 进展字段)**依然存在且未解决**。它是 dev 的课题,需另立条目承接,不再由 research 模式代管。

## 2. 定调点(2026-08-14 用户逐项过审;状态见每条行首标记)

1. ~~**主形态**=代码库勘察,网络调研为辅~~ —— **【作废】** 用户定调 research 为独立深度研究模式,见 §1。
2. **【待重推】工件落点**:原案 `.kanzei/research/<topic>/report.md`,兑现 harness_m1 §5 的 topic 维度;tracker 进展字段只写一行摘要 + 报告路径引用(顺带根治 D-276/D-294 一系:多行内容有了合法去处,进展字段回归单行摘要)。**动机成立**,但目录结构须按新定位重推——论文形态要容纳 `paper.tex`、`figures/`、`refs.bib`,不是单个 report.md。
3. **【已定 2026-08-14】勘察证据等级单列 V 表**,与验证体系 E0-E4 彻底分家:V0=目录/命名推测、V1=读码核实(file:line)、V2=运行时实测、V3=用户复现。写进 conventions,tracker 条目「证据等级」字段按新口径标注;存量条目不回改,新条目起用。**待扩**:现四档全是代码调查口径,文献调研需要另一套等级(一手文献 / 同行评议 / 二手引用 / 预印本 等),扩展方案随 §1 重定位一并给出。
4. **【待重推】回流通道**:原案 research 档注入 backlog 只读索引与 conventions、放行 memory_search,并提供 finding→req/defect 的草稿转化(get+add 子集,add 产物默认 [todo] 待 dev 轮确认)。research 独立后,该回流是否仍属本模式职责需重新判断。
5. **【待复核】记忆一元化**:废弃 research/memory.md 的自由文本注入,研究结论统一走 memory_note→manager 晋升;来源约束复用 validate_source_refs 的 S-/F- 硬校验(机制已在 memory/mod.rs:255-296)。大概率仍成立,但需按新定位复核——论文级工件的**引用管理**(refs.bib)与**记忆晋升**是两件事,不能混为一谈。
6. **【待重推,已知冲突】档位矩阵**:原案 research=只读勘察档——read/glob/grep/files/git 只读/webfetch/websearch 放行,write 仅 `.kanzei/research/**`,**bash 硬 deny** 并带替代指引(复用 ReadonlyProfile 的 managed hard-deny 手法,profiles.rs:652-658 先例)。**与新定位直接冲突**:产出 LaTeX 要跑编译(pdflatex/tectonic),出图表要跑绘图,bash 全禁即做不了。重推方向二选一——(a) bash 限定为 `.kanzei/research/**` 内的编译/绘图命令白名单;(b) 提供专用 latex/plot 工具通道(与 architecture/conventions 专用写通道同一手法,合法路径必须可达)。「readonly 与 research 并存」「桌面端补注册 ReadonlyProfile」两条不受本冲突影响。
7. **【待重推】research 是否可写 `docs/design/*.md`**:原案不可,草稿落 `.kanzei/research/<topic>/`,经用户或 dev 轮验收后转正。新定位下 research 产出的是论文而非设计文档,本条的问法需重新表述。
8. ~~**三形态收敛**~~ —— **【作废】** research 独立后,不再与模型自派 task 勘察、SCOUT_ROLES 编排勘察收敛为同一工件格式、证据口径与落点。

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
