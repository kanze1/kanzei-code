// Domain panels share one disclosure state; this module has no UI dependencies.
let active = false, platform = null;
export function socialActive() { return active; }
export function setSocialActive(value) { active = value; }
export function socialManagement(platformName) { return active && platform === platformName && document.body.dataset.view === "chat"; }
export function setSocialManagement(value) { if (platform === value) return; platform = value; document.dispatchEvent(new CustomEvent("kz:social-management")); }
export function closeSocialManagement() { document.dispatchEvent(new CustomEvent("kz:social-manage-close")); }
export function socialUpdated() { document.dispatchEvent(new CustomEvent("kz:social-updated")); }
