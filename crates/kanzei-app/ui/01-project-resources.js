import { invoke } from "./00-ipc.js";
import { createReadResource } from "./00-resource.js";
import { subscribeRefresh } from "./00-refresh.js";
import { pathKey } from "./00-project-identity.js";

const documents = new Map();
export function projectDocuments(project) {
  const key = pathKey(project);
  if (!documents.has(key)) documents.set(key, createReadResource(() => invoke("docs_snapshot", { projectDir: project })));
  return documents.get(key);
}
export const readProjectDocuments = (project, options) => projectDocuments(project).load(options);
export const invalidateProjectDocuments = project => projectDocuments(project).invalidate();
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
