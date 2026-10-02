import { openDialog, closeSurface } from "./00-surface.js";
import { invoke, on, uiPrefsLoad } from "./01-core.js";
import { currentProject, toast, toastError } from "./03-shell.js";
import { t } from "./02-i18n.js";
import { isGeneralChat } from "./03-general-scope.js";

function element(tag, text = "", className = "") {
  const node = document.createElement(tag); node.textContent = text; node.className = className; return node;
}
function control(text, action) {
  const button = element("button", t(text)); button.type = "button";
  button.addEventListener("click", () => { void Promise.resolve().then(action).catch(error => toastError(String(error))); }); return button;
}
function field(label, input) { input.setAttribute("aria-label", t(label)); const wrapper = element("label"); wrapper.append(element("span", t(label)), input); return wrapper; }
function input(value = "", type = "text") { const node = document.createElement("input"); node.type = type; node.value = String(value); return node; }
function select(options, value) {
  const node = document.createElement("select");
  for (const [key, label] of options) { const option = element("option", t(label)); option.value = key; node.appendChild(option); }
  node.value = value; return node;
}
export async function showSchedules() {
  const prefs = await uiPrefsLoad(); const projects = prefs?.projects || [];
  const dialog = document.getElementById("schedules-overlay"); dialog.replaceChildren(); dialog.setAttribute("aria-label", t("定时任务"));
  const header = element("div", "", "schedule-header"), title = element("h2", t("定时任务"));
  const project = select([...new Set([currentProject, ...projects].filter(Boolean))].map(path => [path, isGeneralChat(path) ? t("无项目对话") : path]), currentProject || projects[0]);
  project.setAttribute("aria-label", t("项目"));
  header.append(title, project, control("关闭", () => closeSurface(dialog)));
  const content = element("div", "", "schedule-content"); dialog.append(header, content);
  const call = (action, args = {}) => invoke("schedule_action", { projectDir: project.value, action, ...args });
  const reload = async () => {
    content.replaceChildren();
    if (!project.value) { content.appendChild(element("p", t("请先添加项目"))); return; }
    const payload = await call("list");
    const toolbar = element("div", "", "schedule-header"); toolbar.append(control("新建定时任务", () => edit(null)), control("刷新", reload)); content.appendChild(toolbar);
    for (const diagnostic of payload?.diagnostics || []) content.appendChild(element("p", `${diagnostic.file}:${diagnostic.line} ${diagnostic.message}`, "schedule-error"));
    const tasks = payload?.tasks || [];
    if (!tasks.length) content.appendChild(element("p", t("暂无定时任务"), "dim"));
    for (const item of tasks) {
      const def = item.definition; const row = element("section", "", "schedule-row");
      const last = item.history?.find(event => event.type === "schedule.run_finished")?.data;
      const heading = element("h3", def.name);
      const detail = element("p", `${def.enabled ? t("已启用") : t("已停用")} · ${def.when} · ${def.host} · ${t("下次运行")} ${new Date(item.next_ms).toLocaleString()}`);
      const result = element("p", last ? `${last.ok ? "✓" : "✗"} ${last.summary} · ${Math.round(last.duration_ms / 1000)}s` : t("尚未运行"));
      const actions = element("div", "", "schedule-actions");
      actions.append(control(def.enabled ? "停用" : "启用", async () => { await call("toggle", { name: def.name, expectedHash: item.revision, enabled: !def.enabled }); await reload(); }), control("立即运行", async () => { await call("run", { name: def.name }); toast(t("任务已排队，结果见运行历史")); }), control("编辑", () => edit(item)), control("运行历史", async () => history(def.name)), control("删除", async () => { await call("delete", { name: def.name, expectedHash: item.revision }); await reload(); }));
      row.append(heading, detail, result, actions); content.appendChild(row);
    }
  };
  const history = async name => {
    const events = await call("history", { name }); content.replaceChildren(control("返回", reload), element("h3", name));
    for (const event of events || []) {
      const row = element("details"); row.appendChild(element("summary", `${event.type} · ${event.data.summary || event.data.reason || event.data.run_id || ""}`));
      row.appendChild(element("pre", JSON.stringify(event.data, null, 2)));
      if (event.type === "schedule.run_finished" && event.data.run_id) row.appendChild(control("查看完整对话", async () => {
        const response = await invoke("conversation_get", { projectDir: project.value, processId: event.data.run_id, sequence: null });
        const messages = Array.isArray(response) ? response : response?.messages || [];
        const transcript = element("div", "", "schedule-transcript");
        for (const message of messages) { transcript.appendChild(element("h4", message.role)); for (const part of message.parts || []) transcript.appendChild(element("pre", part.text || part.content || JSON.stringify(part))); }
        row.appendChild(transcript);
      }));
      content.appendChild(row);
    }
  };
  const edit = item => {
    const def = item?.definition || { name: "", enabled: true, when: "每天 09:00", host: "app", catch_up: "once", agent: isGeneralChat(project.value) ? "general" : "readonly", model: "primary", timeout_secs: 1800, max_steps: 32, steps: [{ prompt: "" }], writeback: ["notify"], body: "" };
    content.replaceChildren(control("返回", reload), element("h3", t(item ? "编辑定时任务" : "新建定时任务")));
    const form = element("form", "", "schedule-editor");
    const name = input(def.name); name.required = true; name.readOnly = Boolean(item);
    const patterns = [["daily", "每天"], ["weekdays", "工作日"], ["weekly", "每周"], ["minutes", "每 N 分钟"], ["hours", "每 N 小时"]];
    const kind = select(patterns, def.when.startsWith("每周") ? "weekly" : def.when.startsWith("工作日") ? "weekdays" : def.when.endsWith("分钟") ? "minutes" : def.when.endsWith("小时") ? "hours" : "daily");
    const at = input(def.when.match(/\d\d:\d\d/)?.[0] || "09:00", "time");
    const interval = input(def.when.match(/每 (\d+) /)?.[1] || "15", "number"); interval.min = "1"; interval.max = "60";
    const day = select([...["一", "二", "三", "四", "五", "六", "日"].map(day => [day, `周${day}`])], def.when.match(/每周(.)/)?.[1] || "一");
    const enabled = input("", "checkbox"); enabled.checked = def.enabled;
    const host = select([["app", "应用打开时"], ["system", "应用关闭时也运行"], ["server", "登记服务器"]], def.host.startsWith("server:") ? "server" : def.host);
    const environment = input(def.host.replace(/^server:/, ""));
    const agent = select(isGeneralChat(project.value) ? [["general", "无项目对话"], ["readonly", "只读"]] : [["readonly", "只读"], ["research", "研究"], ["dev", "开发（独立工作树）"]], def.agent);
    const model = input(def.model), timeout = input(Math.ceil(def.timeout_secs / 60), "number"), stepsLimit = input(def.max_steps, "number"); timeout.min = "1"; timeout.max = "360"; stepsLimit.min = "1"; stepsLimit.max = "1000";
    const catchup = select([["once", "错过后补跑一次"], ["skip", "错过则跳过"]], def.catch_up);
    form.append(field("任务名称", name), field("启用", enabled), field("频率", kind), field("时刻", at), field("星期", day), field("间隔", interval), field("运行主机", host), field("服务器标识", environment), field("运行模式", agent), field("模型", model), field("超时（分钟）", timeout), field("最多步骤", stepsLimit), field("错过时", catchup));
    const update = () => { at.closest("label").hidden = ["minutes", "hours"].includes(kind.value); interval.closest("label").hidden = !["minutes", "hours"].includes(kind.value); day.closest("label").hidden = kind.value !== "weekly"; environment.closest("label").hidden = host.value !== "server"; }; kind.addEventListener("change", update); host.addEventListener("change", update); update();
    const steps = element("div", "", "schedule-steps");
    const addStep = step => {
      const row = element("div", "", "schedule-step"); const type = select([["prompt", "代理指令"], ["run", "命令"]], Object.keys(step)[0]); const text = element("textarea"); text.value = Object.values(step)[0]; text.required = true; text.rows = 4;
      row.append(field("步骤类型", type), field("步骤内容", text), control("移除此步骤", () => row.remove())); steps.appendChild(row);
    };
    def.steps.forEach(addStep); form.append(steps, control("添加步骤", () => addStep({ prompt: "" })));
    const channels = element("div", "", "schedule-writeback"), checked = new Map();
    for (const [key, label] of [["notify", "通知"], ["memory_inbox", "记忆草稿"], ...(isGeneralChat(project.value) ? [] : [["idea", "灵感登记"]])]) { const checkbox = input("", "checkbox"); checkbox.checked = def.writeback.includes(key); checked.set(key, checkbox); channels.appendChild(field(label, checkbox)); }
    const file = input(def.writeback.find(channel => channel.startsWith("file:"))?.slice(5) || ""); channels.appendChild(field("回写文件（相对项目，可留空）", file)); form.appendChild(channels);
    const save = element("button", t("保存")); save.type = "submit"; form.appendChild(save);
    form.addEventListener("submit", event => {
      event.preventDefault(); save.disabled = true;
      const when = kind.value === "minutes" ? `每 ${interval.value} 分钟` : kind.value === "hours" ? `每 ${interval.value} 小时` : kind.value === "weekly" ? `每周${day.value} ${at.value}` : `${kind.value === "weekdays" ? "工作日" : "每天"} ${at.value}`;
      const definition = { ...def, name: name.value, enabled: enabled.checked, when, host: host.value === "server" ? `server:${environment.value}` : host.value, agent: agent.value, model: model.value, catch_up: catchup.value, timeout_secs: Number(timeout.value) * 60, max_steps: Number(stepsLimit.value), steps: [...steps.children].map(row => ({ [row.querySelector("select").value]: row.querySelector("textarea").value })), writeback: [...checked].filter(([, checkbox]) => checkbox.checked).map(([key]) => key).concat(file.value.trim() ? [`file:${file.value.trim()}`] : []) };
      void call("save", { name: definition.name, definition, expectedHash: item?.revision || null }).then(reload).catch(error => { toastError(String(error)); save.disabled = false; });
    });
    content.appendChild(form);
  };
  project.addEventListener("change", () => { void reload().catch(error => toastError(String(error))); });
  openDialog(dialog, { onClose: () => dialog.replaceChildren() }); await reload();
}
on("kz:schedule-run", payload => { if (payload.notify) toast(payload.error || `${payload.name}: ${payload.result?.summary || t("已完成")}`); });
