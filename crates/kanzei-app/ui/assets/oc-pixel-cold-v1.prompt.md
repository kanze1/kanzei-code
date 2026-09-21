# Reserved pixel OC performance atlas

Generated with the built-in imagegen tool on 2026-09-21. The generated RGBA PNG is copied unchanged. Six columns contain body/head poses; four rows contain the base face, blink, small speech mouth and larger speech mouth. Runtime eye and mouth crops make those features independently controllable. The art is not a Live2D rig.

Identity references: the approved `oc-pixel-motion-v1.png` and the owner's original `C:/Users/kanzei/Desktop/kanzeiOC/4858512eb268378517c84d82747d1418.png`.

## Initial generation

Use case: identity-preserve
Asset type: production transparent pixel-art character animation spritesheet, composable eye and mouth states.

Input 1 is the edit target / established pixel OC identity and body proportions. Input 2 is the original OC reference for her cool, emotionally reserved facial character, amber eyes, black bob and cyan cheek mark. Keep the established pixel art rendering from input 1, not the detailed anime rendering of input 2.

Create ONE exact 6-column by 4-row sprite atlas, square overall canvas, ideally 1536x1536 or 2048x2048. All 24 cells are equally sized 2:3 portrait rectangles. True transparent alpha background. No text, labels, lines, numbers, borders, shadows outside character, backgrounds or accessories.
Each cell is the SAME adult woman bust framed from top of hair to waist, full hair and shoulders inside each cell, consistent visual scale, same baseline and torso position, modest 5% transparent margin. No clipping across cell edges. Black chin-length bob, amber gold eyes, small cyan circuit diamond on her LEFT cheek (viewer's right in frontal views), slender neck circuit marking, loose charcoal V-neck sweater. Slim small flat chest, ORIGINAL natural shoulder width from input 1, elegant long slender hands and fingers when visible, normal five fingers. Do not narrow her shoulders, enlarge chest, shorten fingers, or exaggerate anatomy.

CRITICAL STYLE: quiet, reserved, cerebral, calmly confident, slightly detached. Relaxed brows, alert focused eyes with gently lowered upper lids. Mouth usually a very small neutral straight line. Never cheerful, coy, bashful, babyish, sleepy, hostile or sneering. No blush, cute waving, broad smiles or closed-eye smile. Crisp manually drawn pixel shapes, limited warm charcoal / pale skin / amber palette, consistent outlines and integer-looking pixel clusters. Minimal cyan mark only. Preserve OC identity.

Columns are six different BASE POSES, left to right:
1. CALM: face mostly front, head subtly three-quarter to viewer's left, arms hang naturally at sides, relaxed neutral expression.
2. SIDE ATTENTION: same torso and relaxed arms, head turns modestly to viewer's left, eyes look attentively to that side, calm expression. This is a gentle turn, not profile.
3. SKEPTICAL: same torso and relaxed arms, head turns modestly to viewer's right, ONE eyebrow just slightly raised, neutral tiny mouth. Thoughtful questioning, not angry.
4. THINKING: head slightly bowed and turned to viewer's left, eyes directed downward; one elegant hand lifted with a long slender index finger lightly touching the side of the chin, other arm relaxed down. Keep gesture quiet.
5. EXPLAINING: looks mostly forward, attentive eyes slightly more open; one hand at lower shoulder level with a slim index finger gently raised, other fingers naturally relaxed; no waving. Gesture small and self-possessed, body unforced.
6. ACKNOWLEDGEMENT: both arms naturally down, head subtly inclined in a small nod, gaze toward viewer, mouth corners almost imperceptibly raised in a restrained closed-mouth expression. No toothy grin, no blush.

ROWS are FACIAL STATE VARIANTS of those same six poses. They will be cropped and composited independently at runtime:
ROW 1 (top): base expressions, eyes open, mouths closed.
ROW 2: EXACT PIXEL-ALIGNED copies of row 1 with ONLY eyes closed in a natural blink. Eyebrows, head position, hair, body, pose, mouth and lighting absolutely identical to row 1. Do not nod or smile.
ROW 3: EXACT PIXEL-ALIGNED copies of row 1 with ONLY mouth slightly open for quiet speech. Eyes, eyebrows, head position, hair, body, gesture and lighting identical to row 1. Small tasteful dark mouth, no teeth.
ROW 4 (bottom): EXACT PIXEL-ALIGNED copies of row 1 with ONLY mouth moderately open, still restrained, for another speech mouth shape. Eyes, eyebrows, head position, hair, body, gesture and lighting identical to row 1. No exaggerated O mouth.

Preserve exact row-to-row registration within each column: face and body outlines stay identical so we can independently overlay blink and mouth regions. Background pixels must truly be transparent, NOT black, white or a painted checkerboard. Return only this 24-cell atlas.

## Facial-state repair

Edit ONLY the attached 6-column by 4-row transparent pixel-art atlas. Preserve exact grid, all character poses, all bodies, shoulder widths, slender hands, hair, clothing, scale, colors and the entire TOP ROW. This is a technical facial-state repair for animation.

The atlas has 6 columns and 4 rows. Please count carefully.
ROW 2 (second from top): all SIX characters MUST have both eyes fully closed in a relaxed BLINK. In particular column 3 currently has open eyes: close both eyes there. Keep each head at EXACTLY the position and angle of the top-row counterpart. Mouths remain closed.
ROW 3 (third from top): all SIX characters must have a visibly slightly open talking mouth: a small dark narrow opening with a skin lip outline, not a flat line. No teeth. In particular columns 1,2,3,4,6 need real open mouth openings. Eyes remain open and alert, NOT smiling.
ROW 4 (bottom): all SIX characters must have a moderately open talking mouth, clearly taller than row 3 yet restrained. A small dark oval with a muted inner-mouth pixel highlight, no teeth. In particular columns 4 and 6 need the mouth open, not closed or smiling. Eyes remain open. Keep all mouth centers in exactly the top row mouth locations.
Do not redesign faces, move features, enlarge heads, change lighting, or change body positions. We must crop only eyes and mouth to overlay on the TOP ROW; pixel alignment is crucial.
The cyan mark must stay on the character's anatomical LEFT cheek, never mirrored. For column 3's turn toward viewer's right, hide the mark on the far cheek rather than placing it on the near/viewer's-left cheek. Apply this mark fix in column 3 across all rows while preserving every other top-row pixel.
Keep true transparent alpha, exact 6x4 grid and square overall aspect. No text, labels, grid borders, checkerboard, new objects or effects.

Final source: `C:/Users/kanzei/.codex/generated_images/01a0c2f5-1aa4-7071-a2d1-43551158c392/exec-255d61ee-522d-4991-ac2d-3a1380073d4e.png`.
