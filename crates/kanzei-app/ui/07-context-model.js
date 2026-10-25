// The manifest is the authority for inclusion. Search/acquisition receipts only
// describe observations; they never prove that their text entered this request.
export function contextGroup(block) {
  if (block.kind === "tool_schema" || block.label === "tools/catalog") return "tools";
  if (block.kind === "system") {
    if (/memory|preferences|project-knowledge|skills/.test(block.label)) return "knowledge";
    if (/agent\/system|verification-policy|workflow/.test(block.label)) return "rules";
    return "project";
  }
  if (block.kind === "tool_result" || block.kind === "tool_call") return "results";
  return "history";
}

export const CONTEXT_GROUPS = Object.freeze([
  ["rules", "Agent 规则与运行策略"], ["project", "项目与运行上下文"], ["knowledge", "记忆与项目知识"],
  ["history", "对话消息与纪要"], ["results", "工具与资料结果"], ["tools", "工具定义"],
]);

export function contextGroups(manifest) {
  return CONTEXT_GROUPS.map(([id,label]) => {
    const blocks = (manifest?.blocks || []).filter(block => contextGroup(block) === id);
    return { id, label, blocks, chars:blocks.reduce((sum,block)=>sum+block.chars,0) };
  }).filter(group=>group.blocks.length);
}

export function requestSources(manifest, web) {
  const rows = [];
  for (const block of manifest?.blocks || []) {
    if(block.kind==="attachment"){rows.push({key:block.sha256,label:block.label,tool:"attachment",blocks:[block],included:true,acquisitions:[]});continue;}
    if (block.kind !== "tool_result") continue;
    const source = block.source || {}, input = source.input || {}, observation = source.observation || {};
    const locator = observation.location || input.url || input.path || input.ref || "";
    if (!locator) continue;
    const key=observation.location||`${source.tool}:${source.input_sha256}:${locator}`;
    const existing=rows.find(row=>row.key===key);
    if(existing){existing.blocks.push(block);existing.failed=existing.failed&&source.is_error===true;continue;}
    rows.push({ key, label:locator,
      tool:source.tool || block.label, block, blocks:[block], observation, input, included:true, failed:source.is_error === true, acquisitions:[] });
  }
  const seen = new Set();
  for (const receipt of web || []) {
    const acquisition=receipt.acquisition;
    if (!acquisition?.id || seen.has(acquisition.id)) continue;
    seen.add(acquisition.id);
    const locator=acquisition.final_url;
    // An acquisition is independent of the request manifest. A matching URL
    // links the receipt to an already observed block; it does not add inclusion.
    const linked=rows.find(row=>row.label===locator || row.input?.url===acquisition.original_url);
    if (linked) { linked.acquisition=acquisition;linked.acquisitions.push(acquisition);continue; }
    rows.push({key:locator,label:locator,tool:"web",acquisition,acquisitions:[acquisition],blocks:[],included:false});
  }
  return rows;
}
