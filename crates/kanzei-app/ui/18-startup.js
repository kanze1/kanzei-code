import { bindMenus, installTooltips } from "./00-surface.js";
import { navigate_view, currentProject } from "./03-shell.js";
import { defer } from "./01-core.js";
import { $, invoke, uiPrefsLoad } from "./01-core.js";
import { LANGUAGE_PREFERENCES, syncLanguagePreferenceFromSettings, t } from "./02-i18n.js";
import { log, setStatus, toast, toastError } from "./03-shell.js";
import { renderProjects, lastProjectPrefs } from "./09-sessions.js";
import { refreshWorkspace } from "./12-docs-pages.js";
import { persistLanguagePreference, updateResultText } from "./16-settings.js";
import { active_space, restore_workspace_preferences, restore_active_workspace, sync_workspace_visibility } from "./03-workspaces.js";
import { workbenchNavigationGuard, openProjectSpace } from "./12-workbench.js";
import { openGeneralChat } from "./03-general-scope.js";

// ---------- 启动 ----------
// 弹层原语接线:静态菜单触发器([data-kz-menu])与全局 tooltip(接管 title)。
// 放在启动链之外、不等任何 IPC:菜单与提示在首屏就要能用。
defer(() => {
  bindMenus(document);
  installTooltips(document);
});
defer(() => {
  (async () => {
    // UX-119:语言的持久化真源是 app.json 的 ui_layout.prefs.language(切换即存);localStorage 只是首屏缓存。
    // 旧版本把它写在全局 kanzei.toml(要点「保存」才落盘):app.json 里还没有时沿用 toml 的值,并一次性迁过去。
    // 都没有则维持中文默认,不向任何文件写默认值。
    try {
      const stored = (await uiPrefsLoad())?.ui_layout?.prefs?.language;
      if (LANGUAGE_PREFERENCES.has(stored)) {
        syncLanguagePreferenceFromSettings(stored);
      } else {
        const settings = await invoke("settings_get", { projectDir: null });
        if (LANGUAGE_PREFERENCES.has(settings.language)) {
          syncLanguagePreferenceFromSettings(settings.language);
          persistLanguagePreference(settings.language);
        }
      }
    } catch (err) {
      log(`${t("读取界面语言偏好失败")}:${err}`, "warn");
    }
    try {
      const info = await invoke("app_info");
      // 版本号常驻;构建信息(提交 + 日期)是第二段,状态栏窄时让位(style.css 的容器查询),完整串留在提示里。
      // info.build 以版本号开头(「0.9.26 2009581f 2026-09-26」),与前面的 v0.9.26 重复,去掉重复的版本号再显示。
      const versionEl = $("status-version");
      const rawBuild = String(info.build ?? "");
      const buildText = rawBuild.startsWith(`${info.version} `) ? rawBuild.slice(String(info.version).length + 1) : rawBuild;
      versionEl.textContent = "";
      const versionMain = document.createElement("span");
      versionMain.textContent = `v${info.version}`;
      versionEl.append(versionMain);
      if (buildText) {
        const versionBuild = document.createElement("span");
        versionBuild.className = "ver-build";
        versionBuild.textContent = ` (${buildText})`;
        versionEl.append(versionBuild);
      }
      versionEl.title = `v${info.version} (${info.build})`;
      $("update-current").textContent = String(info.build).split(" ")[0];
      log(`kanzei ${t("桌面端启动")} · v${info.version} (${info.build})`);
    } catch (err) {
      log(`${t("获取版本失败")}:${err}`, "warn");
    }
    // 启动静默检查更新(安装版通道):有新包只弹一条 toast,不打断;失败不打扰。
    // D-265 验收④:dev 构建/本地领先这些「装不了」的成因不弹窗打扰,但结论必须
    // 提前落进设置页——否则用户不点「检查更新」就永远不知道自己收不到更新。
    setTimeout(async () => {
      try {
        const r = await invoke("update_check");
        $("update-result").textContent = updateResultText(r);
        if (r.newer && r.url) toast(`${t("发现新版本")} ${r.latest} — ${t("设置页「检查更新」可一键安装")}`);
      } catch {}
    }, 3000);
    // 启动链任一步失败都不能静默中断后半段(否则界面停在初始态,用户看不到任何原因)。
    const runStep = async ([label, step]) => {
      try {
        await step();
      } catch (err) {
        const localizedLabel = t(label);
        log(`${t("启动步骤")}「${localizedLabel}」${t("失败")}:${err}`, "err");
        toastError(`${localizedLabel}${t("加载失败")}:${err}`);
      }
    };
    // Restore the development overview without starting execution. Research owns its saved page.
    await restore_workspace_preferences();
    await runStep(["项目列表", async () => renderProjects(await invoke("projects_get"), { activate: false })]);
    sync_workspace_visibility();
    navigate_view("workspace");
    const isCurrent = workbenchNavigationGuard();
    await runStep(["项目进展", refreshWorkspace]);
    await runStep(["工作空间", () => restore_active_workspace({ isCurrent })]);
    if (isCurrent() && active_space === "dev") {
      const mode = (await uiPrefsLoad())?.ui_layout?.prefs?.conversation_mode;
      if (isCurrent() && (mode === "general" || !lastProjectPrefs.projects?.length)) await runStep(["无项目对话", openGeneralChat]);
      else if (isCurrent() && lastProjectPrefs.current) await runStep(["项目概览", () => openProjectSpace(lastProjectPrefs.current)]);
    }
    if (!currentProject) setStatus("空闲", false);
    document.body.dataset.appReady = "true";
  })();
});
