# OC 角色与动画制作记录

更新：2026-09-24。角色包 `2026.09.24.7`，格式 `kanzei.character-pack.v3`。v7 用炭黑工作衫和黄铜肩线呼应软件配色；短袖自然垂落，保留清瘦中性体型、冷静眼神、金色眼睛和青色脸部裂纹。

## 素材与动作

- 母版：[master-workwear-v7.png](oc-references/master-workwear-v7.png)，1024 × 1536。使用内置 imagegen 编辑；完整提示词：[master-workwear-v7.prompt.txt](../../scripts/oc-h3/master-workwear-v7.prompt.txt)。
- 动作计划：[action-plan-v7.json](../../scripts/oc-h3/action-plan-v7.json)。11 个视频，九种状态，三个 8 秒待机变体。
- 视频原稿：768 × 1152、24 fps，H3 FL2VA + Larry v4-600 EMA。GPU 4、5 同属 NUMA 1，固定版本见[部署记录](oc-h3-deployment.md)。
- 发布素材：逻辑画幅 576 × 864，编码画幅 576 × 1728，上半 RGB、下半 Alpha。资源清单和 SHA256 在 [character-v7.json](../../crates/kanzei-app/ui/assets/oc/character-v7.json)。
- 抬手、停留和放手是一段完整表演，袖子、手腕与肘部共同运动。活跃手势结束后才切换身体状态，声音和嘴型立即响应打断。

待机使用完整视频纵向稳定，保留眨眼、视线与衣料变化。轻呼吸片段处理后头部纵向峰峰位移约 1.52 px、领口 1.81 px、下肋 6.37 px、腰胯 0.25 px。数值来自光流测量；视觉检查另外覆盖首、中、尾帧和循环接缝。其余两个待机变体采用同一处理参数。

## 播放与界面

透明边缘和闭嘴画面在制作阶段烘焙。播放器采样视频的颜色与透明平面，张嘴时才在口部局部区域覆盖嘴型。跟踪数据与真实解码帧时间对应。技术调查、实现和性能证据见[播放方案](oc-playback.md)。

左侧“角色”开关默认关闭并保存选择。关闭后停止视频并销毁画布；语音状态、输入框和对话记录保持可用。角色开启时，空字幕使用“说吧，今天折腾什么。”“等一下，我捋捋。”等状态短句；收到真实转写或回复后优先显示原文。

配音沿用 `Kanzei OC CN C`。样片重新录制四句台词，时间、文字和音频哈希见 [demo-v7.json](../../scripts/oc-h3/demo-v7.json)。音频通过本机已配置的语音服务生成，仍由实际 Web Audio 包络驱动嘴型。

## 复现与检查

原始请求、素材和结果哈希保存在 `output/oc-h3/<job>/`。`review.py` 完整解码并抽帧，`analyze_motion.py` 记录嘴部跟踪与区域位移，`stabilize_idle.py` 处理待机，`bake_alpha.py` 生成透明视频，`pack_v7.py` 检查逐段复审记录后写入运行清单。

`node scripts/oc-preview.mjs --export --v7` 启动交互预览和逐帧导出服务；导出目录为 `output/oc-film-v7/`。界面截图与实测记录保存在 `output/oc-v7/` 和 `output/playwright/`。浏览器检查与安装版 WebView2 检查分别记录。

原始用户参考和制作母版保留。退出使用的旧运行视频不参与当前角色包加载。

v7 的 11 个运行视频合计 12,181,261 字节。v6 的 28 份运行文件已移入 `C:/Users/kanzei/Documents/kanzei-oc-archive/2026-09-24-v6-runtime/`，逐项核对 SHA256 后退出软件资源目录。H3 服务已停止，GPU 4、5 各恢复到 4 MiB，30010 端口关闭。
