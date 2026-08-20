# research 模式设计:独立深度研究模式(文献+仓库,论文级产出)

- 状态: **设计基线**(2026-08-16 §2 定调点全部经用户过审转正;先行对照证据见 [research_mode_prior_art.md](research_mode_prior_art.md))
- 排期: 已开工(2026-08-16 用户定调;原「dev 稳定之后」排期阻塞同日解除)
- 实施条目: R-221(模式基座,分批见 §7)、R-277(研究引擎)、R-273(LaTeX)、R-274(绘图)、R-275(调色板)、R-276(前端)
- 前置文档: [interaction_modes.md](interaction_modes.md)(模式体系)、harness_m1.md §5(research 原始承诺)、[memory_control_plane.md](memory_control_plane.md)(记忆边界)、[research_mode_prior_art.md](research_mode_prior_art.md)(先行对照)
- 审计证据: [audit_20260812_eight_dimensions.md](audit_20260812_eight_dimensions.md) §8

## 0. 现状定性(2026-08-12 审计,全部有证据;历史记录保留)

**骨架早已完整,但形态错位且零使用。**

- 骨架存在:独立 ResearchProfile(profiles.rs:536-624)、S-/F- 来源与发现 DocKind(docstore.rs:65-89)、websearch/webfetch 放行、写权收窄 `.kanzei/research/**`(含 `..` 穿越防护)、前端来源/发现页与 report.md 入口。
- **零使用**:state.db 266 条 episodes 零调用 websearch/source/finding;`.kanzei/research/` 全部 git 历史只有一个空模板 memory.md。
- 真实的研究(代码勘察)全部发生在 dev/dev-auto,结论无固定工件落点(勘察报告被 D-294 单行不变式折成单行塞 tracker 进展字段)。
- 证据等级 E0-E4 双重语义挪用;`.kanzei/research/memory.md` 是绕开记忆控制平面的第二套无校验记忆;harness_m1 §5 承诺的 topic 分目录实现为全局平铺;readonly 档位桌面端不可达。

## 1. 定位(核心命题,2026-08-14 用户定调)

**research 模式 = 独立的深度研究模式,不是「先计划后自举」的载体。**

- **主形态**=深度分析调研(autoresearch 形态):对**文献**与**仓库**两类对象做深度检索、交叉验证与综述。
- **产出物**=论文级工件:正文、LaTeX 源码、图表、参考文献。研究的终点是可发布、可归档的成果本身。
- **网络检索(websearch/webfetch)是主力工具**,不是辅助。
- **绝对独立**:不与 dev 侧的代码库勘察合并;模型自派 task 勘察与 SCOUT_ROLES 编排勘察是 dev 的能力,各归各。

> **作废声明(2026-08-14)**:本节此前版本(2026-08-12 草案)主张「research = 先计划后自举的正式载体」,已被用户明确否定,连带原定调点 1 与 8 作废。dev 侧「勘察工件无固定落点」问题依然存在,需另立条目承接,不由 research 模式代管。

## 2. 定调点(全部已定)

1. ~~主形态=代码库勘察~~ —— **【作废 2026-08-14】** 见 §1。
2. **【已定 2026-08-16】工件落点**:`.kanzei/research/<topic>/` 论文全结构(见 §3),重课题全结构、轻课题可降级只有 report.md——结构是上限不是义务。tracker 进展字段只写一行摘要+报告路径(根治 D-276/D-294 一系)。
3. **【已定 2026-08-14,扩展已定 2026-08-16】证据等级单列 V 表**,与验证体系 E0-E4 分家;文献域扩展见 §4。
4. **【已定 2026-08-16】回流通道保留精简版**:报告「建议登记」段 + finding→req/defect 草稿转化(产物一律 [todo] 待 dev 轮确认)。闸门在产物状态,不在能力面。
5. **【已定 2026-08-16】记忆一元化**:研究结论进记忆统一走 memory_note→manager 晋升;refs.bib 是论文引用管理,与记忆晋升两不相干、各归各;research/memory.md 停止注入(文件保留为历史)。
6. **【已定 2026-08-16】档位矩阵**:bash 维持硬 deny+替代指引;LaTeX 编译与绘图走**专用工具通道**(R-273/R-274/R-275),与 architecture/conventions 专用写通道同一手法。桌面端补注册 ReadonlyProfile。
7. **【已定 2026-08-16】research 不可写 `docs/design/*.md`**:论文工件全落 topic 目录;结论要转正走定调点 4 的草稿回流或用户手动。
8. ~~三形态收敛~~ —— **【作废 2026-08-14】**。

## 3. 工件设计(定调点 2)

```text
.kanzei/research/
  <topic>/                 # kebab-case 课题名
    report.md              # 轻课题:单报告(每条结论带 V 等级+证据锚)
    outline.md             # 大纲(先大纲后分节写作,STORM 先例)
    paper.tex              # 重课题:论文正文(R-273 编译)
    figures/               # 图表(R-274 产出:SVG/PDF 落盘 + PNG 回传)
    refs.bib               # 参考文献(论文引用管理,与记忆分家)
    sources.md             # S- 外部来源
    findings.md            # F- 结构化发现(收集时即绑定来源,见 §5)
    notes/                 # 中间工作区(压缩摘要等),可选
```

### 3.1 dev 侧勘察工件约定(R-304)

research 模式与 dev 侧代码勘察保持能力边界独立，但两者复用同一个可追溯落点：

- **落点**：`.kanzei/research/<topic>/`；dev 勘察的 `<topic>` 使用 `<entry-id>-<slug>`，例如 `r304-dev-recon`。目录内至少保留 `report.md`；`notes/`、`evidence/`、`comparison/` 和图表等为可选中间/证据目录。
- **报告命名**：最终结论固定为 `report.md`，中间文件使用小写 kebab-case 文件名；临时索引、checkpoint、缓存和命令输出不得冒充最终报告。
- **回溯绑定**：报告头部写 `kind: dev_recon`、`topic: <topic>`、`entry_refs: R-/D-/T-` 和 `status: active|archived`；tracker 的 `refs` 仍只写 R-/D-/T- 编号，`进展` 只写一行摘要加相对报告路径，例如 `.kanzei/research/r304-dev-recon/report.md`。这样从条目到报告、从报告头部回到条目都可机械检索，且不把文件路径混入 tracker refs 契约。
- **证据口径**：代码结论沿用 V0–V3 与 `file:line`/提交锚；文献结论必须带 URL、V 等级和摘要级/正文级深度。没有锚点明确写 V0，不把 E0–E4 测试等级混入研究报告。
- **工具边界（复用既有能力）**：dev 侧直接用现有 `write`/`edit`/`insert` 写入 `.kanzei/research/**`，用 `read`/`glob`/`grep`/`files` 复核，用 `git diff/status` 保留变更边界；本条不新增第二套 source/finding 存储，也不把 research profile 专用的 source/finding 工具虚报成 dev 工具。
- **生命周期与清理**：工作中为 `active`；条目关闭时把报告标为 `archived`，最终报告和支撑结论的证据永久保留。只有已被报告吸收且可重建的中间笔记、临时输出和索引缓存可清理；不可重建的证据移入 `archive/` 或保留原位，禁止静默删除。清理动作在条目进展或提交说明中留下记录。
- **先行调研（R-248，已实现）**：工件固定为 `.kanzei/research/<topic>/prior-art.md`。项目首次创建 `.kanzei/`、`req add` 的「核心 + refs 为空」、用户显式调用 `prior_art start` 三者之一会机械触发。核心需求缺工件时，tracker 先按待分配编号创建 `<entry-id>-<slug>` 骨架再拒绝登记；补齐后通过顶层 `prior_art` 传路径，或通过 `prior_art_waiver` 记录用户明确豁免。两者都独立于 `refs`，后者继续只收 R-/D-/T-。

### 3.2 prior-art 工件与联网预算（R-248 / D-571）

- 工件头必须包含 `kind: prior_art`、与目录一致的 `topic`、`status: complete`、`entry_refs` 和 `websearch_round_limit`（1–4）。骨架固定为 `pending`，不能冒充完成。
- `外部已有实现` 与 `仓内既有设计` 两节都至少一条。每条使用三级标题作为方案名，并完整填写 `出处`、`证据等级`、`差异`、`决策`；外部出处必须是 http(s) URL，仓内出处必须是可回读的 `file:相对路径:行号`。任一项缺失都由 `prior_art validate` 或 `req add` 拒绝。
- prior-art 搜索调用在 `websearch` 传 `prior_art_topic`，每次尝试都会消耗该 topic 的搜索预算，达到上限时返回 `PRIOR_ART_SEARCH_LIMIT`，不静默截断或继续扩散。
- research 档的 `websearch`/`webfetch` 额外强制携带 `research_loop begin_search` 返回的 `topic + task_id`。伪造、过期或缺失任务在发出网络请求前被拒；DuckDuckGo HTML 不可达时明确指向已知 URL 的 `webfetch` 与 arXiv abs/pdf/API 通道。

- 轻重课题共用同一目录约定;轻课题只产 report.md 合法。
- report/paper 契约(轻约定,不做 schema 校验):头部=课题/日期/关联条目/总体证据等级(取最低);每条结论=**一句话 + 证据锚(S-id、URL 或 file:line)+ V 等级**,无锚必须显式标 V0;结尾=「建议登记」段(定调点 4 的回流入口)。
- tracker 衔接:条目进展字段只写 `一行摘要 + 报告路径`;tracker 的 `refs` 只写 R-/D-/T- 追踪编号,topic 和文件路径写在报告头部及进展字段。

## 4. 证据等级 V 表(定调点 3,双域)

| 等级 | 代码域(已定 2026-08-14) | 文献域(2026-08-16 扩展,2026-08-16 D-412 补证据深度) |
| --- | --- | --- |
| V0 | 目录/命名推测 | 无出处断言 |
| V1 | 读码核实(file:line) | 二手转述(博客/新闻/论坛);一手来源**仅摘要级**(只读 title+summary,未取正文) |
| V2 | 运行时实测 | 一手来源**正文级**(读过正文:论文原文/官方文档/仓库源码,经正文核验) |
| V3 | 用户复现 | 交叉验证(≥2 独立一手互证,均为正文级)或本地实测复现 |

写进 conventions;报告每条结论标域+等级+证据深度(摘要级/正文级);存量条目不回改,新条目起用。E/V 两表互不混用。

**证据深度口径(D-412 反例)**:文献论断的支撑文本必须落在正文内,仅摘要级来源不得支撑正文级论断。实例:CoALA(arXiv 2309.02427)的 working/episodic/semantic/procedural 四类记忆划分**不在摘要中**(摘要只有 "modular memory components"),只在正文 §2.3 Soar memory 段落(procedural/semantic/episodic 各自定义)与 §4.1——以 arXiv API 摘要为据标 V2 属越界,须取正文核验后标正文级 V2,否则降级 V1(摘要级)。

## 5. 研究引擎(采纳先行对照结论;实施载体 R-277)

四段流水线(全行业收敛模式,见 prior_art §1 综合):

1. **澄清+计划**:定界后产出显式研究计划树,**给用户审批/修改后才跑**(Gemini/ChatGPT/DeerFlow 先例;前端呈现由 R-276 承接,引擎只出计划数据结构)。
2. **检索-阅读-反思环**:**串行迭代 + 有限并发检索**,不做真·多 agent 编排(Anthropic 实测 15 倍 token,单用户不值);子任务隔离上下文,回传前压缩清洗(PaperQA2 RCS 式:相关分 + 带出处压缩摘要),**原始网页/工具输出不直接进主上下文**;信息写入 findings.md 时即绑定来源(STORM 信息表先例)——引用在收集时绑定,不写完再找。反思步找知识缺口决定补搜。
3. **综合写作**:先 outline.md 后分节,**单点一次性生成**(并行写作不连贯是 LangChain 公开踩过的坑);重课题写 paper.tex 并走 R-273 编译回环修错(AI Scientist v1 先例)。
4. **引用校验**:FACT 式论断-出处逐条核验(文献=URL 内容支撑;代码=`file:line @ commit` 存在且语义支撑),抽查不过重写该节。

支撑件:

- **预算显式旋钮**:检索轮次与 token 上限可配,超限收敛写作而非报错(GPT-Researcher breadth/depth、Jina token 预算先例)。
- **本地索引**:tantivy(Rust 全文引擎,PaperQA2 同款)索引文献 PDF 与代码,**与 symbols 反查挂同一检索接口**——「文献论断↔代码实现互证」是现有系统都没做的空白,kanzei 独有优势。
- **断点续跑**:单机进程内状态文件,中途强杀可恢复;不做分布式任务管理。
- 跳过:RL 专训模型(纪律放系统侧,「弱模型也能照着走」)、模拟审稿与自动选题(人选题)、通用 GUI 浏览器自动化(文本抽取+API 足够)。

## 6. 档位与工具面(定调点 6,目标态)

| 能力 | research(目标) | 变更 |
| --- | --- | --- |
| read/glob/grep | 放行 | 不变 |
| files/git(只读) | 放行 | R-218 已交付 SubagentBase 六件套,复用 |
| webfetch/websearch | 放行(主力) | 不变 |
| source/finding | 放行(挂 topic) | 改造(平铺→topic) |
| write | 仅 `.kanzei/research/**` | 不变 |
| bash | **硬 deny+替代指引** | 指引指向 latex/plot 专用工具 |
| latex(R-273)/plot(R-274)/palette(R-275) | 放行 | 新增专用通道 |
| req/defect | get + add(产物 [todo]) | 新增(定调点 4) |
| memory_search/memory_note | 放行 | 新增(定调点 5);memory.md 停止注入 |
| 上下文注入 | research-docs + backlog 只读索引 + conventions | 新增 |

## 7. 分批实施(R-221 基座;引擎另见 R-277)

- 批1 档位收口:桌面注册 ReadonlyProfile(或明示 CLI-only);research 档 bash 硬 deny+替代指引(指向专用工具);工具面加 files/git 只读。验收:桌面/CLI 档位表一致,bash 被拒时指引文案指向 latex/plot。
- 批2 topic 工件:source/finding/report 落 `<topic>/`;前端研究页按 topic 分组。验收:两个课题互不覆盖,report 可从条目 refs 跳转。
- 批3 证据口径:V 表双域写进 conventions;dev/research 提示词同步。验收:标注可查到权威定义,E/V 互不混用。
- 批4 回流通道:backlog 只读索引+conventions 注入;req/defect get+add 子集;finding→草稿动作。验收:一次研究会话能引用 R- 条目并产出一条 [todo] 草稿。
- 批5 记忆一元化:memory_search/memory_note 进 research 档;memory.md 停止注入。验收:research 档无第二套无校验记忆注入点。

(原批6 三形态收敛已随定调点 8 作废删除。)

## 8. 与既有系统的关系

- **interaction_modes.md**:前端模式三选一不变;research 下连跑仍禁用(研究不自动推进)。
- **memory_control_plane.md**:研究结论进记忆走统一管线;S-/F- 引用是记忆 provenance 的合法来源类型;refs.bib 与记忆无关。
- **D-276/R-201(游离文本)**:report.md/paper.tex 是多行内容的合法落点,tracker 字段回归单行摘要。
- **R-248(先行调研内建)**:prior-art 工件落点复用本设计的 topic 目录;该条整体排在 R-221 之后不变。

## 9. 验收总则(整条链路的终局)

用 research 模式对一个真实课题完成一次研究:计划树经用户审批 → 检索-阅读环产出带来源的 findings → 综合出 `<topic>/report.md`(或 paper.tex+figures/+refs.bib,经 R-273 编译通过)→ 每条结论带双域 V 等级与证据锚,FACT 式抽查全部支撑 → 「建议登记」段转成 [todo] 草稿 → 条目 refs 引用 topic、进展字段只有一行摘要——**选题→计划→研究→论文级工件→回流登记的完整链路有轨迹可查**。

## 10. 边界(本设计不做)

- 不做常驻知识库服务与跨项目知识库;tantivy 索引是引擎组件,随课题建随课题用。
- 不做勘察报告 schema 校验(轻约定+提示词纪律);不做模拟审稿、自动选题。
- 不改 dev 模式取活纪律与工具面;不动 E0-E4 在验证体系的语义。
- research 不可写 docs/design、不可提交 git、不可动 tracker 既有条目状态(add 草稿除外)。
- 不为 research 单独造记忆存储——一切走既有控制平面。
