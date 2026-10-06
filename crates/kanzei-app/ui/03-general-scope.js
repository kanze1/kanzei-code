// Scope identity has no UI imports: foundational modules must not pull the
// full conversation controller into their initialization cycle.
let generalRoot = null;
let conversationRoot = null;
let modeGeneration = 0;
let controller = null;
const rootKey = root => String(root || "").replaceAll("\\", "/").replace(/\/$/, "").toLowerCase();
export function setGeneralChatRoot(root) { generalRoot = root; }
export function generalChatRoot() { return generalRoot; }
export function setConversationRoot(root) { conversationRoot = root; }
export function isGeneralChat(root = conversationRoot) { return !!generalRoot && rootKey(root) === rootKey(generalRoot); }
export function registerGeneralChatController(open) { controller = open; }
export function openGeneralChat(options) {
  return controller ? controller(options) : import("./29-general-chat.js").then(module => module.openGeneralChat(options));
}
export function rememberConversationMode(mode) {
  const generation = ++modeGeneration;
  void import("./03-layout.js").then(({ setLayoutPref, flushLayout }) => {
    if (generation !== modeGeneration) return;
    setLayoutPref("prefs", "conversation_mode", mode); flushLayout();
  });
}
export function syncGeneralChatView(view = document.body.dataset.view, translate = text => text) {
  const general = document.body.dataset.space === "dev" && isGeneralChat();
  document.body.dataset.generalChat = String(general);
  document.getElementById("workbench-general-chat")?.classList.toggle("active", general && view === "chat");
  const name = document.getElementById("project-space-name");
  if (general && view === "chat" && name) name.textContent = translate("无项目对话");
  const label = document.getElementById("new-chat")?.querySelector("span");
  if (label) {
    const key = document.body.dataset.space === "research" ? "新建课题对话" : "新对话";
    label.dataset.i18nKey = key; label.textContent = translate(key);
  }
}
