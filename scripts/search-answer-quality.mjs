// Freeze hash-verified acquired pages, then grade real-model citations independently.
import assert from "node:assert/strict";
import { createHash } from "node:crypto";
import { readFile, mkdir, writeFile } from "node:fs/promises";
import path from "node:path";
import { dataset, answerScore } from "./search-quality.mjs";

const [mode, input, output] = process.argv.slice(2);
assert(input && output && ["freeze", "grade"].includes(mode), "freeze <live-results.json> <new-directory>, or grade <benchmark-directory> <report.json>");
if (mode === "freeze") {
  const observation = JSON.parse(await readFile(input, "utf8"));
  assert.equal(observation.dataset_revision, dataset.schema);
  await mkdir(output, { recursive: false });
  const terms = {
    codex: ["Sandbox mode:", "Approval policy:"],
    "claude-code": ["Filesystem isolation,", "Network isolation,"],
    codebuddy: ["CodeBuddy Code uses strict read-only permissions"],
    "kimi-code": ["Always Ask mode (formerly Manual)", "Ask When Needed mode (formerly YOLO)", "Never Ask mode (formerly Auto)"],
  };
  const evidence = [];
  for (const body of observation.bodies) {
    assert(!body.is_error, `Cannot annotate failed acquisition: ${body.id}`);
    const saved = body.content.match(/Saved page: `([^`]+)`/);
    const declared = body.content.match(/SHA-256: ([a-f0-9]{64})/);
    assert(saved && declared, "An answer source must carry its actual acquisition and saved bytes");
    const bytes = await readFile(saved[1]);
    assert.equal(createHash("sha256").update(bytes).digest("hex"), declared[1], "A frozen source must match the actual web acquisition hash");
    const lines = bytes.toString("utf8").split(/\r?\n/), paragraphs = [];
    for (const term of terms[body.id]) {
      const index = lines.findIndex(line => line.includes(term));
      assert(index >= 0, `The acquired ${body.id} page must actually contain: ${term}`);
      let start = index, end = index + 1;
      while (start > 0 && lines[start - 1].trim()) start--;
      while (end < lines.length && lines[end].trim()) end++;
      paragraphs.push({ from_line: start + 1, to_line: end, text: lines.slice(start, end).join("\n") });
    }
    evidence.push({ id: body.id, url: body.url, content: [...new Set(paragraphs.map(paragraph => paragraph.text))].join("\n\n"),
      acquisition_sha256: declared[1], source: body.source, excerpts: paragraphs });
  }
  const requested = dataset.queries.map(query => ({ target: query.id,
    tag: dataset.documents.find(document => document.target === query.id && document.level === "primary").supports[0] }));
  const cases = [
    { id: "four-verified-sources", requested, evidence },
    { id: "missing-codebuddy", requested, evidence: evidence.filter(document => document.id !== "codebuddy") },
  ];
  await writeFile(path.join(output, "frozen-input.json"), JSON.stringify({ scope: "Four documented security properties with exact source quotation; not CVE completeness or a general agent benchmark", dataset_revision: dataset.schema, observed_at: observation.observed_at, cases }, null, 2));
  console.log(`Frozen ${evidence.length} actual hash-verified sources and ${cases.length} answer cases`);
} else {
  const frozen = JSON.parse(await readFile(path.join(input, "frozen-input.json"), "utf8"));
  const observed = JSON.parse(await readFile(path.join(input, "model-results.json"), "utf8"));
  const cases = frozen.cases.map(test => {
    const record = observed.records.find(record => record.id === test.id);
    const answer = JSON.parse(record.answer);
    assert(Array.isArray(answer.claims));
    const available = new Set(test.evidence.map(document => document.id));
    const supported = answer.claims.filter(claim => claim.status === "supported");
    const expected = test.requested.filter(request => available.has(request.target));
    const scored = answerScore(supported, test.evidence);
    const missing = test.requested.filter(request => !available.has(request.target));
    const correctGaps = missing.every(request => answer.claims.filter(claim => claim.target === request.target && claim.status === "insufficient_evidence" && !claim.url && !claim.quote).length === 1);
    const pass = answer.claims.length === test.requested.length && supported.length === expected.length
      && expected.every(request => scored.claims.filter(claim => claim.target === request.target && claim.tag === request.tag && claim.supported).length === 1)
      && scored.claims.every(claim => claim.supported) && correctGaps;
    return { id: test.id, pass, expected_supported: expected.length, observed_supported: supported.length,
      missing_targets: missing.map(request => request.target), correctly_disclosed_gaps: correctGaps, citations: scored, model: record.model, usage: record.usage };
  });
  const result = { scope: frozen.scope, cases, pass: cases.every(test => test.pass),
    limitations: "Two frozen evidence cases, one real Luna request per case, no retry or exclusion; live candidate retrieval and document acquisition are separately recorded. Manual semantic review remains part of the report." };
  await writeFile(output, JSON.stringify(result, null, 2));
  console.log(JSON.stringify(result));
  assert(result.pass, "Preserve failures; source existence, exact quotation and explicit gaps are all required");
}
