// Inspect the actual provider projection; source identity is not inferred from prompt prose.
export function conversationContext(message) {
  const content = typeof message.content === "string" ? message.content
    : (message.content || []).filter(part => part.type === "text").map(part => part.text).join("\n");
  const marker = "[conversation_context]\n";
  if (!content.startsWith(marker)) return null;
  const frame = JSON.parse(content.slice(marker.length));
  if (!frame.text.trimStart().startsWith("{")) return null;
  return { origin: frame.origin, data: JSON.parse(frame.text) };
}
