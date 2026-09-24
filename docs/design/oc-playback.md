# 角色动画播放与性能

2026-09-24，针对 build-534e6be0 的卡顿反馈。

## 调查

| 路线 | 制作与播放 | 本项目取舍 |
| --- | --- | --- |
| Amadeus / SpriteForge | 离线制作动作，导出按时间索引的 KTX2 帧、动作图与嘴部覆盖层；播放器限制纹理缓存 | 适合逐帧控制。大量高清连续帧需要额外的纹理编码、加载与显存预算 |
| Tencent VAP / YYEVA | 把 RGB 与透明信息打包进 MP4，利用视频解码器解码，在 GPU 上恢复透明画面 | 适合当前的完整角色片段；沿用 H.264 和已有动作图 |
| 当前 v6 | 普通背景视频，运行时逐像素抠像、处理边缘、替换嘴部，再混合两段动作 | 制作步骤尚未离线完成，运行 shader 比播放本身复杂 |

来源：[Amadeus 角色包规范](https://github.com/Code-Amadeus/Amadeus/blob/main/docs/character_pack_authoring.md)、[SpriteForge](https://github.com/Code-Amadeus/Amadeus-SpriteForge)、[VAP 原理](https://github.com/Tencent/vap/blob/master/Introduction.md)、[YYEVA](https://github.com/yylive/YYEVA)。参考资源表示方式，播放器继续由本项目实现。VAP 开源仓库已声明停止维护，本项目没有引入该 SDK。

## 查到的问题

v6 的运行帧率已经有限制，并非每次显示器刷新都上传一帧。旧的计时方式每次把时间起点直接重置为当前刷新时刻，在 60 Hz 下会把 24 fps 的更新间隔舍入成 50 ms。高刷新率显示器可能刚好避开这个问题。

8 秒的本机 Chromium / RTX 4090 待机基线约为 23.95 fps、0 次跳转定位、主线程渲染调用 P95 约 0.30 ms。这个结果不能解释所有桌面端停顿；它没有覆盖安装版 WebView2、语音时的主线程竞争和动作首次加载。

另外，旧播放器把视频时间与墙钟比较，误差超过 130 ms 就重新 seek。解码一旦停顿，就可能连续要求重新定位。普通状态也会计算两份完整抠像 shader，画布保留读取缓冲且按最高 2 倍像素比绘制。

## v7 实现

1. 制作阶段烘焙透明边缘和闭嘴画面，输出上半 RGB、下半灰度 Alpha 的 H.264 视频。逻辑画幅 576 × 864，编码画幅 576 × 1728。
2. 收到 `requestVideoFrameCallback` 才上传新视频纹理；普通时刻只采样当前动作，过渡时才读取两段。参考：[视频帧回调](https://web.dev/articles/requestvideoframecallback-rvfc)、[PixiJS 7 VideoResource](https://pixijs.download/v7.4.3/docs/packages_core_src_textures_resources_VideoResource.ts.html)。
3. 片段内部使用视频自身的时钟，切片时定位一次；资源尚未准备好时暂停动作图时钟。显式逐帧导出仍使用精确 seek。
4. 画布最多按 1.5 倍像素比绘制，且不超过素材的有效分辨率；读取缓冲仅用于导出。
5. 角色默认关闭。开启时加载，关闭时销毁画布、停止视频并释放解码实例。文字和语音功能保持可用。

还修复了已播视频被复用为预加载项后仍继续播放的问题。缓存最多四项，稳定播放时运行一个视频，过渡时最多两个；预加载项保持暂停。

## 本轮复测

- 最终素材的同机 Chromium 8 秒样本：约 23.83 次绘制/秒、23.46 次视频纹理更新/秒，渲染调用 P95 约 0.30 ms，片段内部 0 次 seek。解码回调间隔 P95 45.8 ms、最大 46 ms，回调记录没有重复媒体帧。该样本包含一次待机切换；这台高配置电脑的平均帧率没有显著提高，不能据此承诺所有桌面端卡顿均已消失。
- 36 个片段首、中、尾位置完成实际解码与可见像素检查，最大缓存四项。另检查实际 Web Audio 开合、打断、隐藏暂停、减少动态效果、画布迁移与会话隔离。
- 角色开关检查覆盖默认零素材请求、开关持久化、关闭释放、窄窗口和连续切换。稳定状态确认只有一个视频播放。

运行测量、UI 截图和媒体检查保存到 `output/oc-v7/`。安装版的实际性能应与浏览器测量分别记录。

性能脚本同时记录原生 `getVideoPlaybackQuality()`。本机离屏视频仍报告较多 dropped 帧，和每秒约 23 次不同媒体时间的画布上传并不一致；该数值不能直接作为画布丢帧率。Chromium 对外部画布取帧有独立的已绘制标记逻辑（[实现](https://github.com/chromium/chromium/blob/main/third_party/blink/renderer/platform/media/video_frame_compositor.cc)），此处保留原始统计与回调、纹理更新两组证据，供安装版继续对照。
