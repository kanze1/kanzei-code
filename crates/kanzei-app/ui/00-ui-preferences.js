import { invoke } from "./00-ipc.js";
// D-404:关键 UI 偏好后端持久化通道。WebView2 localStorage 在本机数据文件缺失
// (EBWebView\Default\Local Storage\leveldb 无 .ldb/.log,重启即丢),theme/鞭挞
// 等偏好经 ui_prefs_get/ui_prefs_set 存 ~/.kanzei/app.json;localStorage 仅作
// 旧值兼容(读时先本地后后端,写时双写)。uiPrefsSave 失败静默——偏好丢失可接受,
// 不该打断当前操作。
export let uiPrefsCache = null;
let uiPrefsQueue = Promise.resolve();
function queueUiPrefs(operation) {
  const result = uiPrefsQueue.then(operation, operation);
  uiPrefsQueue = result.catch(() => {});
  return result;
}
function prefsObject(value) {
  return value !== null && typeof value === "object" && !Array.isArray(value);
}
// 工作区只写触及的项目/分区/课题字段；null 是选中任务的显式清空值。
// 与 prefs.rs 的 workspace_state 合并边界一致，不用于 ui_layout 的 null 删除。
export function mergeWorkspaceState(state, patch) {
  const merged = { ...(prefsObject(state) ? state : {}) };
  for (const [project, update] of Object.entries(patch)) {
    if (!prefsObject(update)) { merged[project] = update; continue; }
    const next = { ...(prefsObject(merged[project]) ? merged[project] : {}) };
    for (const [section, value] of Object.entries(update)) {
      if (["dev", "research"].includes(section) && prefsObject(value)) {
        next[section] = { ...(prefsObject(next[section]) ? next[section] : {}), ...value };
      } else if (section === "topic_states" && prefsObject(value)) {
        const topics = { ...(prefsObject(next.topic_states) ? next.topic_states : {}) };
        for (const [topic, fields] of Object.entries(value)) {
          topics[topic] = prefsObject(fields) ? { ...(prefsObject(topics[topic]) ? topics[topic] : {}), ...fields } : fields;
        }
        next.topic_states = topics;
      } else next[section] = value;
    }
    merged[project] = next;
  }
  return merged;
}
function mergeUiPrefsPatch(prefs, patch) {
  const merged = { ...prefs };
  for (const key of ["theme", "backdrop", "auto_max", "continue_prompt"]) {
    if (patch[key] !== undefined && patch[key] !== null) merged[key] = patch[key];
  }
  if (prefsObject(patch.work_priority)) merged.work_priority = { ...(prefs.work_priority || {}), ...patch.work_priority };
  if (prefsObject(patch.process_auto_state)) {
    const states = { ...(prefs.process_auto_state || {}) };
    for (const [id, fields] of Object.entries(patch.process_auto_state)) {
      if (fields === null) delete states[id];
      else states[id] = prefsObject(fields) ? { ...(prefsObject(states[id]) ? states[id] : {}), ...fields } : fields;
    }
    merged.process_auto_state = states;
  }
  if (prefsObject(patch.workspace_state)) merged.workspace_state = mergeWorkspaceState(prefs.workspace_state, patch.workspace_state);
  if (patch.memory_view === "list" || patch.memory_view === "graph") merged.memory_view = patch.memory_view;
  // prefs.rs 按分区/键合并;几何等键值整体替换,两个层次的 null 都表示删除。
  if (prefsObject(patch.ui_layout) && new TextEncoder().encode(JSON.stringify(patch.ui_layout)).length <= 64 * 1024) {
    const layout = { ...(prefsObject(prefs.ui_layout) ? prefs.ui_layout : {}) };
    for (const [section, value] of Object.entries(patch.ui_layout)) {
      if (value === null) delete layout[section];
      else if (prefsObject(value)) {
        const keys = { ...(prefsObject(layout[section]) ? layout[section] : {}) };
        for (const [key, entry] of Object.entries(value)) {
          if (entry === null) delete keys[key];
          else keys[key] = entry;
        }
        layout[section] = keys;
      } else layout[section] = value;
    }
    merged.ui_layout = layout;
  }
  return merged;
}
export async function uiPrefsLoad(refresh = false) {
  return queueUiPrefs(async () => {
    if (refresh || !uiPrefsCache) {
      try {
        uiPrefsCache = (await invoke("ui_prefs_get")) || {};
      } catch {
        return uiPrefsCache || {};
      }
    }
    return uiPrefsCache;
  });
}
export async function uiPrefsSave(patch) {
  try {
    // 排队前冻结请求,避免调用方随后修改对象改变原本要保存的偏好。
    const snapshot = JSON.parse(JSON.stringify(patch));
    await queueUiPrefs(async () => {
      await invoke("ui_prefs_set", snapshot);
      if (uiPrefsCache) uiPrefsCache = mergeUiPrefsPatch(uiPrefsCache, snapshot);
    });
  } catch {
    /* 写入失败静默:下次启动回退 localStorage 旧值 */
  }
}
