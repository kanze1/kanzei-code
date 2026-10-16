import { invoke } from "./00-ipc.js";
import { pathKey } from "./00-project-identity.js";
import { registerLeaveGuard, leaveGuardStatus, onLeaveGuardChange } from "./00-leave-guard.js";

const drafts = new Map(), revisions = new Map(), pending = new Map(), listeners = new Set();
let request = null, timer = null;
export const composerDraftStatus = { loaded: false, loadError: "", saveError: "", saving: false,
  get ready() { return leaveGuardStatus.ready && !leaveGuardStatus.closing; },
  get error() { return leaveGuardStatus.error || this.loadError || this.saveError; }, get pending() { return pending.size > 0 || Boolean(request); } };
const leaveGuard = registerLeaveGuard({
  dirty: () => composerDraftStatus.pending,
  flush: flushComposerDrafts,
  leave: async () => { await flushComposerDrafts(); return !composerDraftStatus.pending; },
  error: error => { composerDraftStatus.saveError = String(error); for (const listener of listeners) listener(composerDraftStatus); },
});
const notify = () => { for (const listener of listeners) listener(composerDraftStatus); };
onLeaveGuardChange(notify);
export function composerDraftKey(target) {
  const main = (target.module || "main") === "main" && !target.interactionId && !target.objectId;
  return JSON.stringify([pathKey(target.project), target.processId || "", main ? "" : target.sessionId || "", target.module || "main", target.objectId || "", target.interactionId || ""]);
}
export const composerDraftEntries = () => [...drafts];
export const composerDraftGet = key => drafts.get(key);
export const onComposerDraftChange = listener => { listeners.add(listener); return () => listeners.delete(listener); };
function enqueue(key, draft) {
  revisions.set(key, (revisions.get(key) || 0) + 1);
  pending.set(key, draft);
  clearTimeout(timer); timer = setTimeout(() => void flushComposerDrafts(), 250);
  notify();
}
export function composerDraftSet(key, value) {
  if (!key) return;
  const draft = value && (value.text || value.attachments?.length || Object.keys(value.process || {}).length) ? structuredClone(value) : null;
  if (JSON.stringify(drafts.get(key) || null) === JSON.stringify(draft)) return;
  if (draft) drafts.set(key, draft); else drafts.delete(key);
  enqueue(key, draft);
}
export function composerDraftDelete(key) {
  // An acknowledged send is an explicit tombstone even if the initial load is
  // still in flight. A late disk snapshot must not resurrect submitted input.
  if (drafts.has(key)) { drafts.delete(key); enqueue(key, null); }
  else if (!composerDraftStatus.loaded && !revisions.has(key)) enqueue(key, null);
}
export async function loadComposerDrafts() {
  try {
    await leaveGuard.ready();
    const rows = await invoke("composer_drafts_load");
    for (const row of rows) if (!revisions.has(row.key)) drafts.set(row.key, row.draft);
    composerDraftStatus.loaded = true; composerDraftStatus.loadError = "";
  } catch (error) { composerDraftStatus.loadError = String(error); }
  notify();
}
export async function flushComposerDrafts() {
  clearTimeout(timer);
  if (request) return request;
  composerDraftStatus.saving = true; notify();
  request = (async () => {
    try { await leaveGuard.ready(); }
    catch (error) { composerDraftStatus.saveError = String(error); return; }
    while (pending.size) {
      const [key, draft] = pending.entries().next().value;
      const revision = revisions.get(key);
      try { await invoke("composer_draft_save", { key, draft }); }
      catch (error) { composerDraftStatus.saveError = String(error); return; }
      if (revisions.get(key) === revision) pending.delete(key);
    }
    composerDraftStatus.saveError = "";
  })().finally(() => { request = null; composerDraftStatus.saving = false; notify(); });
  return request;
}
