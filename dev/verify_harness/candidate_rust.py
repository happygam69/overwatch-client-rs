"""Tier-2 candidate: run a Rust implementation through a subprocess.

Two loaders live here:

* ``cargo`` - the shipped path. The probe is ``crates/ow-sim/src/bin/verify_probe.rs``
  built by ``cargo build --release``; it imports the real crate functions. A
  verdict is bound to the function's source file **and** the crate revision, so
  editing either invalidates the ledger entry.
* ``rust``  - the original ``rust_probe.rs`` built with plain ``rustc``. It is
  kept for the selftest's deliberately-broken entry and for old units; it is
  not the path a shipped-code unit uses.

The probe speaks a line protocol on stdin/stdout: one ``call`` request per
input, one ``ok``/``err`` response back. Pointer args cross as hex with explicit
reloc offsets, so the probe rebases them into its own buffer exactly like the C
candidate's wrapper and the oracle's arena write - the callee never sees the
relative form.

A build failure or a protocol violation is a load-stage failure of the unit,
never a silent pass.
"""

from __future__ import annotations

import os
import queue
import shutil
import struct
import subprocess
import tempfile
import threading
from dataclasses import dataclass
from typing import Dict, List, Optional, Sequence, Tuple

from emu_oracle import ArgSpec

HERE = os.path.dirname(os.path.abspath(__file__))
REPO_ROOT = os.path.abspath(os.path.join(HERE, "..", ".."))

# The legacy rustc probe (selftest / old units).
PROBE_SRC = os.path.join(HERE, "rust_probe.rs")

# The shipped-code probe.
OW_SIM_ROOT = os.path.join(REPO_ROOT, "crates", "ow-sim")
OW_SIM_MANIFEST = os.path.join(OW_SIM_ROOT, "Cargo.toml")
CARGO_PROBE_SRC = os.path.join(OW_SIM_ROOT, "src", "bin", "verify_probe.rs")
PROBE_BIN = "verify_probe"

# Which source file implements each probe entry. The ledger's candidate digest
# covers the file(s) listed here plus the probe itself and the crate revision.
# A new entry must add itself here; an unknown entry falls back to the whole
# crate root, which is conservative (more source in the digest, not less).
ENTRY_SOURCES: Dict[str, List[str]] = {
    "orthonormalize_camera_basis": ["crates/ow-sim/src/camera.rs"],
    "move_toward": ["crates/ow-sim/src/movement.rs"],
    "controller_advance": ["crates/ow-sim/src/movement.rs"],
}

# The mutation stage rewrites the *Rust function body*, so a probe entry that
# is a thin wrapper (controller_advance calls Controller::advance) must name
# the function whose lines mutate.py should edit.
ENTRY_MUTATION: Dict[str, str] = {
    "controller_advance": "advance",
}


def mutation_entry(entry: str) -> str:
    return ENTRY_MUTATION.get(entry, entry)

# A probe that deadlocks (or wedges on a poisoned pipe) must fail the call,
# not hang the harness forever. The oracle has MAX_STEPS; this is the
# candidate side's equivalent budget.
DEFAULT_CALL_TIMEOUT = 300.0


def _git_rev() -> str:
    try:
        out = subprocess.run(
            ["git", "-C", REPO_ROOT, "rev-parse", "HEAD"],
            capture_output=True,
            text=True,
            timeout=10,
        )
        if out.returncode == 0:
            return out.stdout.strip()
    except (OSError, subprocess.SubprocessError):
        pass
    return "unknown"


def entry_sources(entry: str) -> List[str]:
    """Repo-relative source files that implement ``entry``."""
    paths = ENTRY_SOURCES.get(entry)
    if paths is None:
        return ["crates/ow-sim/src/lib.rs"]
    return list(paths)


def candidate_binding(entry: str) -> Tuple[str, List[str], str]:
    """Digest of the candidate artifact, its source list, and the crate rev.

    The digest is over the *function's own source file(s)*, the probe source
    that calls them, and the git revision. A ledger verdict therefore expires
    when any of those change - not when an unrelated crate file changes.
    """
    import hashlib

    rel = entry_sources(entry)
    h = hashlib.sha256()
    resolved: List[str] = []
    for r in rel:
        p = os.path.join(REPO_ROOT, r)
        resolved.append(r)
        if os.path.exists(p):
            with open(p, "rb") as f:
                h.update(f.read())
        h.update(b"\0")
    if os.path.exists(CARGO_PROBE_SRC):
        with open(CARGO_PROBE_SRC, "rb") as f:
            h.update(f.read())
    h.update(b"\0")
    rev = _git_rev()
    h.update(rev.encode())
    return h.hexdigest(), resolved, rev


@dataclass
class RustCandidateResult:
    ok: bool
    ret_bits: int
    pointee_after: Dict[int, bytes]
    state_after: Optional[bytes] = None
    fault: Optional[str] = None


class RustCandidate:
    """One built probe binary, one long-lived protocol process.

    ``loader="cargo"`` builds the real crate probe. ``loader="rust"`` builds
    the legacy standalone ``rust_probe.rs``. ``source_override`` is used only
    by the mutation stage; for cargo it is paired with ``source_path`` and the
    whole crate is copied to a temporary directory so the real source is never
    edited in place.
    """

    def __init__(
        self,
        entry: str,
        arg_kinds: Sequence[str],
        ret_kind: str,
        work_dir: Optional[str] = None,
        probe_src: Optional[str] = None,
        source_override: Optional[str] = None,
        source_path: Optional[str] = None,
        loader: str = "rust",
        state_base: Optional[int] = None,
        state_bytes: Optional[bytes] = None,
        call_timeout: float = DEFAULT_CALL_TIMEOUT,
    ):
        self.entry = entry
        self.arg_kinds = list(arg_kinds)
        self.ret_kind = "u64" if ret_kind in ("void", None) else ret_kind
        self.is_void = ret_kind in ("void", None)
        self.call_timeout = call_timeout
        self.loader = loader
        self.state_mode = state_base is not None
        self.state_base = state_base
        self.state_bytes = state_bytes or b""
        work_dir = work_dir or tempfile.mkdtemp(prefix="verify_rust_")
        os.makedirs(work_dir, exist_ok=True)
        if loader == "cargo":
            self.exe = self._build_cargo(work_dir, source_override, source_path)
        else:
            if source_override is not None:
                # A mutant: write the mutated source into the work dir and
                # build from there so the probe binary and the source stay
                # paired.
                src_path = os.path.join(work_dir, "rust_probe_mutant.rs")
                with open(src_path, "w", encoding="utf-8") as f:
                    f.write(source_override)
                self.exe = self._build_rustc(src_path, work_dir)
            else:
                self.exe = self._build_rustc(probe_src or PROBE_SRC, work_dir)
        self.proc = subprocess.Popen(
            [self.exe],
            stdin=subprocess.PIPE,
            stdout=subprocess.PIPE,
            stderr=subprocess.PIPE,
            text=True,
            bufsize=1,
        )
        # Blocking readline() on a wedged child never returns; read on a
        # thread into a queue and put a timeout on every roundtrip.
        self._resp_q: "queue.Queue[str]" = queue.Queue()
        self._reader = threading.Thread(
            target=self._read_loop, args=(self.proc, self._resp_q), daemon=True
        )
        self._reader.start()
        # Handshake: a dead binary must fail here, not on the first real call.
        resp = self._roundtrip("ping")
        if resp != "pong":
            self.close()
            raise RuntimeError(f"rust probe handshake failed: {resp!r}")
        if self.state_mode:
            if not self.state_bytes:
                self.close()
                raise RuntimeError("state mode requested with an empty snapshot")
            resp = self._roundtrip(f"state {self.state_base:x} {self.state_bytes.hex()}")
            if resp != "ok":
                self.close()
                raise RuntimeError(f"rust probe state load failed: {resp!r}")

    @staticmethod
    def _read_loop(proc: subprocess.Popen, q: "queue.Queue[str]") -> None:
        assert proc.stdout is not None
        for line in iter(proc.stdout.readline, ""):
            q.put(line)
        q.put("")  # EOF sentinel

    @staticmethod
    def _cargo_bin_path(target_dir: str) -> str:
        name = PROBE_BIN + (".exe" if os.name == "nt" else "")
        return os.path.join(target_dir, "release", name)

    def _build_rustc(self, src: str, work_dir: str) -> str:
        exe = os.path.join(work_dir, "rust_probe.exe")
        cmd = ["rustc", "--edition", "2021", "-O", src, "-o", exe]
        proc = subprocess.run(cmd, capture_output=True, text=True)
        if proc.returncode != 0:
            raise RuntimeError(f"rust probe build failed:\n{proc.stderr.strip()}")
        return exe

    def _build_cargo(
        self,
        work_dir: str,
        source_override: Optional[str],
        source_path: Optional[str],
    ) -> str:
        """Build the real-crate probe; a mutant gets a temporary crate copy."""
        if source_override is None:
            manifest = OW_SIM_MANIFEST
            target_dir = os.environ.get("CARGO_TARGET_DIR") or os.path.join(
                REPO_ROOT, "target"
            )
            cmd = [
                "cargo",
                "build",
                "--release",
                "--offline",
                "--manifest-path",
                manifest,
                "--bin",
                PROBE_BIN,
            ]
            proc = subprocess.run(cmd, capture_output=True, text=True)
            if proc.returncode != 0:
                raise RuntimeError(
                    "cargo probe build failed:\n"
                    + (proc.stderr.strip() or proc.stdout.strip())
                )
            exe = self._cargo_bin_path(target_dir)
            if not os.path.exists(exe):
                raise RuntimeError(f"cargo probe built but not found: {exe}")
            return exe

        if not source_path:
            raise RuntimeError("cargo mutant needs the source file it mutates")
        crate_dir = os.path.join(work_dir, "crate")
        # ow-sim is deliberately dependency-free and has no build.rs; the
        # copy is the whole candidate. If it grows either, extend this copy
        # rather than mutating the checkout in place.
        shutil.copytree(
            OW_SIM_ROOT,
            crate_dir,
            ignore=shutil.ignore_patterns("target", "*.pdb"),
        )
        # The copy is outside the workspace, so replace the inherited
        # package fields with concrete ones. ow-sim has no dependencies.
        with open(os.path.join(crate_dir, "Cargo.toml"), "w", encoding="utf-8") as f:
            f.write(
                "[package]\n"
                'name = "ow-sim"\n'
                'version = "0.1.0"\n'
                'edition = "2021"\n'
                "\n"
                "[lib]\n"
                'path = "src/lib.rs"\n'
                "\n"
                "[[bin]]\n"
                f'name = "{PROBE_BIN}"\n'
                'path = "src/bin/verify_probe.rs"\n'
            )
        # source_path is repo-relative (e.g. crates/ow-sim/src/camera.rs);
        # translate it into the copy.
        rel = os.path.relpath(source_path, OW_SIM_ROOT)
        if rel.startswith(".."):
            raise RuntimeError(f"mutant source outside ow-sim: {source_path}")
        dst = os.path.join(crate_dir, rel)
        with open(dst, "w", encoding="utf-8") as f:
            f.write(source_override)
        target_dir = os.path.join(work_dir, "target")
        cmd = [
            "cargo",
            "build",
            "--release",
            "--offline",
            "--manifest-path",
            os.path.join(crate_dir, "Cargo.toml"),
            "--target-dir",
            target_dir,
            "--bin",
            PROBE_BIN,
        ]
        proc = subprocess.run(cmd, capture_output=True, text=True)
        if proc.returncode != 0:
            raise RuntimeError(
                "cargo mutant build failed:\n"
                + (proc.stderr.strip() or proc.stdout.strip())
            )
        exe = self._cargo_bin_path(target_dir)
        if not os.path.exists(exe):
            raise RuntimeError(f"cargo mutant built but not found: {exe}")
        return exe

    def _roundtrip(self, line: str) -> str:
        assert self.proc.stdin and self.proc.stdout
        try:
            self.proc.stdin.write(line + "\n")
            self.proc.stdin.flush()
        except (BrokenPipeError, OSError) as e:
            raise RuntimeError(f"rust probe stdin closed: {e}") from e
        try:
            resp = self._resp_q.get(timeout=self.call_timeout)
        except queue.Empty:
            raise RuntimeError(
                f"rust probe timed out after {self.call_timeout:.0f}s on {line[:60]!r}"
            )
        if resp == "":
            stderr = ""
            if self.proc.stderr:
                try:
                    stderr = self.proc.stderr.read()
                except OSError:
                    pass
            raise RuntimeError(f"rust probe died: {stderr.strip()}")
        return resp.strip()

    def call(self, args: Sequence[ArgSpec]) -> RustCandidateResult:
        parts = ["call", self.entry, str(len(args))]
        for a, k in zip(args, self.arg_kinds):
            if k == "ptr":
                relocs = ",".join(str(o) for o in a.relocs) if a.relocs else "-"
                if self.loader == "cargo":
                    guest = (
                        f"{a.guest:x}"
                        if getattr(a, "guest", None) is not None
                        else "-"
                    )
                    parts += ["ptr", a.mem.hex(), relocs, guest]
                else:
                    # The legacy rust_probe.rs speaks the two-field ptr form.
                    parts += ["ptr", a.mem.hex(), relocs]
            elif k == "f32":
                parts += ["f32", f"{struct.unpack('<I', struct.pack('<f', a.value))[0]:08x}"]
            elif k == "f64":
                parts += ["f64", f"{struct.unpack('<Q', struct.pack('<d', a.value))[0]:016x}"]
            else:
                parts += ["u64", f"{a.bits:016x}"]
        try:
            resp = self._roundtrip(" ".join(parts))
        except RuntimeError as e:
            return RustCandidateResult(ok=False, ret_bits=0, pointee_after={}, fault=str(e))
        toks = resp.split(" ")
        if toks[0] == "err":
            return RustCandidateResult(
                ok=False, ret_bits=0, pointee_after={}, fault=toks[1] if len(toks) > 1 else "err"
            )
        if toks[0] != "ok" or len(toks) < 2:
            return RustCandidateResult(
                ok=False, ret_bits=0, pointee_after={}, fault=f"bad response: {resp!r}"
            )
        ret = 0 if toks[1] == "-" else int(toks[1], 16)
        after: Dict[int, bytes] = {}
        oi = 2
        for i, k in enumerate(self.arg_kinds):
            if k != "ptr":
                continue
            if oi >= len(toks):
                return RustCandidateResult(
                    ok=False, ret_bits=0, pointee_after={}, fault="missing pointee in response"
                )
            after[i] = bytes.fromhex(toks[oi])
            oi += 1
        state_after = None
        if self.state_mode:
            if oi >= len(toks):
                return RustCandidateResult(
                    ok=False,
                    ret_bits=0,
                    pointee_after={},
                    fault="missing state in response",
                )
            state_after = bytes.fromhex(toks[oi])
        return RustCandidateResult(
            ok=True,
            ret_bits=ret,
            pointee_after=after,
            state_after=state_after,
            fault=None,
        )

    def close(self) -> None:
        if self.proc.stdin:
            try:
                self.proc.stdin.close()
            except OSError:
                pass
        try:
            self.proc.wait(timeout=5)
        except subprocess.TimeoutExpired:
            self.proc.kill()
            try:
                self.proc.wait(timeout=5)
            except subprocess.TimeoutExpired:
                pass

    def __enter__(self) -> "RustCandidate":
        return self

    def __exit__(self, *exc) -> None:
        self.close()
