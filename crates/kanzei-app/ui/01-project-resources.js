import { invoke } from "./00-ipc.js";
import { createReadResource } from "./00-resource.js";
import { subscribeRefresh } from "./00-refresh.js";
import { pathKey } from "./00-project-identity.js";

const documents = new Map();
const confirmedClosures = new WeakMap();
export function projectDocuments(project) {
  const key = pathKey(project);
  if (!documents.has(key)) {
    const resource = createReadResource(() => invoke("docs_snapshot", { projectDir: project }));
    const closed = new Set();
    confirmedClosures.set(resource, closed);
    // Only a current successful read supersedes acknowledged mutations. A
    // failed or invalidated read must not revive a confirmed archived editor.
    resource.subscribe(() => { if (resource.isCurrent()) closed.clear(); });
    documents.set(key, resource);
  }
  return documents.get(key);
}
export const readProjectDocuments = (project, options) => projectDocuments(project).load(options);
export const invalidateProjectDocuments = project => projectDocuments(project).invalidate();
export function projectDocumentClosed(project, id) {
  const resource = projectDocuments(project);
  confirmedClosures.get(resource).add(id);
  resource.invalidate();
}
export const projectDocumentIsClosed = (project, id) => confirmedClosures.get(projectDocuments(project)).has(id);
export function projectDocumentsView(project, snapshot) {
  const closed = confirmedClosures.get(projectDocuments(project));
  if (!snapshot || !closed.size) return snapshot;
  // Keep the last-good backend snapshot intact, including its archive counts.
  // Views only exclude entries whose closure the backend already confirmed.
  const view = { ...snapshot };
  for (const key of ["requirements", "defects", "ideas"]) {
    if (snapshot[key]) view[key] = snapshot[key].filter(entry => !closed.has(entry.id));
  }
  return view;
}
// Views acquire a subscription while mounted and dispose it on departure.
// Calibration belongs to this project resource, so multiple views share a read.
const leases = new Map();
// A document mutation invalidates even an unmounted project's cache. Only an
// active observer requests the new version immediately; hidden views read it
// when mounted again instead of doing background work.
export function projectDocumentsChanged(project) {
  const resource = projectDocuments(project);
  resource.invalidate();
  const lease = leases.get(pathKey(project));
  return !document.hidden && lease && [...lease.owners].some(owner => owner.active())
    ? resource.load() : Promise.resolve(resource.data);
}
export function observeProjectDocuments(project, listener, active = () => true) {
  const key = pathKey(project), resource = projectDocuments(project);
  let lease = leases.get(key);
  if (!lease) {
    lease = { owners: new Set(), dispose: null };
    lease.dispose = subscribeRefresh({ interval: 20000,
      active: () => [...lease.owners].some(owner => owner.active()),
      refresh: () => resource.load({ force: true }),
      // The resource publishes its error to every mounted view.
      onError: () => {},
    });
    leases.set(key, lease);
  }
  const owner = { active }; lease.owners.add(owner);
  const dispose = resource.subscribe(listener); listener(resource);
  return () => {
    dispose(); lease.owners.delete(owner);
    if (!lease.owners.size) { lease.dispose(); leases.delete(key); }
  };
}
