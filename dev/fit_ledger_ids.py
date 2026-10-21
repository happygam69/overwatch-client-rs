"""Put the fit ids into the ledger's feature cells.

    python dev/fit_ledger_ids.py

Each row of `docs/fitted-ledger.md` names the fit. The id (`FIT-CAM-01` and
friends) is added to the feature cell so the row and the code marker match.
`dev/fit_check.py` then checks the two sides against each other.
"""

from __future__ import annotations

import pathlib

ROOT = pathlib.Path(__file__).resolve().parent.parent
LEDGER = ROOT / "docs" / "fitted-ledger.md"

# feature cell -> id
IDS = {
    "View-model drag spring": "FIT-CAM-01",
    "Camera look filter": "FIT-CAM-02",
    "First-person pass depth clear": "FIT-CAM-03",
    "World camera near/far": "FIT-CAM-04",
    "Eye height": "FIT-CAM-05",
    "First-person pitch": "FIT-CAM-06",
    "Mover arithmetic": "FIT-MOV-01",
    "Steep slide": "FIT-MOV-02",
    "Crouch speed label": "FIT-MOV-03",
    "Player collision": "FIT-MOV-04",
    "Projectiles and shots vs. the map": "FIT-MOV-05",
    "Damage order": "FIT-COM-01",
    "Armour mitigation": "FIT-COM-02",
    "Shield regeneration": "FIT-COM-03",
    "Per-shot damage": "FIT-COM-04",
    "Spread, falloff, ammo": "FIT-COM-05",
    "Cast-length interpretation": "FIT-COM-06",
    "Bots": "FIT-COM-07",
    "Playback source (fallback)": "FIT-ANI-01",
    "First-person camera framing": "FIT-ANI-02",
    "First-person camera/view-model parameters": "FIT-ANI-03",
    "Weapon attachment bones": "FIT-ANI-04",
    "First-person blend tree": "FIT-ANI-05",
    "FP blend-item order and playback": "FIT-ANI-06",
    "Animation-to-skin join": "FIT-ANI-07",
    "Materials/textures": "FIT-PRE-01",
    "Shading/lighting": "FIT-PRE-02",
    "Bots/projectiles": "FIT-PRE-03",
}


def main() -> int:
    lines = LEDGER.read_text(encoding="utf-8").splitlines(keepends=True)
    done = 0
    for i, line in enumerate(lines):
        if not line.startswith("| "):
            continue
        cells = line.split("|")
        feature = cells[1].strip()
        for name, fid in IDS.items():
            if feature == name and fid not in line:
                cells[1] = f" {name} (`{fid}`) "
                lines[i] = "|".join(cells)
                done += 1
            elif feature == name:
                done += 1
    LEDGER.write_text("".join(lines), encoding="utf-8", newline="")
    print(f"{done} ledger row(s) carry an id")
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
