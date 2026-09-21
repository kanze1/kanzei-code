# UI artwork

`oc-pixel-v3.png` is the active OC artwork for both themes. It was generated with
the built-in imagegen tool on 2026-09-21, using the project owner's original
`kanzeiOC/4858512eb268378517c84d82747d1418.png` and
`kanzeiOC/dd88ac584626ec713ad96765b48171e8.png` as identity references.
The built-in imagegen tool then refined the same sprite to a leaner body and a
small, flat chest, preserving the face, hair, pose and circuit coordinates.
The generated 1024 x 1536 PNG and its alpha channel are preserved without pixel edits.
Both the original and revision prompts are in `oc-pixel-v3.prompt.md`.

The UI uses warm charcoal, paper white and clear amber-gold status accents, informed
by the [Radix neutral-palette guidance](https://www.radix-ui.com/colors/docs/palette-composition/composing-a-palette).
These are custom theme tokens; text contrast is checked separately.

`22-oc-companion.js` aligns simple SVG circuit paths to the cheek and neck marks.
The portrait and its marks share a two-pixel breathing motion. Thinking lights
the cheek; tool execution sends square light points inward; reply streaming
sends them outward; completion briefly settles to gold.

Real task events drive these effects. Current-session identity and runtime
terminal state take precedence over animation history. The decorative layer
does not modify task state. The welcome view shows the large sprite, while
conversations reserve a small right margin when the chat area is at least 900px.
Narrow views hide it, hidden views pause it, and reduced-motion mode uses static light.
