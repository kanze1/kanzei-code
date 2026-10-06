import { memoryEntriesFor } from "./memory-graph-fixture.mjs";

export function projectKnowledgeFixture(project) {
  const entries = memoryEntriesFor("project").filter(entry => entry.status === "active").slice(0, 2);
  const memories = entries.map((entry, index) => ({
    id: entry.id, title: entry.title, description: entry.description, kind: index ? "contract" : "pitfall",
    status: entry.status, source: entry.source ?? "preview", path: `.kanzei/memory/${entry.id}.md`,
    updated: entry.updated ?? "2026-10-06", revision: "preview-revision",
    links: [{ area: "kanzei-tools/edit", provenance: index ? "keyword" : "field", strength: index ? "weak" : "strong" },
      { area: "kanzei-tools/registry", provenance: "field", strength: "strong" }], refs: [], missing_areas: [],
  }));
  const makeArea = (id, parent, depth, dependencies = [], dependents = [], withMemory = false) => ({
    id, label: id.split("/").at(-1), kind: parent ? "module" : "crate", parent,
    tree_depth: depth, dependency_depth: parent ? 1 : 0, dependencies, dependents,
    memory_ids: withMemory ? memories.map(memory => memory.id) : [],
    purposes: withMemory ? [{ path: `crates/kanzei-tools/src/${id.split("/").at(-1)}.rs`, text: "修改工具的职责与项目边界", provenance: "file_summary" }] : [],
  });
  return { version: 1, enabled: true, project_root: project, code_root: project, areas: [
    makeArea("kanzei-harness", null, 0, [], ["kanzei-tools"]),
    makeArea("kanzei-tools", null, 0, ["kanzei-harness"], []),
    makeArea("kanzei-tools/edit", "kanzei-tools", 1, [], [], true),
    makeArea("kanzei-tools/registry", "kanzei-tools", 1, [], [], true),
  ], memories, unassigned: [], warnings: [] };
}
