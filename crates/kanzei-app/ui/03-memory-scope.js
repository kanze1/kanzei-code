// Browsing memories must not change the recipient of a conversation or task.
export let memoryProject = null;
export function setMemoryProject(project) { memoryProject = project || null; }
