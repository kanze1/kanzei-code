// Lazy view boundary. Passive project changes reset an existing view only;
// they never create a research page or start research background work.
let loaded = null;
export async function loadResearchView() {
  if (!loaded) loaded = await import("./19-research.js");
  return loaded;
}
export async function refreshResearch(options) { return (await loadResearchView()).refreshResearch(options); }
export function reset_research_project() { loaded?.reset_research_project(); }
export function sync_research_process_context(item) { loaded?.sync_research_process_context(item); }
export async function open_research_chat() { return (await import("./19-research-navigation.js")).open_research_chat(); }
