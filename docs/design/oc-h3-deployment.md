# OC 动画：H3 社区版本与双卡方案

核对日期：2026-09-24。状态：用户认可 v5 修订方向后，已完成 v6 九种状态的完整视频角色包和三种待机变体，并接入本地源码与预览。原始视频与生成证据均保留；具体运行处理见 [制作记录](oc-production.md)。服务器独立目录为 `/home/p2522808/kanzei-h3`，全部生成结束后服务已停止，GPU 4、5 各 2 MiB，30010 端口关闭。下文 v5 及更早段落是各阶段的历史记录。

部署脚本见 [scripts/oc-h3](../../scripts/oc-h3/README.md)。实际 FL2VA 下载清单为 144,051,185,561 字节；该值包含完整磁盘权重和配置，不等于推理时的 GPU 显存需求。运行环境使用发布的 SGLang 0.5.20 wheel，依赖锁定结果存入服务器 `logs/requirements.lock.txt`。下列 SGLang 源码 revision 用于固定所参考的部署文档。

## 选择

首选 **MiniMax H3 FL2VA + Larry Turbo v4-600 EMA，8 次去噪计算，SGLang TP2**。以当前 OC 母版做图生视频，先验证完整角色抬手时的肩膀、袖口、前臂和手指关系。

Larry 作者将 v4-600 EMA 推荐用于静止和小动作画面，称其改善脸、手指等细节；SGLang 文档也把该文件列为速度与质量平衡方案。此结论来自作者说明及运行框架的适配记录，尚未在本项目的 OC 上比较画质。

保留 **LightX2V FL2VA Turbo 8-step v1.0 768p** 作为对照。它支持首尾帧和图生视频，作者推荐 8 次计算、video shift 6 / audio shift 3。不能直接套用同仓库旧版 4-step v0.1 的 alpha 和采样设置。

FastH3 Preview v1 的 4 步稀疏版本只蒸馏了文生视频，作者明确未蒸馏 FL2VA / Ref2VA，不用于当前需要锁定角色参考图的第一轮样片。GGUF 主要作为显存容量方案；当前选择的 SGLang GGUF 路径不支持 LoRA，因此不与本次 Turbo 配置混用。RTX 5880 Ada 不具备 SGLang 原生 NVFP4 路径要求的 compute capability 10.0+。

## 固定版本

| 部件 | 仓库、文件与 revision |
| --- | --- |
| 基座 | `MiniMaxAI/MiniMax-H3`，`42ed227ee7df40d41602854ae760620d6eb651fe`；只下载 `model_index.json` 与 `FL2VA/*` |
| 首选适配器 | `larryvrh/MiniMax-H3-Turbo-Lora`，`43a74557ac3f6539db8e0f2a959d03feb7a81480`；`minimax_h3_turbo_v4_step600_ema.safetensors` |
| 对照适配器 | `lightx2v/Minimax-h3-Turbo`，`3ec17a324ced54151364f24f8b5fb6bf7e26414f`；`minimax_h3_fl2v_turbo_8step_v1.0_768p_bf16.safetensors` |
| 运行框架参考 | `sgl-project/sglang`，`954458567e10257f1d8d5ff808d2dbc74fc26967`；实际安装发布 wheel 0.5.20，PyTorch 2.13.0+cu130 |

## 服务器与选卡

SSH 别名为 `gpu`。2026-09-24 04:28 北京时间核对：8 张 RTX 5880 Ada，单卡 49140 MiB；GPU 1–5 无计算进程，GPU 0、6、7 有其他用户任务。

仅选择以下两张卡，二者同属 **NUMA 0**，`nvidia-smi topo -m` 相互连接为 `NODE`，不是跨 NUMA 的 `SYS`，没有 NVLink。P2P read 能力报告为 `OK`，这不是实际 NCCL 通信测试的替代。

| 物理编号 | UUID | NUMA |
| --- | --- | --- |
| GPU 1 | `GPU-eff8864c-9830-23bb-4dc9-104389c8ddad` | 0 |
| GPU 2 | `GPU-74f54117-182b-6fcc-429b-876cc116be10` | 0 |

启动时按固定 UUID 查找卡，重新检查 NUMA、进程和显存占用。SGLang 0.5.20 的 NVML 辅助函数只接受数字掩码，因此再核对 PCI 排序与 NVML 编号一致，将对应编号写入 `CUDA_VISIBLE_DEVICES=1,2`。CPU 优先绑定 NUMA 0，内存优先节点 0，但允许系统内存卸载跨节点回退。

实测默认 NCCL P2P 路径超时；设置 `NCCL_P2P_DISABLE=1` 后双卡 all-reduce 和 BF16 计算均通过。启动配置已固定这个设置。SGLang 通过目录名识别 H3，因此用 `models/MiniMax-H3` 链接到原有 `models/base`，权重无需复制。

首次完整加载暴露 SGLang 0.5.20 的 H3/LoRA 兼容缺陷：`_accepts_mxfp8_input` 直接读取 LoRA 外层不存在的 `quant_method`。独立环境内加入小范围修正，缺少此属性的层继续使用原 BF16 路径，保留 LoRA 运算。原文件、修改前后 SHA256 与四项能力分支检查结果保存在 `logs/compat-patches.json`；补丁脚本会拒绝不匹配的版本或源代码。首次 CUDA JIT 所需的 `CUDA_HOME`、`lib64`、`libcudart.so` 路径也由启动脚本配置。修正后已通过两次完整出片验证。

服务器系统 Python 未提供开发头文件。将 Ubuntu `libpython3.12-dev` 包解压到部署的 `tools/python-dev`，通过 `CPATH` 提供给 Triton；未改动系统 Python。媒体工具为独立目录内的 FFmpeg / FFprobe 7.0.2。当前请求不运行额外的合成预热，验收使用真实 OC 图生视频任务。

## 首轮配置

- 两张卡共同处理一个请求：`num_gpus=2`、`tp_size=2`、`ulysses_degree=1`。
- 使用基座 BF16/FP32 权重和 CPU 分层卸载；先按 SGLang 双 32GB 示例保留 20 个 DiT block，prefetch 1，关闭 torch compile。双 RTX 5880 的实测显存和速度决定后续驻留数量。
- Larry LoRA scale 为 1.0，alpha 遵循该文件的 rank 约定；不要套用 LightX2V 旧版的 alpha 8。
- SGLang H3 的 `num_inference_steps` 计入终止零点，**8 次实际去噪应设置为 9**。`quality=lossless` 仅用于关闭额外近似，不代表使用 Turbo 后仍等同原模型。
- 不同时叠加 Cache-DiT、稀疏注意力和极低位量化；先得到可比较的样片，再逐项优化。
- 独立 Python 环境，服务监听 `127.0.0.1`，本机通过 SSH 转发访问。

首轮使用 `master-clean-v4.png`，固定镜头、768 像素短边、约 6 秒，动作是“自然站立 → 缓慢抬手 → 短暂停留 → 放下”。当前实测 Larry 8 步；原基座和 LightX2V 对照仍未运行。连续帧抠像、嘴型适配、循环接缝和动作状态图是生成后的独立工序。

## 首段出片实测

- 任务：`ff137b6d-3d29-4f3a-abd6-15279b34a3a6`，通过本地 `/v1/videos` API 生成并下载，状态 `completed`。
- 位置：本地 `output/oc-h3/oc-gesture-larry-768p-r2/sample.mp4`；服务器 `outputs/oc-gesture-larry-768p-r2/sample.mp4`。
- 原请求时长 6 秒，H3 的有效帧数规划将输出对齐为 158 帧，即 6.583333 秒。尺寸 768 × 1152，24 fps。
- 从提交到下载完毕用时 239.082 秒。去噪 201.4394 秒，解码 33.8110 秒。此数据不含安装、下载和模型启动时间。
- `nvidia-smi` 每 5 秒采样，两卡观察到的最高占用均为 24412 MiB；框架记录峰值为 23470 MB，两种统计口径不同。
- 文件 648155 字节，SHA256 `3b90e8ddf47ae034d61f9f4edddbd0288d2c217e73628fa73539c6dc8ad19269`。本地完整解码通过，远程与本地哈希一致。
- 抽帧看到袖口、上臂、前臂连续联动，回到下垂姿态；头发与衣料轮廓清楚。抬手阶段笑意偏明显，手指姿态过于刻意，因此增加保持平直嘴线、手势经过躯干前方的对照版本。
- 此片仍带背景；首尾存在绘制差异，尚未作为无缝循环、透明帧序列或实时嘴型素材接入软件。

## 第二段历史样片：视觉未通过

- 文件：`output/oc-h3/oc-gesture-reserved-768p/sample.mp4`；任务 `43284e05-cbb1-48d4-89c0-54d27763f760`。
- 使用同一母版、同一随机种子和同一 Larry 8 步配置，只将提示词改为平直嘴线、冷淡表情、前臂沿躯干前方的小幅摊手。提示词保存于 `scripts/oc-h3/gesture-reserved-prompt.txt`，请求快照也包含全文。
- 输出 593630 字节，SHA256 `5e748dc76c976505b90be82e06dc264bd446db126388d38e19e1e614250a79ac`。
- 从提交到取回文件用时 233.853 秒；框架推理时间 228.803 秒。两张卡的采样峰值分别为 24492、24490 MiB。
- 本地核对远程哈希，FFprobe 确认 158 帧 / 24 fps，全部帧解码通过。此前抽帧判断“手掌更自然、袖口正常联动”不充分：宽袖仍呈硬撑的锥形，手腕和摊开的手指刻意，母版的金蓝外光晕也被保留。用户已指出这三项，不能用运行成功抵消视觉问题。
- 当前只确认完整角色生成路线可用。此片保留背景，尚未完成透明化、首尾精确闭环、任意 TTS 口型和软件集成；不替换此前的运行角色包。
- 本地部署证据目录为 `output/oc-h3/deployment/`。两个候选的原始请求、结果、视频、媒体探测结果与检查帧均已保留。

## v5：柔光、下垂袖口与首尾姿态

- 使用内置 imagegen 修改完整母版，参考原始 `oc-references/body-and-fabric.png` 的清瘦体型和衣料。新稿为 `oc-references/master-soft-v5.png`：柔和中性照明、浅灰背景、收敛的发色高光、自然垂手。另绘 `oc-references/gesture-soft-v5.png`，将手势限定在同侧下肋，手腕与前臂顺接，宽袖余量下垂在肘部下面。
- 两张原画对应的完整提示词为 `scripts/oc-h3/master-soft-v5.prompt.txt` 和 `gesture-soft-v5-keyframe.prompt.txt`；视频使用 `gesture-soft-v5-enter.txt`、`gesture-soft-v5-exit.txt`。母版与抬手图均保留为新文件，当前软件角色包未被实验素材覆盖。
- H3 FL2VA 当前接口只接受第一帧 `0`、末帧 `-1` 或二者。按“待机→抬手”和“抬手→待机”分别生成，首尾使用明确原画，避免要求模型自由发明终点手势。每段请求 4 秒，实际帧数以输出检测为准。
- 复查服务器时 GPU 0–3 已被其他用户任务占用，选择空闲的 GPU 4、5，两者同属 **NUMA 1**。独立配置 `scripts/oc-h3/profile-soft-v5.json` 记录 UUID `GPU-7f6571d2-f216-2551-d133-4e4abe0fbaee`、`GPU-d85823ae-521b-78b9-dfbe-f3dce9eb9881`，CPU/内存优先节点随之改为 1。启动前的占用和拓扑检查仍执行，旧的 GPU 1、2 配置保留。
- 已检查每段覆盖全程的 12 格联系图、每秒 4 帧的手与袖口局部图、原尺寸抽帧，以及拼接处第 105–108 帧。抽帧中外层金蓝光晕消失，手势集中在同侧下肋，袖口余量垂在肘部下面；收手时袖口展开下落。拼接处未见明显姿态跳变，衣料和线条仍有细小重绘差异，未宣称无缝循环或逐帧手工校正。

### v5 输出与验证

| 片段 | 任务 ID | 时长 / 帧数 | 从提交到取回 | SHA256 |
| --- | --- | --- | --- | --- |
| 抬手 `oc-gesture-soft-v5-enter` | `0c01d35d-0180-4db6-9edc-b60d77d9a86e` | 4.458333 秒 / 107 | 147.512 秒 | `42406a2adad69b363c6de0624899bc89e80d00e7b146aaec1d54566af946097b` |
| 收手 `oc-gesture-soft-v5-exit` | `5ba28a8c-4c12-4a2c-9423-cf4cc358f0ec` | 4.458333 秒 / 107 | 152.561 秒 | `b08f352343d55549f5b82f99d0d23cc7401feb1605268852cd93ba1c48193a91` |

- 组合预览：[sample.mp4](../../output/oc-h3/oc-gesture-soft-v5/sample.mp4)，8.916667 秒、768 × 1152、恒定 24 fps、214 帧，SHA256 `f8da39e0e02f5bdd9cd5f2209dbc4b3cc74a26a12ae1b061719f614122bc41b0`。按抬手、收手顺序拼接原视频帧，以 24 fps 重新编码为 H.264，静音；未倒放、补帧或交叉淡化。
- 两个原视频的本地 SHA256 与服务器结果一致；两段原视频及组合预览均通过 FFmpeg 完整解码。检查图、媒体元数据、时间索引和 `checks.json` 保存在各目录的 `review/`。
- 采样最高显存：抬手两卡各 23010 MiB；收手两卡 23118 / 23116 MiB。生成后停止本部署进程组；复查 GPU 4、5 各 2 MiB、无计算进程，30010 端口关闭。
- 素材与完整提示词索引：[soft-v5-manifest.json](oc-references/soft-v5-manifest.json)。两张原画由内置 imagegen 编辑，视频由已部署的 H3 生成。该样片保留浅灰背景，尚未作为透明角色包和实时嘴型接入软件。

## 一手来源

- [Larry 模型说明](https://huggingface.co/larryvrh/MiniMax-H3-Turbo-Lora)
- [SGLang H3 双卡、LoRA 与格式支持说明](https://github.com/sgl-project/sglang/blob/954458567e10257f1d8d5ff808d2dbc74fc26967/docs/cookbook/diffusion/MiniMax/MiniMax-H3.mdx)
- [LightX2V 版本与采样参数](https://github.com/ModelTC/Minimax-H3-Turbo)
- [FastH3 Preview v1 的适用范围](https://huggingface.co/FastVideo/FastVideo-FastH3-4-step-Preview-v1-VSA-DataFree)
