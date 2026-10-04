import { $, defer, invoke } from "./01-core.js";
import { closeSurface } from "./00-surface.js";
import { t } from "./02-i18n.js";
import { activeProcessId, activeSessionId, currentProject, running, toastError } from "./03-shell.js";
import { currentGoalText, renderGoalState, syncAutoRunState, cancelAutoContinueTimer, setAutoPaused } from "./08-auto.js";
import { rememberAutoUiState, sendText, syncAutoContinueWithProfile } from "./08-compose-runtime.js";

const node = (tag, cls, text) => { const el = document.createElement(tag); el.className = cls; if (text) el.textContent = text; return el; };
let owner = "", request = 0, selected = [], busy = false;
function updateSkillsLabel() {
  const picker = $("skills-picker");
  if (picker) { picker.textContent = selected.length ? `Skills · ${selected.length}` : "Skills"; picker.title = selected.map(name => `$${name}`).join(" · ") || t("为本对话绑定 Skills"); }
}
async function loadSkills() {
  const token = ++request, project = currentProject, process = activeProcessId, session = activeSessionId;
  if (!project || !session) return;
  const menu = $("skills-menu"), list = menu.querySelector(".skills-list");
  list.textContent = t("正在读取…");
  try {
    const [skills, names] = await Promise.all([
      invoke("skills_list", { projectDir: project }), invoke("skills_get_binding", { projectDir: project, processId: process }),
    ]);
    if (token !== request || session !== activeSessionId) return;
    selected = names || []; updateSkillsLabel(); list.replaceChildren();
    const missing = selected.filter(name => !skills.some(skill => skill.name === name && skill.userInvocable));
    for (const name of missing) skills.push({ name, description: t("已绑定的 Skill 不可用，请取消绑定或恢复文件"), userInvocable: true });
    for (const skill of skills.filter(skill => skill.userInvocable)) {
      const row = node("label", "skills-row"); const checkbox = node("input", ""); checkbox.type = "checkbox"; checkbox.checked = selected.includes(skill.name);
      const text = node("span", ""); text.append(node("strong", "", `$${skill.name}`), node("small", "dim", skill.description)); row.append(checkbox, text); row.title = skill.path || "";
      checkbox.addEventListener("change", async () => {
        if (busy || session !== activeSessionId) { checkbox.checked = selected.includes(skill.name); return; }
        const previous = [...selected]; busy = true; for (const input of list.querySelectorAll("input")) input.disabled = true;
        try {
          const next = checkbox.checked ? [...selected, skill.name] : selected.filter(name => name !== skill.name);
          const saved = await invoke("skills_bind", { projectDir: project, processId: process, names: next });
          if (session === activeSessionId && token === request) { selected = saved; updateSkillsLabel(); }
        } catch (error) { checkbox.checked = previous.includes(skill.name); toastError(String(error)); }
        finally { busy = false; for (const input of list.querySelectorAll("input")) input.disabled = false; }
      });
      list.append(row);
    }
    if (!list.children.length) list.append(node("p", "dim", t("未发现 Skills。把 SKILL.md 放入项目或用户的 .agents/skills、.claude/skills、.codex/skills 或 .kanzei/skills。")));
  } catch (error) { if (token === request) list.textContent = String(error); }
}
function syncControls() {
  if (owner !== activeSessionId) { owner = activeSessionId; selected = []; updateSkillsLabel(); closeSurface($("skills-menu")); void loadSkills(); }
  const autonomous = $("profile-select").value === "dev-auto";
  $("autorun-bar").hidden = !autonomous && !currentGoalText().trim();
  $("goal-picker").classList.toggle("has-goal", Boolean(currentGoalText().trim()));
}
defer(() => {
  const menu = $("skills-menu");
  const head = node("div", "skills-head"); head.append(node("strong", "", "Skills"), node("small", "dim", t("绑定后每轮使用，也可直接输入 $技能名")));
  const refresh = node("button", "ghost mini", t("刷新")); refresh.type = "button"; refresh.addEventListener("click", () => void loadSkills()); head.append(refresh);
  menu.append(head, node("div", "skills-list")); $("composer").append(menu);
  $("skills-picker").addEventListener("click", () => void loadSkills());
  $("goal-start").addEventListener("click", async () => {
    const goal = currentGoalText().trim(); if (!goal) { $("auto-goal").focus(); return; }
    const session = activeSessionId, process = activeProcessId;
    $("auto-continue").checked = true; setAutoPaused(false); rememberAutoUiState(activeProcessId, ["enabled", "paused"]);
    try {
      await syncAutoRunState({ enabled: true, paused: false, goal });
      if (session !== activeSessionId || process !== activeProcessId) return;
      renderGoalState(); syncControls(); closeSurface($("goal-menu"));
      if (!running) await sendText(goal);
    } catch (error) { toastError(String(error)); }
  });
  $("goal-clear").addEventListener("click", async () => {
    $("auto-goal").value = ""; cancelAutoContinueTimer(); syncAutoContinueWithProfile();
    try { await syncAutoRunState({ goal: "", enabled: $("auto-continue").checked }); renderGoalState(); syncControls(); }
    catch (error) { toastError(String(error)); }
  });
  $("auto-goal").addEventListener("change", syncControls); $("profile-select").addEventListener("change", syncControls);
  document.addEventListener("kz:goal-restored", syncAutoContinueWithProfile);
  for (const event of ["kz:conversation-selected", "kz:session-state-changed", "kz:view-changed", "kz:goal-restored"]) document.addEventListener(event, syncControls);
  syncControls();
});
