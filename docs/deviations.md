# Deviations: legacy non-1-to-1 code

**This file is a removal list, not an accepted design.** Every row is code
that does not yet match the client, carries the same `FIT-*` id next to the
code, and must be deleted or replaced by a read implementation.

No new row may be added. `cargo xtask deviation-check` fails if a `FIT-*` id in
the code is not in this table, or if a new id appears at all.

The simulation deviations are gone with the deleted movement/combat code. One
`ow-tank` deviation remains (`FIT-ANI-07`); the rest are presentation and
view-model framing in `ow-client` / `ow-scene`. The wider list of `ow-tank`
code that is not 1 to 1, with or without a marker, is
[`ow-tank-1to1.md`](ow-tank-1to1.md).

## Camera and view model

| id | what deviates | removal |
|---|---|---|
| `FIT-ANI-02` | FP camera framing uses `--fp-cam-drop`/`--fp-cam-back` instead of the client's FP pose. | Read and apply the FP pose; then remove the pass framing. |
| `FIT-ANI-04` | Weapon attachment source is chosen from the FP skeleton with a fold; the reference retarget is missing. | Read the retarget and the attachment path. |
| `FIT-CAM-03` | Depth clear between world and FP passes; the client's exact pass boundary is unread. | Read the FP pass setup and depth policy. |
| `FIT-CAM-05` | Viewer eye height `1.75 m`; the client's own camera height is unread. | Read the camera height from the FP pose/component. |
| `FIT-CAM-06` | Whole FP rig rotated by camera pitch instead of the client's per-bone distribution. | Read the FP camera pose application. |

## Animation join

| id | what deviates | removal |
|---|---|---|
| `FIT-ANI-07` | Animation-to-skin join drops ids not on the skin. | Read the reference rig and the surplus-id owner. |

## Presentation

| id | what deviates | removal |
|---|---|---|
| `FIT-PRE-01` | White material placeholder where content is missing. | Keep as an explicit missing-content display, never as client behaviour. |
| `FIT-PRE-02` | Viewer shading is viewer-only, not the client's renderer. | Out of the gameplay gate; document as viewer-only. |
