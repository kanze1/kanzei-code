// Pure projections shared by the production workspace and its regression checks.
export function batchCells(entry) {
  const units = entry.work_units || [];
  if (units.length) return units.map(unit => ({ id: unit.unit_id, state: unit.status }));
  const total = Math.max(0, Number(entry.batches?.total) || 0);
  const done = Math.min(total, Math.max(0, Number(entry.batches?.done) || 0));
  return Array.from({ length: Math.min(total, 100) }, (_, index) => ({ id: String(index + 1), state: index < done ? "done" : index === done && ["doing", "fixing"].includes(entry.status) ? "active" : "pending" }));
}

export function dependencyLayers(members, edges) {
  const names = new Set(members.map(member => member.name));
  const links = edges.filter(edge => names.has(edge.from) && names.has(edge.to) && edge.from !== edge.to && !edge.transitive && (!edge.kind || edge.kind === "normal"));
  const remaining = new Set(names), layers = [];
  while (remaining.size) {
    const next = [...remaining].filter(name => !links.some(edge => edge.to === name && remaining.has(edge.from)));
    // Cycles stay visible in one unresolved layer; do not invent a parent.
    if (!next.length) { layers.push([...remaining]); break; }
    layers.push(next); next.forEach(name => remaining.delete(name));
  }
  return { layers, edges: links, cyclic: [...remaining] };
}

export function runtimeSummary(rounds, recalls, { since = 0, session = "" } = {}) {
  const rows = rounds.filter(row => Number(row.at) >= since && (!session || row.sessionId === session));
  const observations = recalls.filter(row => Number(row.at) >= since && (!session || row.session_id === session));
  const measured = rows.filter(row => row.measured);
  const durations = rows.filter(row => Number.isFinite(row.durationMs) && row.durationMs > 0);
  const sum = key => measured.reduce((total, row) => total + (Number(row.metrics?.[key]) || 0), 0);
  const reads = observations.filter(row => Array.isArray(row.read_ids));
  const tokens = rows.filter(row => Number.isFinite(row.inputTokens) && Number.isFinite(row.outputTokens));
  const steps = rows.filter(row => Number.isFinite(row.steps));
  const durationsSorted = durations.map(row => row.durationMs).sort((a, b) => a - b);
  const outcomes = new Map();
  for (const row of rows) outcomes.set(row.outcome || "unknown", (outcomes.get(row.outcome || "unknown") || 0) + 1);
  const toolNames = new Map();
  for (const row of measured) for (const [name, value] of Object.entries(row.tools || {})) {
    const count = typeof value === "number" ? value : Number(value?.calls);
    if (Number.isFinite(count)) toolNames.set(name, (toolNames.get(name) || 0) + count);
  }
  return {
    rounds: rows.length, measured: measured.length,
    rows: [...rows].sort((a, b) => b.at - a.at), outcomes: [...outcomes], toolNames: [...toolNames].sort((a, b) => b[1] - a[1]),
    inputTokens: tokens.length ? tokens.reduce((sum, row) => sum + row.inputTokens, 0) : null,
    outputTokens: tokens.length ? tokens.reduce((sum, row) => sum + row.outputTokens, 0) : null,
    tokenSamples: tokens.length,
    steps: steps.length ? steps.reduce((sum, row) => sum + row.steps, 0) : null,
    totalDuration: durations.length ? durations.reduce((sum, row) => sum + row.durationMs, 0) : null,
    p95Duration: durations.length ? durationsSorted[Math.max(0, Math.ceil(durations.length * .95) - 1)] : null,
    meanDuration: durations.length ? durations.reduce((total, row) => total + row.durationMs, 0) / durations.length : null,
    durationSamples: durations.length,
    tools: measured.length ? sum("total_calls") : null,
    failures: measured.length ? sum("failed_calls") : null,
    rejected: measured.length ? sum("tool_rejections") : null,
    recalls: observations.length ? observations.reduce((total, row) => total + (row.retrieved_ids?.length || 0), 0) : null,
    injected: observations.length ? observations.reduce((total, row) => total + (row.injected_ids?.length || 0), 0) : null,
    read: reads.length ? reads.reduce((total, row) => total + row.read_ids.length, 0) : null,
    memorySamples: observations.length, readSamples: reads.length,
  };
}

export function workChoices(entries, lines, processId) {
  return entries.filter(entry => !entry.closed && !["draft", "awaiting_external", "parked", "done", "dropped", "fixed", "wontfix"].includes(entry.status)).map(entry => {
    const explicitOwners = (entry.owner_lines || []).map(owner => typeof owner === "string" ? owner : owner.id);
    const currentLine = lines.find(line => line.id === processId);
    const receiptOwner = lines.find(line => line.id !== processId && line.current_item_id === entry.id);
    const declaredOwner = !explicitOwners.includes(processId) && explicitOwners.find(id => id !== processId);
    // Peer conversations can share a Git branch. A branch label alone must
    // not make every peer look like an independent owner of the same claim.
    const branchOwner = entry.claimed_by && entry.claimed_by !== currentLine?.branch
      ? lines.find(line => line.branch === entry.claimed_by) || { label: entry.claimed_by } : null;
    const owner = receiptOwner || (declaredOwner && (lines.find(line => line.id === declaredOwner) || { id: declaredOwner })) || branchOwner;
    return { ...entry, owner, selectable: !owner && !entry.blocked };
  });
}
