// Decode the engine-owned source frame without discarding provider error markers.
import assert from "node:assert/strict";
const marker = "[external_source_data]\n";
export function decodeExternalSource(value) {
  const index = value.indexOf(marker);
  if (index < 0) return { content: value, source: null };
  const prefix = value.slice(0, index);
  assert(/^(?:\[tool error\]\n|\[tool_outcome=[^\n]+\]\n)*$/.test(prefix), "Source frame may only follow an engine error marker");
  const frame = JSON.parse(value.slice(index + marker.length));
  assert.equal(frame.authority, "untrusted_data");
  assert.equal(typeof frame.content, "string");
  assert.equal(typeof frame.source.location, "string");
  assert(["web", "browser", "file", "process"].includes(frame.source.kind));
  return { content: prefix + frame.content, source: frame.source };
}
