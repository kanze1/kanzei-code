import { listen } from "./00-ipc.js";

const subscriptions = new Set(), listeners = new Set();
export const eventSubscriptionState = {
  get failed() { return [...subscriptions].filter(item => item.status === "failed"); },
  get pending() { return [...subscriptions].filter(item => item.status === "pending"); },
  get ready() { return this.failed.length === 0 && this.pending.length === 0; },
};
const notify = () => { for (const listener of listeners) listener(eventSubscriptionState); };
export function onEventSubscriptionChange(listener) { listeners.add(listener); return () => listeners.delete(listener); }
async function connect(item) {
  if (item.request) return item.request;
  if (item.status === "ready") return true;
  item.status = "pending"; item.error = ""; notify();
  item.request = Promise.resolve().then(async () => {
    try { item.unlisten = await listen(item.event, item.handler); item.status = "ready"; return true; }
    catch (error) { item.status = "failed"; item.error = String(error); return false; }
    finally { item.request = null; notify(); }
  });
  return item.request;
}
export function subscribeRuntimeEvent(event, handler) {
  const item = { event, handler, status: "pending", error: "", request: null, unlisten: null };
  subscriptions.add(item); void connect(item);
  return item;
}
export async function restoreEventSubscriptions() {
  await Promise.all(eventSubscriptionState.failed.map(connect));
  return eventSubscriptionState.ready;
}
export async function runtimeEventsReady() {
  await Promise.all([...subscriptions].map(item => item.request));
  return eventSubscriptionState.ready;
}
