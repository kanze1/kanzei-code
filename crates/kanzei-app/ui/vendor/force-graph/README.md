# force-graph runtime

- Version: 1.51.4, UMD browser bundle `dist/force-graph.min.js`, unmodified (177,599 bytes).
- Source: https://cdn.jsdelivr.net/npm/force-graph@1.51.4/dist/force-graph.min.js
  (upstream https://github.com/vasturiano/force-graph).
- License: MIT (Copyright (c) 2018 Vasco Asturiano); unmodified upstream LICENSE is included beside the runtime.
- SHA-256: `1008539bb9e171a0dc343453366451a1b3a6ded06028ef4f978608b658ba2d0a`
  (checked by `scripts/ui-memory-graph-smoke.mjs`; a mismatch fails the smoke).
- Bundled dependencies (inlined by upstream, licenses carried by the bundle):
  d3-array, d3-color, d3-dispatch, d3-drag, d3-ease, d3-format, d3-interpolate, d3-quadtree,
  d3-scale, d3-scale-chromatic, d3-selection, d3-time, d3-time-format, d3-timer, d3-transition,
  d3-zoom (ISC, Mike Bostock); d3-force-3d, d3-binarytree, d3-octree, kapsule, accessor-fn,
  index-array-by, float-tooltip, canvas-color-tracker (MIT, Vasco Asturiano); lodash-es (MIT);
  @tweenjs/tween.js (MIT); bezier-js (MIT); tinycolor2 (MIT).
- Loaded locally and lazily by `24-graph-view.js` (`loadForceGraph`) the first time the memory
  graph is opened; no runtime CDN requests.
- Why a CommonJS shim instead of `<script>` or `import()`: `17-files.js` loads Monaco's
  `vendor/monaco/loader.js`, which defines a global `define.amd`. The UMD wrapper checks
  `define.amd` before falling back to `window.ForceGraph`, so once the Files view has been opened a
  plain `<script>` would call Monaco's anonymous `define` instead. The loader fetches the text and
  runs it via `new Function("module", "exports", "define", source)` with `define` undefined, which
  takes the CommonJS branch. This needs `unsafe-eval` if the Tauri CSP (currently `null`) is ever
  tightened; the fallback then is an offline esbuild ESM re-bundle of the same version into this
  directory.
- Role: shared canvas renderer for graph views (memory knowledge graph now; R-368 B5 reference
  ego graph and R-307 B3 dependency DAG later via `dag-lr` layout). Decision recorded in
  `docs/design/memory_knowledge_graph.md` and `docs/design/doc_reference_graph.md`.
- Upgrade: re-download the new version, update the file name, `FORCE_GRAPH_URL` in
  `24-graph-view.js`, this README and the SHA-256 constant in `scripts/ui-memory-graph-smoke.mjs`.
