"""Digest-bound verification records.

Same contract as the Sandustry harness's ledger.js: a verdict is bound to
SHA-256 digests of the oracle bytes, the candidate source, the options, and
the surface list. Edit any of them and the entry reports `stale` rather than
`verified`. Without this, a green ledger entry can outlive the code it
described.

The ledger file lives at dev/verify_harness/ledger.json (gitignored: it is a
produced artifact, regenerable by re-running the harness).

Every entry also binds HARNESS_VERSION: the verifier itself is an input to
its own verdicts. A harness bugfix changes what `verified` means; entries
produced by an older harness report stale until re-run.
"""

from __future__ import annotations

import hashlib
import json
import os
import time
from dataclasses import dataclass, field, fields
from typing import Any, Dict, List, Optional

LEDGER_PATH = os.path.join(os.path.dirname(__file__), "ledger.json")

# Bump on every harness change that could alter a verdict (corpus changes,
# gate logic fixes, oracle behaviour fixes). Consumers binding old entries
# then see `stale`, never a verdict carried across a harness upgrade.
HARNESS_VERSION = "3"


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
    harness_version: str = "1"
    timestamp: float = 0.0
    # Where the candidate came from, when it is shipped crate code: the
    # function's source file(s) and the git revision the probe was built from.
    candidate_sources: List[str] = field(default_factory=list)
    crate_rev: str = ""

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
            "harness_version": self.harness_version,
            "timestamp": self.timestamp,
            "candidate_sources": self.candidate_sources,
            "crate_rev": self.crate_rev,
        }

    @staticmethod
    def from_json(d: Dict[str, Any]) -> "LedgerEntry":
        known = {f.name for f in fields(LedgerEntry)}
        dropped = set(d) - known
        if dropped:
            # A newer or hand-edited ledger: never crash the whole run over
            # a record we would overwrite anyway.
            print(f"  ledger: ignoring unknown entry keys: {sorted(dropped)}")
        return LedgerEntry(**{k: v for k, v in d.items() if k in known})


def load_ledger(path: Optional[str] = None) -> Dict[str, LedgerEntry]:
    path = path or LEDGER_PATH
    if not os.path.exists(path):
        return {}
    with open(path, "r", encoding="utf-8") as f:
        raw = json.load(f)
    return {e["unit_id"]: LedgerEntry.from_json(e) for e in raw.get("entries", [])}


def save_ledger(entries: Dict[str, LedgerEntry], path: Optional[str] = None) -> None:
    path = path or LEDGER_PATH
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
    verdict; `stale` otherwise. The caller turns stale into `inconclusive`.
    A verdict produced by a different harness version is stale by rule."""
    if (
        entry.oracle_sha256 == oracle_sha256
        and entry.candidate_sha256 == candidate_sha256
        and entry.options_sha256 == options_sha256
        and entry.surfaces_sha256 == surfaces_sha256
        and entry.harness_version == HARNESS_VERSION
    ):
        return "fresh"
    return "stale"


def record(
    entries: Dict[str, LedgerEntry],
    entry: LedgerEntry,
    path: Optional[str] = None,
) -> None:
    entry.timestamp = time.time()
    entries[entry.unit_id] = entry
    save_ledger(entries, path)
