// These lines are only an empty-transcript presence. Recorded speech always wins.
export const VOICE_STATUS = {
  off: "语音已关闭", connecting: "正在准备语音服务…", listening: "聆听", hearing: "收音中",
  recognizing: "识别中", thinking: "思考中", speaking: "回应中", error: "连接中断",
};
const PLAIN_LINES = {connecting:"连接语音", listening:"开始语音对话", hearing:"正在接收语音", recognizing:"正在转写", thinking:"正在处理", speaking:"正在回复"};

export function voicePresenceLine(state) {
  return PLAIN_LINES[state] || "";
}
