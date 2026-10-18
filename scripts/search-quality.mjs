// Frozen query/coverage/citation rubric; live evidence is a separate input.
import assert from "node:assert/strict";
import { readFile, mkdir, writeFile } from "node:fs/promises";
import path from "node:path";
import { fileURLToPath } from "node:url";

export const dataset = JSON.parse(await readFile(new URL("./fixtures/search-quality.json",import.meta.url),"utf8"));
export const canonicalSourceKey = url => { try { const parsed = new URL(url); return `${parsed.hostname.toLowerCase().replace(/^www\./,"")}${parsed.pathname.replace(/\/$/,"")}`; } catch { return null; } };
const key = canonicalSourceKey;
export function retrievalScore(groups, k = 5) {
  const targets = dataset.queries.map(query => {
    const observed = groups.find(group => group.id === query.id);
    const results = observed?.results?.slice(0,k) || [];
    const expected = new Set(query.expected_urls.map(key));
    const relevant = results.filter(result => expected.has(key(result.url)));
    return { id:query.id, requested:Boolean(observed), status:observed?.status || "not_requested", candidates:results.length,known_url_matches:relevant.length,known_url_match_fraction:results.length ? relevant.length/results.length : 0,covered:relevant.length > 0,verified:false };
  });
  return {targets, requested:targets.filter(target=>target.requested).length,covered:targets.filter(target=>target.covered).length,total:targets.length,coverage:targets.filter(target=>target.covered).length/targets.length,uncovered:targets.filter(target=>!target.covered).map(target=>target.id)};
}
const normalize = value => String(value || "").replace(/\s+/g, " ").trim();
export function answerScore(claims, evidence) {
  const scored = claims.map(claim => {
    const card = dataset.documents.find(document => document.target === claim.target && key(document.url) === key(claim.url));
    const source = evidence.find(document => document.id === claim.target && key(document.url) === key(claim.url) && !document.is_error);
    const quote = normalize(claim.quote);
    const quoted = Boolean(quote && source && normalize(source.content).includes(quote));
    return { target: claim.target, tag: claim.tag, url: claim.url, quoted,
      supported: Boolean(quoted && card?.level === "primary" && card.supports.includes(claim.tag)) };
  });
  const targets = dataset.queries.map(query => ({ target: query.id,
    supported: scored.filter(claim => claim.target === query.id && claim.supported).length === 1 }));
  return { claims: scored, targets, covered: targets.filter(target => target.supported).length, total: targets.length,
    citation_precision: scored.length ? scored.filter(claim => claim.supported).length / scored.length : null,
    pass: scored.length === targets.length && targets.every(target => target.supported) && scored.every(claim => claim.supported) };
}
export function citationScore(claims) {
  const rows = claims.map(claim => {
    const source = dataset.documents.find(document=>key(document.url) === key(claim.url));
    return {tag:claim.tag,url:claim.url,source_level:source?.level || "unknown",supported:source?.level === "primary" && source.supports.includes(claim.tag)};
  });
  return {claims:rows,supported:rows.filter(row=>row.supported).length,total:rows.length,precision:rows.length ? rows.filter(row=>row.supported).length/rows.length : null};
}
if (process.argv[1] && path.resolve(process.argv[1]) === fileURLToPath(import.meta.url)) {
  const groups = dataset.queries.map(query=>({id:query.id,status:"ok",results:[{url:query.expected_urls[0]}]}));
  assert.equal(retrievalScore(groups).coverage,1);
  assert.deepEqual(retrievalScore(groups.filter(group=>group.id!=="codebuddy")).uncovered,["codebuddy"]);
  assert.equal(retrievalScore([{id:"codebuddy",status:"ok",results:[{url:"https://unrelated.example/CodeBuddy/security"}]}]).covered,0);
  const claims = dataset.documents.filter(document=>document.level==="primary").map(document=>({tag:document.supports[0],url:document.url}));
  assert.equal(citationScore(claims).precision,1);
  assert.equal(citationScore([{tag:claims[0].tag,url:claims[1].url}]).precision,0);
  assert.equal(citationScore([{tag:claims[2].tag,url:"https://unrelated.example/CodeBuddy/security"}]).precision,0);
  assert.equal(retrievalScore(dataset.queries.map(query=>({id:query.id,status:"challenge",results:[]}))).coverage,0);
  const evidence = claims.map((claim, index) => ({ id: dataset.queries[index].id, url: claim.url, content: "Frozen short evidence." }));
  const quoted = claims.map((claim, index) => ({ ...claim, target: dataset.queries[index].id, quote: "Frozen short evidence." }));
  assert.equal(answerScore(quoted, evidence).pass, true);
  assert.equal(answerScore(quoted.filter(claim => claim.target !== "codebuddy"), evidence).pass, false);
  assert.equal(answerScore(quoted.map(claim => ({ ...claim, quote: "Invented quotation." })), evidence).pass, false);
  assert.equal(answerScore(quoted.map(claim => ({ ...claim, url: claims[0].url })), evidence).pass, false);
  assert.equal(answerScore([...quoted, quoted[0]], evidence).pass, false);
  const output=path.resolve("output/remediation-20261018/search-quality"); await mkdir(output,{recursive:true});
  await writeFile(path.join(output,"frozen-rubric.json"),JSON.stringify({dataset,checks:12,retrieval:retrievalScore(groups),citations:citationScore(claims),limits:"Frozen annotations validate the evaluator. They do not measure the live provider or establish general search quality."},null,2));
  console.log("Search quality rubric PASS: 12 checks; missing objects, invented quotations and false citations are detected");
}
