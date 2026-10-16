// One calibration clock. Events still invalidate resources immediately; polling
// only reconciles data for subscribed, visible surfaces after missed events.
const subscriptions = new Set();
let clock = null;
export function subscribeRefresh({ interval, active, refresh, onError }) {
  const subscription = { interval, active, refresh, onError, due: Date.now() + interval, request: null };
  subscriptions.add(subscription);
  if (!clock) clock = setInterval(() => {
    if (document.hidden) return;
    const now = Date.now();
    for (const entry of subscriptions) {
      if (!entry.active() || entry.request || now < entry.due) continue;
      entry.due = now + entry.interval;
      entry.request = Promise.resolve().then(entry.refresh).catch(entry.onError).finally(() => { entry.request = null; });
    }
  }, 1000);
  return () => { subscriptions.delete(subscription); if (!subscriptions.size) { clearInterval(clock); clock = null; } };
}
