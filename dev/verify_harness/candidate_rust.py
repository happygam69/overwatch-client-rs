"""Tier-2 candidate: run a Rust implementation through a subprocess.

The probe (rust_probe.rs) is built once with rustc and speaks a line
protocol on stdin/stdout: one `call` request per input, one `ok`/`err`
response back. Pointer args cross as hex with explicit reloc offsets, so
the probe rebases them into its own buffer exactly like the C candidate's
wrapper and the oracle's arena write - the callee never sees the relative
form.

This is the path a future clean-room Rust implementation takes: put the
behaviour in rust_probe.rs (or a successor binary), declare a unit with
`candidateLoader: "rust"`, and the same five-stage ladder applies.

A build failure or a protocol violation is a load-stage failure of the
unit, never a silent pass.
"""

from __future__ import annotations

import os
import struct
import subprocess
import tempfile
from dataclasses import dataclass
from typing import Dict, List, Optional, Sequence

from emu_oracle import ArgSpec

PROBE_SRC = os.path.join(os.path.dirname(__file__), "rust_probe.rs")


@dataclass
class RustCandidateResult:
    ok: bool
    ret_bits: int
    pointee_after: Dict[int, bytes]
    fault: Optional[str] = None


class RustCandidate:
    """One built probe binary, one long-lived protocol process."""

    def __init__(
        self,
        entry: str,
        arg_kinds: Sequence[str],
        ret_kind: str,
        work_dir: Optional[str] = None,
        probe_src: Optional[str] = None,
        source_override: Optional[str] = None,
    ):
        self.entry = entry
        self.arg_kinds = list(arg_kinds)
        self.ret_kind = "u64" if ret_kind in ("void", None) else ret_kind
        self.is_void = ret_kind in ("void", None)
        work_dir = work_dir or tempfile.mkdtemp(prefix="verify_rust_")
        os.makedirs(work_dir, exist_ok=True)
        if source_override is not None:
            # A mutant: write the mutated source into the work dir and build
            # from there so the probe binary and the source stay paired.
            src_path = os.path.join(work_dir, "rust_probe_mutant.rs")
            with open(src_path, "w", encoding="utf-8") as f:
                f.write(source_override)
            self.exe = self._build(src_path, work_dir)
        else:
            self.exe = self._build(probe_src or PROBE_SRC, work_dir)
        self.proc = subprocess.Popen(
            [self.exe],
            stdin=subprocess.PIPE,
            stdout=subprocess.PIPE,
            stderr=subprocess.PIPE,
            text=True,
            bufsize=1,
        )
        # Handshake: a dead binary must fail here, not on the first real call.
        resp = self._roundtrip("ping")
        if resp != "pong":
            self.close()
            raise RuntimeError(f"rust probe handshake failed: {resp!r}")

    def _build(self, src: str, work_dir: str) -> str:
        exe = os.path.join(work_dir, "rust_probe.exe")
        cmd = ["rustc", "--edition", "2021", "-O", src, "-o", exe]
        proc = subprocess.run(cmd, capture_output=True, text=True)
        if proc.returncode != 0:
            raise RuntimeError(f"rust probe build failed:\n{proc.stderr.strip()}")
        return exe

    def _roundtrip(self, line: str) -> str:
        assert self.proc.stdin and self.proc.stdout
        self.proc.stdin.write(line + "\n")
        self.proc.stdin.flush()
        resp = self.proc.stdout.readline()
        if not resp:
            stderr = ""
            if self.proc.stderr:
                stderr = self.proc.stderr.read()
            raise RuntimeError(f"rust probe died: {stderr.strip()}")
        return resp.strip()

    def call(self, args: Sequence[ArgSpec]) -> RustCandidateResult:
        parts = ["call", self.entry, str(len(args))]
        for a, k in zip(args, self.arg_kinds):
            if k == "ptr":
                relocs = ",".join(str(o) for o in a.relocs) if a.relocs else "-"
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
        return RustCandidateResult(ok=True, ret_bits=ret, pointee_after=after, fault=None)

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

    def __enter__(self) -> "RustCandidate":
        return self

    def __exit__(self, *exc) -> None:
        self.close()
