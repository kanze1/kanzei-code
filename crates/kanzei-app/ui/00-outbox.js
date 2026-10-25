// Pending/failed sends are client-owned attempts. Only an IPC acknowledgement
// promotes a message to accepted. A retry reuses its record and frozen payload.
export function createOutbox(deliver) {
  const unresolved = new Map();
  const notify = record => { for (const listener of record.listeners) listener(record); };
  return {
    pending(sessionId) { return [...unresolved.values()].filter(record => record.sessionId === sessionId); },
    prepare(sessionId, payload) {
      const key = JSON.stringify([sessionId, payload]);
      if (unresolved.has(key)) return unresolved.get(key);
      const record = { key, sessionId, payload: { ...structuredClone(payload), commandId: crypto.randomUUID() }, status: "pending", error: "", request: null, listeners: new Set() };
      unresolved.set(key, record); return record;
    },
    subscribe(record, listener) { record.listeners.add(listener); listener(record); return () => record.listeners.delete(listener); },
    send(record) {
      if (record.request) return record.request;
      if (record.status === "accepted") return Promise.resolve(true);
      record.status = "pending"; record.error = ""; notify(record);
      record.request = Promise.resolve().then(async () => {
        try {
          await deliver(record.payload);
          record.status = "accepted"; unresolved.delete(record.key); return true;
        } catch (error) { record.status = "failed"; record.error = String(error); return false; }
        finally { record.request = null; notify(record); }
      });
      return record.request;
    },
  };
}
