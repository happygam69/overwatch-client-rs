"""Mark the fitted, assumed and stand-in implementations in the code.

    python dev/fit_marks.py            # insert the markers
    python dev/fit_marks.py --check    # report where they are missing

Each row of `docs/fitted-ledger.md` has an id (`FIT-CAM-01` and friends). The
id goes next to the code that carries the fit, so a reader of the code can ask
"is this read or fitted?" and a reader of the ledger can find the code.
`dev/fit_check.py` then checks the two sides against each other.
"""

from __future__ import annotations

import sys

# id, file, anchor (first line containing it), what the marker says
MARKS = [
    ("FIT-CAM-01", "crates/ow-view/src/play.rs", "fn vm_spring_step", "view-model drag spring: form fitted, constants read"),
    ("FIT-CAM-02", "crates/ow-view/src/play.rs", "look_inverse", "camera look filter: time constant read, follower form fitted"),
    ("FIT-CAM-04", "crates/ow-view/src/main.rs", "// World projection.", "world camera near/far: stand-in (only the FP pair is read)"),
    ("FIT-CAM-03", "crates/ow-view/src/main.rs", "let fp_projection = Mat4::perspective_rh", "FP pass: near/far read, the depth clear is fitted"),
    ("FIT-CAM-05", "crates/ow-scene/src/lib.rs", "pub const EYE_HEIGHT", "eye height: assumed (the client's own was not measured)"),
    ("FIT-CAM-06", "crates/ow-scene/src/lib.rs", "pub fn view_model_matrix", "first-person pitch: whole-rig rotation fitted"),
    ("FIT-MOV-01", "crates/ow-sim/src/movement.rs", "pub const ACCEL", "mover rates: fitted to keyed traces (the control code is tier 3)"),
    ("FIT-MOV-02", "crates/ow-sim/src/movement.rs", "*-slide case (|v| = 4.6895 m/s", "steep slide: not modelled, tier 3"),
    ("FIT-MOV-03", "crates/ow-sim/src/movement.rs", "pub const FIELD_CROUCH", "crouch speed field: read value, label provisional"),
    ("FIT-MOV-04", "crates/ow-view/src/play.rs", "struct Sandbox", "player collision: stand-in (ground plane plus boxes)"),
    ("FIT-MOV-05", "crates/ow-sim/src/lib.rs", "/// A travelling projectile.", "projectiles and shots: no map collision, the harness treats the map as visual"),
    ("FIT-COM-01", "crates/ow-sim/src/rules.rs", "pub enum DamageOrder", "damage order: assumed"),
    ("FIT-COM-02", "crates/ow-sim/src/rules.rs", "pub enum ArmourMitigation", "armour mitigation: assumed"),
    ("FIT-COM-03", "crates/ow-sim/src/rules.rs", "pub struct Rules", "combat defaults: assumed (shield regen, spread, falloff)"),
    ("FIT-COM-04", "crates/ow-view/src/play.rs", "pub struct Supplied", "per-shot damage: supplied by the caller, not a fit"),
    ("FIT-COM-05", "crates/ow-view/src/play.rs", "fn hero_weapon", "spread, falloff and ammo: left unread"),
    ("FIT-COM-06", "crates/ow-view/src/play.rs", "fn volley_mode", "cast-length interpretation: assumed"),
    ("FIT-COM-07", "crates/ow-game/src/sandbox.rs", "pub fn bot_box()", "bots: stand-in boxes, no client AI"),
    ("FIT-ANI-01", "crates/ow-view/src/play.rs", "fn resolve_idle_clip", "playback fallback: single clip when there is no tree"),
    ("FIT-ANI-02", "crates/ow-view/src/main.rs", "fp_camera_drop", "first-person camera framing: drop/back fitted"),
    ("FIT-ANI-03", "crates/ow-view/src/play.rs", "fp_fov", "FP camera/view-model parameters: values read, the mapping is fitted"),
    ("FIT-ANI-04", "crates/ow-scene/src/lib.rs", "pub character_weapon_pose: Option<ow_tank::clip::BlendPose>,", "weapon attachment: read structure, chosen frame (c71 idle)"),
    ("FIT-ANI-05", "crates/ow-view/src/anim_tree.rs", "pub fn build_chains", "blend tree: read graph, fitted driver math"),
    ("FIT-ANI-06", "crates/ow-view/src/play.rs", "fn read_blend_set", "blend-item order: read mapping, fitted driver math"),
    ("FIT-ANI-07", "crates/ow-tank/src/clip.rs", "pub fn pair_bone_tables_with_floats", "animation-to-skin join: id join measured, surplus ids dropped"),
    ("FIT-PRE-01", "crates/ow-scene/src/lib.rs", "pub placeholder: SceneTexture", "materials: white placeholder where the key is missing"),
    ("FIT-PRE-02", "crates/ow-view/src/scene.wgsl", "fn fs_main(", "shading: stand-in key/fill/sky model"),
    ("FIT-PRE-03", "crates/ow-view/src/play.rs", "pub bots: Vec<Combatant>,", "bots and projectiles: drawn as boxes and markers"),
]


def marker(id: str, text: str) -> str:
    return f"// {id}: {text}\n"


def main() -> int:
    check = "--check" in sys.argv
    missing = 0
    for id, path, anchor, text in MARKS:
        try:
            lines = open(path, encoding="utf-8").readlines()
        except FileNotFoundError:
            print(f"{id}: {path} is gone")
            missing += 1
            continue
        if any(id in line for line in lines):
            continue
        hit = next((i for i, line in enumerate(lines) if anchor in line), None)
        if hit is None:
            print(f"{id}: anchor {anchor!r} not found in {path}")
            missing += 1
            continue
        if check:
            print(f"{id}: no marker in {path} (anchor {anchor!r})")
            missing += 1
            continue
        comment = "// " if path.endswith(".rs") else "// "
        indent = ""
        stripped = lines[hit].lstrip()
        if stripped and not stripped.startswith("//"):
            indent = lines[hit][: len(lines[hit]) - len(stripped)]
        lines.insert(hit, f"{indent}{comment}{id}: {text}\n")
        open(path, "w", encoding="utf-8", newline="").writelines(lines)
        print(f"{id}: marked {path}")
    if check:
        print(f"{missing} marker(s) missing")
        return 1 if missing else 0
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
