// ---------- 启动 ----------
(async () => {
  try {
    const info = await invoke("app_info");
    $("status-version").textContent = `v${info.version} (${info.build})`;
    $("update-current").textContent = String(info.build).split(" ")[0];
    log(`kanzei 桌面端启动 · v${info.version} (${info.build})`);
  } catch (err) {
    log(`获取版本失败:${err}`, "warn");
  }
  // 启动静默检查更新(安装版通道):有新包只弹一条 toast,不打断;失败不打扰。
  // D-265 验收④:dev 构建/本地领先这些「装不了」的成因不弹窗打扰,但结论必须
  // 提前落进设置页——否则用户不点「检查更新」就永远不知道自己收不到更新。
  setTimeout(async () => {
    try {
      const r = await invoke("update_check");
      $("update-result").textContent = updateResultText(r);
      if (r.newer && r.url) toast(`发现新版本 ${r.latest} — 设置页「检查更新」可一键安装`);
    } catch {}
  }, 3000);
  // 启动链任一步失败都不能静默中断后半段(否则界面停在初始态,用户看不到任何原因)。
  for (const [label, step] of [
    ["项目列表", async () => renderProjects(await invoke("projects_get"))],
    ["历史对话", loadConversation],
    ["项目文档", refreshDocs],
    ["模型列表", loadModels],
    ["git 状态", refreshGit],
    ["排队输入", refreshPendingInputs],
  ]) {
    try {
      await step();
    } catch (err) {
      log(`启动步骤「${label}」失败:${err}`, "err");
      toastError(`${label}加载失败:${err}`);
    }
  }
  setStatus("空闲", false);
})();
