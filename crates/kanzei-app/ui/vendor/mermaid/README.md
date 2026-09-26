# Mermaid runtime (ESM chunked build)

- Version: 12.0.0 (npm `mermaid@12.0.0`, published 2026-09-10), MIT.
- Source tarball: https://registry.npmjs.org/mermaid/-/mermaid-12.0.0.tgz
  - registry `dist.integrity`: `sha512-/wQXC9iBxoGV8p3erbvaXs9h77VyLDBH6GdayVjj3hEcSQhFU4N1WUhUppotCEqlIxI2pRMwjwBSwTB1MfZBgQ==` (verified)
  - registry `dist.shasum` (SHA-1): `3c4cdf54fd24110c988b90def857127055eb842b` (verified)
  - tarball SHA-256: `7df1e7de572d26ea7aca5eaa7b0e77f5caacb63567006f4077c2753d730ffd9d`
- Files taken from `package/dist/`, byte-for-byte, nothing else:
  - `mermaid.esm.min.mjs` (entry, 30 198 bytes), SHA-256
    `6bd10ac3b89c196062102fc8e89c849e440fd8587891b58c46155b956f61ab52`
  - `chunks/mermaid.esm.min/*.mjs` (104 chunks, 5 427 631 bytes); SHA-256 of
    `sha256sum` over all 105 `.mjs` files sorted by path (run inside this directory,
    `find . -name '*.mjs' | sort | xargs sha256sum | sha256sum`):
    `f2896aef35a84fcf99f002fd6909e7d325e2934ad7283fa572cb43b99a7a9fb7`
  - no `.map` files (15 MB of source maps are left out); the IIFE `mermaid.min.js`
    (5.6 MB single file) is not used.
  - Hashes refer to the upstream bytes (the git blobs); a Windows checkout with
    `core.autocrlf=true` may show CRLF in the working tree, which is harmless for JS.
- `LICENSE` is the unmodified upstream MIT license. Third-party code bundled inside the
  chunks (ELK layout under EPL-2.0, DOMPurify, KaTeX, cytoscape, d3, …) is listed with
  licenses and source locations in `THIRD_PARTY.md`; the license texts and copyright
  notices available so far are collected in `LICENSES-THIRD-PARTY.txt` (added by Kanzei,
  not part of the upstream tarball). The chunks must not be edited
  (EPL-2.0 applies per file to the ELK chunk).

## How Kanzei loads it

- Loaded locally and lazily by `ui/04-diagram.js` with
  `import("./vendor/mermaid/mermaid.esm.min.mjs")` the first time a diagram is rendered
  (architecture page, or a closed ```` ```mermaid ```` fence in any rendered Markdown).
  The architecture page preloads it when the browser is idle. No CDN requests.
- Only the chunks a diagram type needs are fetched (flowchart + ELK ≈ 3 MB on first use).
- Configuration is fixed in `04-diagram.js`: `securityLevel: "strict"`, `htmlLabels: false`,
  `theme: "base"` with `themeVariables` read from the `--diagram-*` tokens in `style.css`,
  `look: "neo"`, `layout: "elk"`. See `docs/design/architecture_diagrams.md`.
- Fallback plan if 12.0.0 shows a blocking regression: mermaid 11.17.2 +
  `@mermaid-js/layout-elk`, same configuration.
