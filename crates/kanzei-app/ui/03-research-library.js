import { invoke } from "./01-core.js";
import { active_space, project_workspace, save_research_workspace } from "./03-workspaces.js";
import { activate_execution_root, refreshProcesses } from "./09-sessions.js";
import { refreshResearch, reset_research_project } from "./19-research.js";

export let research_library = null;
let library_request = null;
let library_selection = 0;

export function selected_library_entry() {
  return research_library?.entries.find((entry) => entry.id === project_workspace().research.topic_id);
}

export async function load_research_library(force = false) {
  if (research_library && !force) return research_library;
  if (library_request) return library_request;
  library_request = invoke("research_library_list").then((value) => {
    if (!Array.isArray(value?.entries)) throw new Error("研究课题库读取失败");
    research_library = value;
    return value;
  }).finally(() => { library_request = null; });
  return library_request;
}

export async function select_library_entry(id) {
  const sequence = ++library_selection;
  const entry = research_library?.entries.find((entry) => entry.id === id);
  if (active_space !== "research") return;
  save_research_workspace({ topic_id: entry?.id || "", topic: entry?.topic || "", storage_root: entry?.storage_root || "", category: entry?.kind || project_workspace().research.category });
  activate_execution_root(entry?.available === false ? null : entry?.storage_root || null);
  reset_research_project();
  await refreshProcesses();
  if (sequence !== library_selection || active_space !== "research") return;
  await refreshResearch();
}

export async function enter_research_library(development_root) {
  const library = await load_research_library(true);
  if (active_space !== "research") return;
  const saved = project_workspace().research;
  const entry = saved.topic_id
    ? library.entries.find((entry) => entry.id === saved.topic_id)
    : library.entries.find((entry) => entry.topic === saved.topic && entry.storage_root === development_root && entry.kind === saved.category)
      ?? library.entries.find((entry) => entry.kind === saved.category);
  await select_library_entry(entry?.id || "");
}
