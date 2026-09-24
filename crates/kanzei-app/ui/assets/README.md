# UI artwork

The active OC is `oc/character-v6.json` (format `kanzei.character-pack.v3`).
The owner-approved v5 reference has a slim androgynous silhouette, flat chest,
matte navy fabric, soft neutral lighting and loose sleeves that hang downward.
`master-soft-v5.png` is the 1024 x 1536 static fallback.

Twelve H.264 videos in `oc/clips-v6/` contain complete-character motion at
768 x 1152 and 24 fps. The graph has nine states and three idle variants. The
arm, elbow, wrist and sleeve are never assembled from separate moving layers.
The raising clip includes a held pose and joins a matching lowering clip;
a resting interval separates repetitions. Pending states wait for authored
neutral exit windows. Idle stabilization is recorded with its source hashes.

The renderer keys the pale grey backdrop and controls the closed/open mouth
using per-frame tracking and `mouth-soft-v6.png`. Mouth amplitude comes from
actual Web Audio playback. Hidden and reduced-motion views pause video
playback. A bounded cache keeps at most four video decoders.

`reference.png` preserves the original owner image. `demo-speech-c-v3.wav` is
the 30-second soundtrack using the selected C voice; `speech-main-c-v3.wav`
is its explanation line and the browser audio integration test input.

Raster keyframes and the mouth reference were edited with built-in imagegen.
Complete video clips were generated with the pinned H3 / Larry deployment.
PixiJS 7.4.3 and its license are in `../vendor/pixi/`.

Use `node scripts/oc-preview.mjs` from the repository root. See the
[design brief](../../../../docs/design/oc.md),
[production record](../../../../docs/design/oc-production.md), and
[idle direction](../../../../docs/design/oc-idle-direction.md).

Retired v4 runtime assets and implementations are preserved outside the
runtime directory at
`C:/Users/kanzei/Documents/kanzei-oc-archive/2026-09-24-v4-runtime/`.
The archive inventory records the exact file hashes.
