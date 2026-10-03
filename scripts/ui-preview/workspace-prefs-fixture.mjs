// Native fixture counterpart of prefs.rs's workspace_state domain merge.
// Only touched project/section/topic fields are writes; null remains a value.
export function mergeWorkspacePrefs(current, patch) {
  const object = value => value !== null && typeof value === "object" && !Array.isArray(value);
  const result = structuredClone(object(current) ? current : {});
  for (const [project, update] of Object.entries(patch)) {
    if (!object(update)) { result[project] = structuredClone(update); continue; }
    if (!object(result[project])) result[project] = {};
    for (const [section, value] of Object.entries(update)) {
      if (["dev", "research"].includes(section) && object(value)) {
        result[project][section] = { ...(object(result[project][section]) ? result[project][section] : {}), ...structuredClone(value) };
      } else if (section === "topic_states" && object(value)) {
        if (!object(result[project].topic_states)) result[project].topic_states = {};
        for (const [topic, fields] of Object.entries(value)) {
          result[project].topic_states[topic] = object(fields)
            ? { ...(object(result[project].topic_states[topic]) ? result[project].topic_states[topic] : {}), ...structuredClone(fields) }
            : structuredClone(fields);
        }
      } else result[project][section] = structuredClone(value);
    }
  }
  return result;
}
