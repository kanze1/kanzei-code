import { $, defer } from "./01-core.js";
import { closeSurface } from "./00-surface.js";
import { activeProcessId, activeSessionId, running, toastError } from "./03-shell.js";
import { currentGoalText, renderGoalState, syncAutoRunState, cancelAutoContinueTimer, setAutoPaused } from "./08-auto.js";
import { rememberAutoUiState, sendText, syncAutoContinueWithProfile } from "./08-compose-runtime.js";

function syncControls() {
  const autonomous = $("profile-select").value === "dev-auto";
  $("autorun-bar").hidden = !autonomous && !currentGoalText().trim();
  $("goal-picker").classList.toggle("has-goal", Boolean(currentGoalText().trim()));
}
defer(() => {
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
