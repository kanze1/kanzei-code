// A window has one leave owner. Each editor supplies its own persistence and
// decision contract; no editor may close the window while another is saving.
const guards = new Set();
const listeners = new Set();
let registration = null, unlisten = null, closing = null;
export const leaveGuardStatus = { ready: false, error: "", get closing() { return Boolean(closing); } };
export function onLeaveGuardChange(listener) { listeners.add(listener); return () => listeners.delete(listener); }
const notify = () => { for (const listener of listeners) listener(leaveGuardStatus); };
const dirty = () => [...guards].filter(guard => guard.dirty());
async function requestClose(event) {
  if (closing) { event.preventDefault(); return; }
  closing = Promise.resolve().then(async () => {
    for (const guard of guards) {
      if (!guard.dirty()) continue;
      try { if (!await guard.leave()) { event.preventDefault(); return; } }
      catch (error) { guard.error(error); event.preventDefault(); return; }
    }
  });
  // Freeze editors synchronously, before any owner's asynchronous decision.
  // A later owner cannot acquire fresh user input while another is saving.
  notify();
  try { await closing; } finally { closing = null; notify(); }
}
async function sync() {
  const win = window.__TAURI__?.window?.getCurrentWindow?.();
  if (typeof win?.onCloseRequested !== "function") {
    // A browser preview has beforeunload, but no native window lifetime.
    leaveGuardStatus.ready = true; notify(); return true;
  }
  if (unlisten) return true;
  if (registration) return registration;
  leaveGuardStatus.ready = false; leaveGuardStatus.error = ""; notify();
  registration = (async () => {
    try { unlisten = await win.onCloseRequested(requestClose); leaveGuardStatus.ready = true; return true; }
    catch (error) { leaveGuardStatus.error = String(error); return false; }
    finally { registration = null; notify(); }
  })();
  return registration;
}
export function registerLeaveGuard(guard) {
  guards.add(guard);
  // Register before any editor becomes usable, and retain the listener across
  // clean/dirty transitions. Subscribing only after input leaves an IPC race.
  void sync();
  return { sync, async ready() { if (!await sync()) throw new Error(leaveGuardStatus.error); }, dispose() { guards.delete(guard); } };
}
export function guardBeforeUnload(event) {
  const unsaved = dirty();
  if (!unsaved.length) return;
  for (const guard of unsaved) void guard.flush();
  event.preventDefault(); event.returnValue = "";
}
window.addEventListener("beforeunload", guardBeforeUnload);
