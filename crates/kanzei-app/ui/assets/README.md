# UI artwork

`oc-pixel-cold-v1.png` is the active OC artwork for both themes: a transparent
six-column, four-row atlas. It was generated with the built-in imagegen tool on
2026-09-21 from the approved pixel portrait and the owner's original
`kanzeiOC/4858512eb268378517c84d82747d1418.png` identity reference.
The animation preserves the natural shoulder width and slim chest, with slender
hands and relaxed arms in the neutral pose. Six poses cover calm attention,
sideways attention, skepticism, thought, explanation and a quiet acknowledgment.
Expressions are restrained; completion uses a subtle nod rather than a wave.
The generated PNG and its alpha channel are preserved without pixel edits.
Generation and revision prompts are in `oc-pixel-cold-v1.prompt.md`.
`oc-pixel-motion-v1.png` and `oc-pixel-v3.png` remain earlier references.

The UI uses warm charcoal, paper white and clear amber-gold status accents, informed
by the [Radix neutral-palette guidance](https://www.radix-ui.com/colors/docs/palette-composition/composing-a-palette).
These are custom theme tokens; text contrast is checked separately.

`22-oc-performance.js` samples body pose and blinking on independent clocks.
The first row supplies the body; calibrated face masks reveal closed eyes from
row two and two mouth openings from rows three and four. Speech amplitude is
provided separately through `setSpeaking` and `setMouthLevel`. Text events alone
never open the mouth. The cheek light follows each pose's calibrated coordinates.
Pose and blink timers run only at their next deadline; mouth updates are supplied
by the audio consumer. Completion plays once and settles. The portrait and light
share a subtle breathing motion; execution sends small light points inward and
completion settles to gold.

The standalone `output/playwright/oc-cold-preview.html` demonstrates the poses and
lip movement driven by an existing, owner-approved English voice sample. The
sample is embedded only in the local preview, not bundled with the product. This
uses sprite compositing rather than a Live2D rig. The product's `23-voice-*`
modules separately connect live microphone input, streaming TTS and playback amplitude.

Real task events drive these effects. Current-session identity and runtime
terminal state take precedence over animation history. The decorative layer
does not modify task state. The welcome view shows the large sprite, while
conversations reserve a small right margin when the chat area is at least 900px.
Narrow views hide it, hidden views pause it, and reduced-motion mode uses a static
neutral pose and light. The sprite controller cleans up its timers and listeners.
