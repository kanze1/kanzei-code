# KaTeX runtime

- Version: `katex@0.19.0`, MIT; upstream license is in `LICENSE`.
- Source: https://registry.npmjs.org/katex/-/katex-0.19.0.tgz
- Verified npm tarball integrity: `sha512-v6Tznz3zJ7u3niRCoDTsumM2+HA2XXcCu+WAacCeHD2z3p9A9Ks987o5FzfTGBN0e8A0vjEgIDvLbICrXpdw/Q==`.
- Unmodified upstream files: `dist/katex.mjs`, `dist/katex.min.css`, `dist/fonts/*`, `dist/contrib/copy-tex.mjs`, and `LICENSE`. Source maps and unused extensions are excluded.

The shared Markdown parser imports this public ESM entry through `04-math.js`. The stylesheet and fonts load locally. Math is rendered synchronously from protected TeX spans before ordinary Markdown punctuation is processed; no CDN or asynchronous DOM replacement is involved. The browser composition root installs upstream `copy-tex.mjs` for selection copying. This is independent of Mermaid's private, versioned KaTeX chunk.
