import { invoke } from "./00-ipc.js";
import { pathKey } from "./00-project-identity.js";

export const HISTORY_PAGE_SIZE = 50;
const resources = new Map(), listeners = new Set();
export function onConversationHistoryChange(listener) { listeners.add(listener); return () => listeners.delete(listener); }
const notify = resource => { for (const listener of listeners) listener(resource); };
export function conversationHistory(projectDir, processId) {
  const key = JSON.stringify([pathKey(projectDir), processId]);
  if (!resources.has(key)) resources.set(key, { projectDir, processId, items: [], nextBeforeSequence: null, hasMore: false, loaded: false, error: "", request: null });
  return resources.get(key);
}
export function invalidateConversationHistory(projectDir, processId) {
  const resource = conversationHistory(projectDir, processId);
  resource.request = null; resource.loaded = false; resource.items = []; resource.nextBeforeSequence = null; resource.hasMore = false; resource.error = "";
  notify(resource);
}
export async function readConversationHistory(projectDir, processId, { older = false } = {}) {
  const resource = conversationHistory(projectDir, processId);
  if (resource.request) return resource.request.promise;
  if (older && resource.loaded && !resource.hasMore) return resource;
  const request = { promise: null }; resource.request = request; resource.error = ""; notify(resource);
  const beforeSequence = older ? resource.nextBeforeSequence : null;
  request.promise = Promise.resolve().then(async () => {
    try {
      const page = await invoke("conversation_page", { projectDir, processId, beforeSequence, limit: HISTORY_PAGE_SIZE });
      if (resource.request !== request) return resource;
      const hadOlder = resource.loaded && resource.items.some(item => item.sequence < (page.items[0]?.sequence ?? Infinity));
      const items = new Map((!older && !page.hasMore ? [] : resource.items).map(item => [item.sequence, item]));
      for (const item of page.items) items.set(item.sequence, item);
      resource.items = [...items.values()].sort((a, b) => a.sequence - b.sequence);
      if (older || !hadOlder || !page.hasMore) { resource.nextBeforeSequence = page.nextBeforeSequence; resource.hasMore = page.hasMore; }
      resource.loaded = true;
    } catch (error) { if (resource.request === request) resource.error = String(error); }
    finally { if (resource.request === request) { resource.request = null; notify(resource); } }
    return resource;
  });
  return request.promise;
}
