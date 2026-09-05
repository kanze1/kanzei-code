import { navigate_view } from "./03-shell.js";
import { defer } from "./01-core.js";
import { $, invoke } from "./01-core.js";
import { LANGUAGE_PREFERENCES, syncLanguagePreferenceFromSettings, t } from "./02-i18n.js";
import { log, setStatus, toast, toastError } from "./03-shell.js";
import { loadModels } from "./08-models.js";
import { refreshPendingInputs, refreshProcesses, renderProjects } from "./09-sessions.js";
import { refreshDocs } from "./14-docs-actions.js";
import { loadConversation, refreshGit } from "./15-views-misc.js";
import { updateResultText } from "./16-settings.js";
import { project_workspace, restore_workspace_preferences } from "./03-workspaces.js";

// ---------- 启动 ----------
defer(() => {
  (async () => {
    // R-225:语言的持久化真源是全局配置;localStorage 只保留即时切换/旧版本迁移所需的
    // 客户端缓存。未显式设置时维持中文默认,不向配置写入默认键。
    try {
      const settings = await invoke("settings_get", { projectDir: null });
      if (LANGUAGE_PREFERENCES.has(settings.language)) syncLanguagePreferenceFromSettings(settings.language);
    } catch (err) {
      log(`${t("读取界面语言偏好失败")}:${err}`, "warn");
    }
    try {
      const info = await invoke("app_info");
      $("status-version").textContent = `v${info.version} (${info.build})`;
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
    // 项目列表必须先落地:后面每一步都要 currentProject(历史对话还要等它选出主会话),
    // 这一条是真依赖,串行。
    await restore_workspace_preferences();
    await runStep(["项目列表", async () => renderProjects(await invoke("projects_get"))]);
    // 线路列表是「历史对话」与「模型列表」的**共同前置**:前者要它选出主会话才知道
    // conversation_get 带哪个 processId,后者要按当前线路已选模型收敛紧凑列表。
    // 原实现靠"历史对话排在模型列表前面"隐式满足,并发后必须显式提出来——否则
    // loadModels 拿到空的 processItems,当前线路已选模型不进紧凑列表(冒烟实测捕获)。
    // refreshProcesses 自己按项目单飞去重,这里先拉一次不会让后面重复请求。
    await runStep([
      "线路列表",
      async () => {
        if (typeof refreshProcesses === "function") await refreshProcesses();
      },
    ]);
    // 其余五步彼此无依赖,原实现却是串行 await,于是冷启动 = 五次 IPC 往返首尾相接,
    // 其中「历史对话」要渲染整段会话(实测主会话 993 条消息/1665 个 part)、「项目文档」
    // 要解析 ~1.25MB 归档 —— 后面三步只是排在它们后面干等。改并发后总时长按最慢的一步
    // 算而不是求和。每步仍各自 try/catch(runStep 内),一步失败不影响其余;
    // 顺序无关:五步各写各的视图区,无共享中间态(refreshProcesses 自己按项目单飞去重)。
    await Promise.all(
      [
        ["历史对话", loadConversation],
        ["项目文档", refreshDocs],
        ["模型列表", loadModels],
        ["git 状态", refreshGit],
        ["排队输入", refreshPendingInputs],
      ].map(runStep),
    );
    const workspace = project_workspace();
    navigate_view(workspace[workspace.space].view);
    setStatus("空闲", false);
  })();
});
