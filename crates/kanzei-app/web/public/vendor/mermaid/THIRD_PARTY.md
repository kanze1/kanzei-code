# Third-party components bundled in the Mermaid 12.0.0 chunks

The files in this directory are the unmodified upstream `mermaid@12.0.0` ESM build
(see `README.md` for hashes). Mermaid's build inlines several dependencies into the
chunks; they are redistributed here as-is, in object-code (minified) form. Some chunks
keep their upstream license banners (`/*! Bundled license information: … */`, e.g.
DOMPurify, cytoscape, lodash-es) at the end of the file.

| Component | Version range (mermaid 12.0.0 `package.json`) | License | Upstream source |
|---|---|---|---|
| mermaid | 12.0.0 | MIT (`LICENSE` in this directory) | https://github.com/mermaid-js/mermaid |
| @mermaid-js/parser | ^2.0.0 | MIT | https://github.com/mermaid-js/mermaid/tree/develop/packages/parser |
| **elkjs** (Eclipse Layout Kernel, chunk `elk-*.mjs`) | ^0.9.3 | **EPL-2.0** | https://github.com/kieler/elkjs · https://github.com/eclipse/elk |
| DOMPurify | ^3.4.12 | MPL-2.0 OR Apache-2.0 | https://github.com/cure53/DOMPurify |
| KaTeX | ^0.16.47 | MIT | https://github.com/KaTeX/KaTeX |
| cytoscape | ^3.34.0 | MIT | https://github.com/cytoscape/cytoscape.js |
| cytoscape-cose-bilkent, cose-base, layout-base | ^4.1.0 | MIT | https://github.com/cytoscape/cytoscape.js-cose-bilkent |
| cytoscape-fcose | ^2.2.0 | MIT | https://github.com/iVis-at-Bilkent/cytoscape.js-fcose |
| d3 (and its d3-* modules) | ^7.9.0 | ISC | https://github.com/d3/d3 |
| d3-sankey | ^0.12.3 | BSD-3-Clause | https://github.com/d3/d3-sankey |
| dagre-d3-es | 7.0.14 | MIT | https://github.com/tbo47/dagre-es |
| roughjs | ^4.6.6 | MIT | https://github.com/rough-stuff/rough |
| khroma | ^2.1.0 | MIT | https://github.com/fabiospampinato/khroma |
| marked | ^16.3.0 | MIT | https://github.com/markedjs/marked |
| stylis | ^4.3.6 | MIT | https://github.com/thysultan/stylis |
| dayjs | ^1.11.21 | MIT | https://github.com/iamkun/dayjs |
| chevrotain | ~11.1.2 | Apache-2.0 | https://github.com/chevrotain/chevrotain |
| langium, vscode-languageserver-types (via @mermaid-js/parser) | — | MIT | https://github.com/eclipse-langium/langium |
| lodash-es | — | MIT | https://github.com/lodash/lodash |
| es-toolkit | ^1.45.1 | MIT | https://github.com/toss/es-toolkit |
| uuid | ^11 – ^14 | MIT | https://github.com/uuidjs/uuid |
| ts-dedent | ^2.2.0 | MIT | https://github.com/tamino-martinius/node-ts-dedent |
| @braintree/sanitize-url | ^7.1.2 | MIT | https://github.com/braintree/sanitize-url |
| @iconify/utils | ^3.0.2 | MIT | https://github.com/iconify/iconify |
| @upsetjs/venn.js | ^2.0.0 | MIT | https://github.com/upsetjs/venn.js |

## EPL-2.0 notice (elkjs)

The ELK layout engine bundled in `chunks/mermaid.esm.min/elk-*.mjs` is made available
under the Eclipse Public License 2.0 (https://www.eclipse.org/legal/epl-2.0/). It is
distributed here unmodified, in object-code form, exactly as shipped by mermaid 12.0.0.
The corresponding Source Code is available from the upstream projects listed above
(elkjs 0.9.x: https://github.com/kieler/elkjs/tree/0.9.3, ELK: https://github.com/eclipse/elk).
The EPL-2.0 is a file-level copyleft: it covers that chunk only and does not extend to
Kanzei's own source. Do not edit the chunk; to change the layout engine, replace the
whole mermaid build and update `README.md`.

## Full license texts

`LICENSES-THIRD-PARTY.txt` in this directory collects the texts that ship with the build:

- **Kept as banners inside the chunks** (copied verbatim into Part A): lodash-es
  (MIT), DOMPurify 3.4.12 (MPL-2.0 OR Apache-2.0; redistributed under Apache-2.0), and
  the third-party snippets inside cytoscape (Promises/A+ thenable, jQuery event object,
  bezier and spring function generators; MIT).
- **LICENSE files from the npm tarballs** (Part B): d3 7.9.0 and each `d3-*` module
  (ISC, Mike Bostock / Observable, Inc.), roughjs 4.6.6 (MIT, Preet Shihn).
- **Apache License 2.0 terms** (Part C): chevrotain, and DOMPurify's Apache option.
- **LICENSE files from the npm tarballs of the remaining components** (Part D): elkjs (EPL-2.0 full
  text), KaTeX, cytoscape, cytoscape-cose-bilkent, cose-base (1.x and 2.x), layout-base,
  cytoscape-fcose, d3-sankey, dagre-d3-es, khroma, marked, stylis, dayjs, chevrotain,
  @mermaid-js/parser, langium, vscode-languageserver-types, es-toolkit (incl. NOTICE), uuid, ts-dedent,
  @braintree/sanitize-url, @iconify/utils, @upsetjs/venn.js — fetched from registry.npmjs.org
  (versions listed in each Part D header).
