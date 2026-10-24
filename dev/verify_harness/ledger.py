"""Digest-bound verification records.

Same contract as the Sandustry harness's ledger.js: a verdict is bound to
SHA-256 digests of the oracle bytes, the candidate source, the options, and
the surface list. Edit any of them and the entry reports `stale` rather than
`verified`. Without this, a green ledger entry can outlive the code it
described.

The ledger file lives at dev/verify_harness/ledger.json (gitignored: it is a
produced artifact, regenerable by re-running the harness).
"""

from __future__ import annotations

import hashlib
import json
import os
import time
from dataclasses import dataclass, field
from typing import Any, Dict, List, Optional

LEDGER_PATH = os.path.join(os.path.dirname(__file__), "ledger.json")


def sha256_bytes(b: bytes) -> str:
    return hashlib.sha256(b).hexdigest()


def sha256_file(path: str) -> str:
    with open(path, "rb") as f:
        return sha256_bytes(f.read())


@dataclass
class LedgerEntry:
    unit_id: str
    status: str  # verified | divergent | inconclusive | invalid
    oracle_sha256: str
    candidate_sha256: str
    options_sha256: str
    surfaces_sha256: str
    coverage: Optional[float] = None
    mutation_kill_rate: Optional[float] = None
    mutants_killed: int = 0
    mutants_total: int = 0
    equivalent_mutants: List[Dict[str, Any]] = field(default_factory=list)
    acknowledged_gaps: List[Dict[str, Any]] = field(default_factory=list)
    divergences: List[Dict[str, Any]] = field(default_factory=list)
    notes: List[str] = field(default_factory=list)
    timestamp: float = 0.0

    def to_json(self) -> Dict[str, Any]:
        return {
            "unit_id": self.unit_id,
            "status": self.status,
            "oracle_sha256": self.oracle_sha256,
            "candidate_sha256": self.candidate_sha256,
            "options_sha256": self.options_sha256,
            "surfaces_sha256": self.surfaces_sha256,
            "coverage": self.coverage,
            "mutation_kill_rate": self.mutation_kill_rate,
            "mutants_killed": self.mutants_killed,
            "mutants_total": self.mutants_total,
            "equivalent_mutants": self.equivalent_mutants,
            "acknowledged_gaps": self.acknowledged_gaps,
            "divergences": self.divergences[:50],
            "notes": self.notes,
            "timestamp": self.timestamp,
        }

    @staticmethod
    def from_json(d: Dict[str, Any]) -> "LedgerEntry":
        return LedgerEntry(**d)


def load_ledger(path: str = LEDGER_PATH) -> Dict[str, LedgerEntry]:
    if not os.path.exists(path):
        return {}
    with open(path, "r", encoding="utf-8") as f:
        raw = json.load(f)
    return {e["unit_id"]: LedgerEntry.from_json(e) for e in raw.get("entries", [])}


def save_ledger(entries: Dict[str, LedgerEntry], path: str = LEDGER_PATH) -> None:
    payload = {"version": 1, "entries": [e.to_json() for e in entries.values()]}
    tmp = path + ".tmp"
    with open(tmp, "w", encoding="utf-8") as f:
        json.dump(payload, f, indent=1, sort_keys=True)
    os.replace(tmp, path)


def check_entry(
    entry: LedgerEntry,
    oracle_sha256: str,
    candidate_sha256: str,
    options_sha256: str,
    surfaces_sha256: str,
) -> str:
    """`fresh` if every digest still describes the inputs that produced the
    verdict; `stale` otherwise. The caller turns stale into `inconclusive`."""
    if (
        entry.oracle_sha256 == oracle_sha256
        and entry.candidate_sha256 == candidate_sha256
        and entry.options_sha256 == options_sha256
        and entry.surfaces_sha256 == surfaces_sha256
    ):
        return "fresh"
    return "stale"


def record(
    entries: Dict[str, LedgerEntry],
    entry: LedgerEntry,
    path: str = LEDGER_PATH,
) -> None:
    entry.timestamp = time.time()
    entries[entry.unit_id] = entry
    save_ledger(entries, path)
