# 本地语音交互

- 身份: live_design
- 实现基线: 788dc43e

输入区的「语音」开启当前会话的持续对话。停顿后识别并发送，文字回复按句进入合成队列，PCM 到达后连续播放。字幕、人物说话姿势和嘴型依据实际播放时间推进。再次点击「结束语音」释放麦克风；「打断」停止当前回复和全部待播音频，继续听用户讲话。

## 参考与实现

参考 [Amadeus 60369c4](https://github.com/Code-Amadeus/Amadeus/tree/60369c48db23e0d7f21c11c6b94c25989dae29cb) 的 sentence scheduler、playback mouth signal、barge-in 和 interrupt epoch 设计。这里是独立实现，没有复制其代码或角色资源。

本项目的模型组合使用本机 Faster-Whisper small（CPU int8）识别和 Qwen3-TTS 1.7B Base 克隆。合成由 vLLM-Omni 的异步音频分块提供，接收 24kHz 单声道 PCM16；不把一次性生成后切片冒充流式推理。接口依据 [vLLM-Omni speech API](https://docs.vllm.ai/projects/vllm-omni/en/stable/serving/speech_api/) 实现。

选定音色沿用用户音色项目的 `artifacts/trilingual-01/ja_a75.wav`，与此前被接受的英文直接克隆方案一致，使用 `x_vector_only_mode`。不改写用户原模型，也不把中文候选标记为已验收。

## 安装与运行

需要 Windows、Ubuntu WSL2、NVIDIA GPU、Windows Python/uv。依赖安装在 WSL 的 `~/.venvs/kanzei-voice`，不修改系统 Python。模型及配置保存在 Windows 用户目录的 `.kanzei/voice-runtime`。

```powershell
./scripts/voice/setup.ps1 -VoiceDirectory '音色项目的绝对路径'
./scripts/voice/start.ps1
./scripts/voice/stop.ps1
```

首次安装下载模型和 CUDA 推理依赖；首次启动编译 GPU 内核，时间长于后续启动。进入桌面端后点击「语音」，允许麦克风即可。语音设置中的「保存并检查」显示实际服务是否就绪。网关监听 `127.0.0.1:7388`，TTS 监听 WSL 内的 `127.0.0.1:8091`。服务日志是私有运行目录下的 `gateway.log`、`tts.log`、`supervisor.log`。`start.ps1` 只启动服务；不会自动打开麦克风。

核心推理依赖版本固定在 `scripts/voice/requirements.txt`，模型版本记录在运行目录 `models.json`。CUDA 编译检查会先执行采样内核；链接别名和 CUDA 依赖都在私有 venv 内。WSL 的显存统计可能漏算 Windows 进程占用，所以单人对话的 KV 缓存明确限制为 512MiB，避免按空闲显存自动过量分配。

两个子进程由同一 supervisor 管理；停止脚本验证 PID 的命令行后只停止该运行实例。组件失败会一并收回该实例的子进程。

## 数据与动作

- 麦克风通过 WebRTC 请求回声消除、降噪和自动增益；AudioWorklet 转成 16kHz 单声道。语音起点需持续 220ms；播放时需 260ms，停顿 700ms 后送识别，单段上限 30 秒。
- 只有设备确认支持回声消除时才启用播放中的自动插话；否则播报时暂停识别，仍可点击「打断」。当前起点判断是音量门限，服务端使用 Whisper 的 Silero VAD 再过滤录音；尚未把神经 VAD 接到浏览器的实时插话判断中。
- 识别结果送入原有 `sendText → run_prompt` 通路，复用当前项目、线路、模型与权限流程。不会把思考文本送去播报。Markdown 代码围栏被过滤，文字只按语义标点或长度边界拆分。
- 合成请求串行，最多保留 32 段；播放缓冲超过 3 秒时延缓下一段合成。音频包带请求、会话、序号和结束标志，处理跨包的半个 PCM 采样。
- 播放起点保留 280ms 缓冲，避免第一个 80ms codec 包与后续块之间出现断音。AudioContext 的播放时间与 Analyser 的实际输出幅度驱动字幕和嘴型。文本到达和合成请求发出都不会提前张嘴。

## 生命周期

每次开启语音都有独立所有权标识；每次打断提升回复代次，立即停止 AudioBufferSource、关闭嘴型、清空文本和音频队列，再取消 HTTP 合成/识别及当前 LLM 运行。迟到 PCM、迟到识别结果与旧会话事件均被丢弃。显式 PCM 结束消息防止 IPC command 的返回越过音频通道中的尾包。

切换项目、会话、主视图，隐藏窗口或结束语音会释放麦克风、AudioContext 和队列。退出语音不会取消用户的普通文本任务；「打断」会停止当前回复。音频没有云端回退和自动保存；识别出的文本按普通对话历史保存。

## 验证入口

```powershell
node scripts/ui-voice-smoke.mjs
cargo test -p kanzei-app voice::tests --no-default-features
wsl -d Ubuntu --exec /home/USER/.venvs/kanzei-voice/bin/python /path/to/scripts/voice/test_service.py
python scripts/voice/check_runtime.py --input recording-16k-mono.wav --text '嗯，我在。' --language zh --output output/voice/check
```

纯逻辑测试覆盖跨 token 代码过滤、PCM 对齐、短噪声、队列串行、打断和跨会话迟到结果。浏览器测试需要真实 Web Audio；实际麦克风、扬声器回声消除与已安装 Tauri 应用需要独立验证，不能用模拟接口通过替代。

2026-09-21 本机验证：15.52 秒英文参考音频约 2.03 秒转写；6.8 秒中文样本约 1.19 秒转写。限制显存后的中文合成样本长 7.12 秒，首包 1.36 秒，总生成 2.44 秒；热身后的英文样本长 5.6 秒，首包 0.30 秒，总生成 1.16 秒。结果是本次样本测量，不是延迟保证。

浏览器使用虚拟麦克风播放参考 WAV，真实 AudioWorklet、Whisper、Qwen3-TTS 和 Web Audio；LLM 回复、Tauri 桥为测试替身。识别文本进入原 `sendText`，播放出现 0/2/3 嘴型；注入 260ms 连续语音帧后，下一次检查时音源、缓冲和队列已清空，旧回复停止；切换视图后音轨为 ended，AudioContext 已释放。1280×840 和最小 800×500 窗口控件均可见，没有浏览器异常。

另外编译并启动了真实 Tauri 开发版，用独立 `KANZEI_HOME` 和 WebView2 数据目录验证原生接口。该轮没有替换 IPC：虚拟麦克风经 AudioWorklet 返回 16 帧；`voice_speak` 通过真实 Tauri Channel 返回 20 包、5.44 秒音频，生成约 1.19 秒；Web Audio 播放幅度有效；同一音频再次经 `voice_transcribe` 返回中文。测试后释放录音和播放资源，并关闭测试进程。证据保存在 `output/voice/`。实体麦克风与扬声器的回声消除效果尚未验证。以上实机记录来自开发版；正式安装包的发布验证另见对应 GitHub Release。
