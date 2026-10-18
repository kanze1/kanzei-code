// A read resource owns cached data, invalidation generations and one request.
// Invalidations during a read collapse into one fresh read before callers settle.
export function createReadResource(read) {
  const listeners = new Set();
  let revision = 0, resolvedRevision = -1;
  const resource = { data: null, error: "", status: "idle", request: null };
  const notify = () => { for (const listener of listeners) listener(resource); };
  resource.subscribe = listener => { listeners.add(listener); return () => listeners.delete(listener); };
  resource.isCurrent = () => resource.status === "ready" && resolvedRevision === revision;
  resource.invalidate = () => { revision += 1; if (!resource.request) resource.status = "stale"; notify(); };
  resource.load = ({ force = false } = {}) => {
    if (force) resource.invalidate();
    if (resource.request) return resource.request;
    if (resource.isCurrent()) return Promise.resolve(resource.data);
    resource.status = "loading"; resource.error = "";
    resource.request = Promise.resolve().then(async () => {
      do {
        const requestedRevision = revision;
        const data = await read();
        if (requestedRevision !== revision) continue;
        resource.data = data; resolvedRevision = requestedRevision;
      } while (resolvedRevision !== revision);
      resource.status = "ready"; return resource.data;
    }).catch(error => {
      resource.error = String(error); resource.status = "error"; throw error;
    }).finally(() => { resource.request = null; notify(); });
    notify();
    return resource.request;
  };
  return resource;
}
