# research mode 先行对照:已有方案调查(prior art)

- 状态: 完成(2026-08-16 三路调查全部落盘);本文档是 research_mode.md 重写与 R-273~R-276 登记的证据基座
- 来源: 2026-08-16 用户定调 research mode 开工并点名 LaTeX 编译、科研绘图、调色板推荐三项配套;三路后台调查代理产出,关键结论均附出处
- 关联: docs/design/research_mode.md(设计真源)、R-221(实施条目)、R-248(先行调研内建——本文档正是该机制的一次人工先行示范)

## §1 现有 auto research 系统盘点

领域综述索引:[Deep Research Agents: A Systematic Examination and Roadmap(arXiv 2506.18096)](https://arxiv.org/abs/2506.18096)。

### 商业产品

- **OpenAI Deep Research**:单 agent,端到端 RL 把「搜索→点击→读文件→Python 分析→写报告」直接训练进早期 o3;开跑前交互式澄清;5–30 分钟自决轮次;内联引用+侧栏来源;无 LaTeX。HLE 26.6%、GAIA ~67%([System Card](https://openai.com/index/deep-research-system-card/))。
- **Google Gemini Deep Research**:planner+task 模型异步任务管理器,出错局部恢复;**先生成显式研究计划给用户批准/修改**(全行业最突出的计划前置);单次读 20–100+ 来源;报告可导出 Docs 并增量修改。DeepResearch Bench RACE 48.88 居首([philschmid](https://www.philschmid.de/deep-research-update))。
- **Anthropic Claude Research**:orchestrator-worker 多 agent(lead Opus 派 3–5 个 Sonnet subagent 并行检索);**显式 effort-scaling 规则**(简单查询 1 agent/3–10 调用,复杂 10+ agent);**独立 CitationAgent** 写后逐条配引用;token 用量约普通对话 15 倍且解释 80% 性能方差。评测方法可抄:~20 条查询小集 + LLM rubric judge([工程博客](https://www.anthropic.com/engineering/built-multi-agent-research-system))。
- **Perplexity Deep Research**:单 agent 迭代环(搜索→读→推理→修订计划),3 分钟内完成、20–50 次查询,快而便宜;内联数字引用+独立 Sources 面板([官方博客](https://www.perplexity.ai/hub/blog/introducing-perplexity-deep-research))。

### 开源系统

- **Stanford STORM / Co-STORM(MIT)**:两阶段「预写作(知识策展)→写作(按大纲逐节)」;**perspective-guided 提问**(先归纳多视角,每视角驱动一条模拟对话线);信息入 `StormInformationTable` 即绑定来源,写作从表中取材——**引用在收集时绑定**。Co-STORM 加动态思维导图与人机协作([GitHub](https://github.com/stanford-oval/storm))。
- **GPT-Researcher(Apache-2.0,28.8k stars)**:planner→并行 executors→publisher;deep 模式树状递归,**breadth/depth/concurrency 三个显式预算旋钮**;deep 约 5 分钟/$0.40([文档](https://docs.gptr.dev/docs/gpt-researcher/gptr/deep_research))。
- **HuggingFace smolagents ODR(Apache-2.0)**:CodeAgent(动作用 Python 代码表达)比 JSON agent 少 30% 步骤,GAIA 55% vs 33%;纯文本浏览器工具成本低([HF 博客](https://huggingface.co/blog/open-deep-research))。
- **LangChain Open Deep Research(MIT)**:三段 **Scope(压缩出研究简报)→Research(supervisor 派并行 researcher,各自隔离上下文,回传前压缩清洗)→Write(一次性单点写作)**。公开教训:并行写作不连贯(改研究并行、写作串行)、原始工具输出撑爆上下文(逐层压缩)。RACE 0.4344 开源最高档([博客](https://www.langchain.com/blog/open-deep-research))。
- **Sakana AI Scientist v1/v2**:**唯一原生产出 LaTeX 论文的系统**——v1 按会议模板逐节填充+编译回环修错,每篇 <$15;v2 加实验管理 agent+best-first 树搜索,ICLR workshop 过审 1 篇。**注意 2025-12 起改为自定义许可**(商用/再分发需读条款);幻觉引用是公认弱项([v1](https://github.com/sakanaai/ai-scientist)、[v2](https://arxiv.org/abs/2504.08066))。
- **PaperQA2(Apache-2.0,FutureHouse)**:agentic RAG;三层检索 **tantivy(Rust 全文引擎)+向量+LLM 重排**;核心机制 **RCS**——top-k 片段逐条打相关分+生成带出处压缩摘要,**答案只能从带源摘要合成,机制性杜绝无源论断**;另有引用遍历(沿引文图扩检)。LitQA2 超 PhD 人类基线([论文](https://arxiv.org/html/2409.13740v1))。
- **Tongyi DeepResearch(Apache-2.0 权重)**:30.5B MoE 专训;Heavy 模式 **IterResearch——每轮重建精简工作区**对抗「认知窒息」(与 kanzei R-267 消息窗口化同构);训练派上限证明但不可复制([博客](https://tongyi-agent.github.io/blog/introducing-tongyi-deep-research/))。
- **DeerFlow(MIT,字节)**:LangGraph 状态图:Coordinator→Planner→**人工审批闸口**→Research Team(**Coder 与 Researcher 并列一等角色**)→Reporter——对「文献+代码」双对象最有参考价值([GitHub](https://github.com/bytedance/deer-flow))。
- 其它:Jina node-DeepResearch(**token 预算做终止条件**)、dzhng/deep-research(<500 行最小递归实现)、gemini-fullstack-langgraph-quickstart(「反思找缺口→迭代补搜」教科书示例)。

### 评测基准

- **DeepResearch Bench**(RACE/FACT):**FACT 抽「论断-URL」对逐条验证支撑**,量化引用准确率——做引用体系时直接可复用的验收框架([arXiv 2506.11763](https://arxiv.org/abs/2506.11763))。
- FutureSearch Deep Research Bench:离线冻结网页快照保证可复现;结论之一:带搜索工具的裸 o3 胜过 OpenAI DR 产品。
- repo 级 QA 基准已成体系:RepoQA、SWE-QA(720 题/15 仓/340 万行)、CodeRepoQA、CodeRAG-Bench——可抽小样本做 kanzei 研究模式验收集。

### 前端呈现模式(R-276 的输入)

横评来源:[Moretti: Deep Research UIs 四家对比](https://dev.to/franciscomoretti/deep-research-uis-perplexity-vs-manus-vs-chatgpt-vs-gemini-5cc2)。一句话定位:Perplexity 来源至上;Manus 过程至上(嘈杂);ChatGPT 用户控制+干净步骤;Gemini 报告至上+双面板。

- 进度:Gemini 独立 Thoughts 面板+**可编辑研究计划是一等 UI 对象**;ChatGPT 实时步骤 sheet 完成后折叠成紧凑卡、运行中可转向;Perplexity 几乎不展示过程。
- 引用:Perplexity 三处冗余(内联数字+顶部来源卡+Sources 页)溯源最强;Claude 内联引用悬停预览;Gemini 融于报告最干净但溯源弱。
- 报告:Gemini 双面板(左会话右文档,步骤折叠于报告下),可继续对话增量修改——「结果>过程」层级最明确。
- **四组件通用 schema**:document(报告)/steps(活动)/sources(来源)/annotations(内联引用),各家差异只在权重分配。

### 综合:共同模式 / 抄什么 / 双对象启示

**全行业收敛的七条**:①四段流水线(澄清→计划→检索-阅读-反思环→综合写作);②workflow 编排派 vs RL 训练派,开源可自持的只有前者;③**研究并行、写作串行单点**(LangChain 踩坑实证);④上下文靠逐层压缩存活(隔离子上下文→压缩回传→或每轮重建工作区);⑤**引用在收集时绑定而非写完再找**(STORM 信息表/PaperQA2 RCS);⑥预算是显式旋钮(effort 规则/breadth-depth/token 预算);⑦计划要给人看(审批/修改闸口是与普通 chat 的最大交互差异)。

**kanzei 应抄**:LangChain ODR 三段骨架(状态机形态与 Rust 亲和);STORM 大纲驱动+信息表;**PaperQA2 的 RCS + tantivy(tantivy 本身是 Rust 库,可直接做本地文献/代码全文索引)**;Anthropic effort-scaling 显式规则+小评测集方法;GPT-Researcher 预算旋钮;AI Scientist v1 的 LaTeX 模板填充+编译回环(只抄写作段);Gemini/ChatGPT 计划审批交互+Perplexity 来源冗余呈现。

**kanzei 应跳过**:真·多 agent 并行编排(15 倍 token;单用户串行迭代环+有限并发检索即可,上下文冲突用「子任务隔离+压缩回传」同样能解);RL 训练专用模型(纪律放系统侧,与「弱模型也能照着走」准绳一致);分布式任务管理/多租户(单机断点续跑状态文件即可);模拟审稿与自动选题(人选题);通用 GUI 浏览器自动化(文本浏览器+API 足够)。

**「文献+代码」双对象启示**:代码侧样板是 Cognition DeepWiki(图式仓库分析→wiki 工件)与 LangChain OpenWiki(**文档随仓库演进**,定位与 kanzei 最契合);代码的「检索」=符号图+全文索引+结构遍历(kanzei 已有 symbols/define 反查即 DeepWiki 图式分析的对应物,应与 tantivy 文献索引挂同一检索接口);代码引用格式 `文件:行号 @ commit` 等价于文献 `DOI+页码`,FACT 式逐条验证在代码侧同样可机检;**文献论断↔代码实现互证是现有系统全都没做的空白点,kanzei 的独有优势**。

## §2 LaTeX 编译与科研绘图工具链(Windows / Rust / Tauri+CLI)

### LaTeX:Tectonic 侧车为底、系统发行版为增强的双层通道

- **Tectonic 维护状态:活跃复兴。** 0.16.0(2026-04)bundle 系统重做支持 TeXLive 2024、xetex_layout 重写为纯 Rust、新增外部工具调用 `-X`;0.17.0(2026-07-27)修 XeTeX segfault、初次构建并发拉取缓存([releases](https://github.com/tectonic-typesetting/tectonic/releases)、[CHANGELOG](https://docs.rs/crate/tectonic/latest/source/CHANGELOG.md))。Windows 官方预编译二进制,单文件、无需 TeX 发行版、自动跑对遍数、宏包按需下载([主页](https://tectonic-typesetting.github.io/))。
- **CLI 侧车,不嵌 crate。** 官方明说依赖大量 C/C++ 库(harfbuzz/ICU/freetype/fontconfig),Windows 构建链 cargo-vcpkg 又长又脆([构建文档](https://tectonic-typesetting.github.io/book/latest/howto/build-tectonic/index.html));随包分发官方 exe(或首启下载校验),进程调用功能面与 crate 无差别。
- **离线:** 首编译联网拉宏包并缓存;`--only-cached` 强制离线。默认模式每次调用发网络请求核对 bundle 拖慢启动([#1224](https://github.com/tectonic-typesetting/tectonic/issues/1224))→ 封装预热常用宏包后默认 `--only-cached`,失败再放开网络重试。
- **bib:** bibtex 内置纯 Rust 实现,循环全自动;**biber 不内置**([#1010](https://github.com/tectonic-typesetting/tectonic/issues/1010))→ agent 提示词约定默认 natbib/bibtex 路线,biblatex 声明为"检测到 biber 才可用"。
- **系统发行版增强:** PATH 检测 `kpsewhich`/`latexmk`(VS Code LaTeX Workshop 惯例),检测到 MiKTeX/TeX Live(有 biber/latexmk/全量宏包)优先用,否则回落 Tectonic,不要求用户装。
- **PDF→PNG 回传:** `pdfium-render` crate + [pdfium-binaries](https://github.com/bblanchon/pdfium-binaries/releases) 预编译 DLL 侧车,页面→PNG 是第一优先能力。
- **Typst(诚实对比,暂不纳入):** typst crate 纯 Rust 可嵌、typst-bake 可完全自包含、直接出 PNG/SVG 零转换;但期刊投稿生态仍是 LaTeX 垄断(NeurIPS/ICML/ACL 要求 .tex)。定位只能是"快速预览+内部文档"并行通道,不替代 LaTeX;是否加挂另行评估。

### 绘图:Vega-Lite(vl-convert)+ PGFPlots 零安装双轨,matplotlib 为检测增强

| 方案 | 出版级质量 | agent 生成难易 | Windows 零安装 | 色板注入 |
|---|---|---|---|---|
| Vega-Lite + vl-convert | ★★★★ | 易(JSON 规格可校验) | ✓ 纯 Rust/独立 CLI | 最易(spec `scale.range`/config) |
| PGFPlots/TikZ | ★★★★★ 与正文字体统一 | 中(语料丰,调试痛) | ✓ 复用 Tectonic | 中(cycle list) |
| matplotlib+scienceplots | ★★★★★ 事实标准 | 最易(语料最丰) | ✗ 需 Python(可 uv 环境化) | 易(rcParams/cycler) |
| plotters/gnuplot/charming/plotly.rs | 排除 | — | — | — |

- **vl-convert 核查结论:嵌入可行,最优纯 Rust 路线。** 经 deno_runtime 内嵌 v8 跑官方 Vega-Lite JS,完全离线;SVG 原生、PNG 经 resvg;代价是 deno_runtime/v8 依赖树巨大(编译时间/体积)→ **可改用其独立 CLI 当侧车,功能等同**([vl-convert](https://github.com/vega/vl-convert))。
- **PGFPlots** 零额外依赖(宏包 Tectonic 按需拉),投稿场景不可替代;迭代慢、报错不友好,当终稿通道。
- **matplotlib+scienceplots** 是上限增强:检测到 Python/uv 才启用,`uv run --with matplotlib,scienceplots` 按需环境化([SciencePlots](https://github.com/garrettj403/SciencePlots))。
- plotters 无抗锯齿出版级不达标、gnuplot 需外装且质量平庸、charming 信息图风不符期刊规范、plotly.rs 的 kaleido 依赖乱——均排除。

### 统一形态

三条通道输出统一转 PNG 回传模型(ToolOutput images 通道,R-249 已交付)、原始 PDF/SVG 落盘给用户;色板注入在 Vega-Lite(JSON config)与 matplotlib(rcParams)两端都有干净挂点。

## §3 科研配色体系与调色板推荐

### 内置源(许可证全干净,机器可读,零运行时联网)

| 体系 | 类型 | 许可证 | 数据 |
|---|---|---|---|
| ColorBrewer | seq/div/qual 全 | Apache-2.0(需致谢) | [官方 JSON](https://colorbrewer2.org/export/colorbrewer.json) |
| viridis 系(matplotlib) | seq | CC0 | [BIDS/colormap](https://github.com/BIDS/colormap) |
| Petroff 色环(petroff10) | qual 色盲安全 | CC0 | [仓库](https://github.com/mpetroff/accessible-color-cycles) |
| Crameri Scientific Colour Maps | seq/div/cyclic+categorical | MIT | Zenodo 10.5281/zenodo.1243862 |
| Paul Tol(SRON) | qual/div/seq,红绿色盲校验 | 按 BSD-3 对待 | 官网 PDF+tol_colors.py |
| Okabe-Ito 八色 | qual 事实标准 | 纯色值无版权,注出处 | R≥4.0 base 内置 |
| cmocean | 变量语义主题 | MIT | [仓库](https://github.com/matplotlib/cmocean) |

内置组合:定性 = Okabe-Ito + Tol bright/muted + ColorBrewer Set2/Dark2;序列 = viridis/cividis + batlow + ColorBrewer;发散 = vik/roma + RdBu + sunset;主题 = cmocean。

### 推荐规则与校验(Rust 本地可全量实现,无需 Python)

- 选型规则:无序分类→qualitative(≤8–12 色);有序连续→sequential(亮度单调);有中点→diverging(中点浅色);周期→cyclic;硬禁忌 jet/rainbow 连续量、定性板不得插值([Crameri et al. 2020](https://www.nature.com/articles/s41467-020-19160-7))。
- 校验链:CVD 模拟(Machado 2009/Viénot 1999)→ 两两 CIEDE2000(模拟后再跑才算真色盲安全)→ WCAG 图形对比度 ≥3:1 → 连续板亮度单调性。Rust `palette` crate 内置 Ciede2000,另有 deltae/color_blinder 参考。
- 工程先例:**Vega-Lite 按字段类型自动选板**(nominal→tableau10、quantitative→viridis/blues,[文档](https://vega.github.io/vega-lite/docs/scale.html))是最接近的先例;R colorspace、Colorgorical、Palettailor、qualpalr 提供构造/生成/分配算法参考。

### 配色网站爬取:砍掉,免爬替代更优

- Coolors **ToS 明确禁止**批量抓取与合集再分发;Adobe Color API 已死(2019 下线);ColorHunt 无官方 API 且 ToS 不可核验(灰色);Paletton 是和谐规则生成器(规则可本地 OKLCH 实现);Lospec 有官方 API 但像素画向。
- 版权务实结论:纯色值组合基本不构成版权客体,风险来自 ToS/反爬而非色值版权。
- 免爬替代:paletteer(2893 板/79 包)、pypalettes(2500+)、palettable(MIT)等开源聚合 + §3 官方源,覆盖面与质量均超爬站。**保留「用户粘贴 hex / 导入 .gpl/.ase」入口即满足"自己喂色板"需求。**

### 用户自定义色板

- 导入:GIMP .gpl 首选(开源界事实标准)、Adobe .ase、每行一色 .hex;**内部规范 JSON 为唯一真源**(name/type/colors[]/max_classes/colorspace/source_url/license)。
- 映射:按板固定顺序分配,同系列跨图同色;**定性板不够长绝不插值**——默认拒绝并提示改分面/高亮,兜底循环+线型区分;连续板按位置采样取 n 档正当,发散取奇数档保中点。
- 导入时打 type 标签→跑校验链→评分;AI 绘图按类型规则自动选,用户板同类型优先。
