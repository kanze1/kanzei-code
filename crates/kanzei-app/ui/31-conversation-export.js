// Pure, structured Markdown export. The backend supplies the visible transcript.
const fence = (text, language = "") => {
  const runs = String(text).match(/`+/g) || [];
  const marker = "`".repeat(Math.max(3, ...runs.map(run => run.length + 1)));
  return `${marker}${language}\n${text}\n${marker}`;
};
export function conversationMarkdown({ name, project, sessionId, messages }) {
  const lines = [`# ${String(name || "Conversation").replace(/\n/g, " ")}`, "", `- Project: ${project}`, `- Session: ${sessionId}`, ""];
  for (const message of messages || []) {
    lines.push(`## ${message.role === "user" && message.parts?.some(part => part.type === "tool_result") ? "Tool results" : ({ user: "User", assistant: "Assistant", system: "System" })[message.role] || message.role}`, "");
    for (const part of message.parts || []) {
      if (part.type === "text") lines.push(part.text, "");
      else if (["thinking", "reasoning"].includes(part.type)) lines.push("<details><summary>Thinking</summary>", "", part.text || part.thinking || "", "", "</details>", "");
      else if (["tool_use", "tool_call"].includes(part.type)) lines.push(`### Tool: ${part.name} (${part.id || ""})`, "", fence(JSON.stringify(part.input, null, 2), "json"), "");
      else if (part.type === "tool_result") lines.push(`### Result: ${part.call_id}${part.is_error ? " (error)" : ""}`, "", fence(part.content), "");
      else if (["image", "document"].includes(part.type)) lines.push(`[${part.type}: ${part.source?.path || part.media_type || "attachment"}]`, "");
      else lines.push(fence(JSON.stringify(part, null, 2), "json"), "");
    }
  }
  return lines.join("\n");
}
