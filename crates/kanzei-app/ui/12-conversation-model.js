// One conversation projection for live registrations and retained history.
// session_id owns messages; id remains the legacy process locator used by IPC/preferences.
// A closed execution removes sending capabilities, not record management capabilities.
export function conversationRecord(project, item, { closed = false, general = false, kind, name, pinned = false } = {}) {
  const ordinal = Number(item.ordinal) || Number(/^p(\d+)\|/.exec(String(item.id))?.[1]) || 0;
  const updatedAt = Number(item.updated_at ?? item.updatedAt);
  return {
    id: item.id,
    session_id: item.session_id,
    identity: item.session_id || `${project}\u001f${item.id}`,
    project,
    scope: general ? "general" : "project",
    lifecycle: closed ? "closed" : "active",
    execution: closed ? null : item,
    item,
    kind: closed ? "conversation" : kind,
    name,
    custom: Boolean(item.title_custom),
    pinned,
    ordinal,
    updatedAt: Number.isFinite(updatedAt) && updatedAt > 0 ? updatedAt : 0,
    worktree: closed ? "" : item.worktree_path || "",
    branch: closed ? "" : item.branch || "",
    capabilities: { rename: true, delete: true, send: !closed, reorder: !closed },
  };
}

export function uniqueConversations(rows) {
  const seen = new Set();
  return rows.filter((row) => {
    if (seen.has(row.identity)) return false;
    seen.add(row.identity);
    return true;
  });
}

// Search uses displayed names and project labels, never hidden runtime paths or ids.
export function matchesConversation(row, query, projectName = "") {
  const terms = query.trim().toLocaleLowerCase().split(/\s+/).filter(Boolean);
  const text = `${row.name} ${projectName}`.toLocaleLowerCase();
  return terms.every((term) => text.includes(term));
}
