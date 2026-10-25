// Provider roles describe transport; authorship comes from durable origin metadata.
export function messageSource(message) {
  if (message.role === "assistant") return "assistant";
  if (message.parts?.some(part => part.type === "tool_result")) return "tool";
  return message.origin || "unspecified";
}
export const messageSourceLabels = {
  assistant: "助手", human: "你", engine: "引擎消息", summary: "历史摘要",
  notification: "异步通知", agent: "Agent 消息", tool: "工具结果", unspecified: "来源未记录",
};
