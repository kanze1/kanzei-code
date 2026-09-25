# UI artwork

The active OC is `oc/character-v7.json`, format `kanzei.character-pack.v3`.
Its charcoal matte work shirt, short cuffs and restrained brass shoulder seams
match the graphite/amber interface. The slim androgynous silhouette, original
face, dark bob, gold eyes and cyan cheek fracture remain the identity anchors.

Eleven complete-character H.264 clips have a logical size of 576 x 864 at 24 fps.
The encoded size is 576 x 1728: straight RGB above, greyscale alpha below.
The action graph contains nine states and three idle variants. Each hand
gesture includes its entire raise, hold and return, followed by quiet rest.

Transparency and closed mouths are baked offline. `mouth-soft-v6.png` supplies
the open-mouth reference with per-frame affine tracking and actual Web Audio
amplitude. `master-workwear-v7-alpha.png` supplies the transparent still frame.
The renderer uploads newly decoded frames, keeps at most four cached video
entries, and plays one stream in steady state or two during transitions.

The left-rail character toggle defaults off. Switching it on loads the player;
switching off destroys it. Hidden views and reduced motion pause video playback.

`reference.png` is the original owner image. `demo-speech-c-v7.wav` is the new
30-second sample soundtrack, recorded through the selected C voice service.
Raster artwork was edited with built-in imagegen; whole-character videos were
generated with the pinned H3 / Larry deployment. PixiJS 7.4.3 and its license
are in `../vendor/pixi/`.

See the [design](../../../../docs/design/oc.md),
[production record](../../../../docs/design/oc-production.md), and
[playback research](../../../../docs/design/oc-playback.md).
