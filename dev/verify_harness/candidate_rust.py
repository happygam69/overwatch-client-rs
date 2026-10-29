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

import json
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

# The default shipped-code crate/probe. A unit may name another crate with its
# `candidateCrate` / `candidateProbe` fields; the ENTRY_SOURCES table below
# still has to name the source files whose edits must invalidate the verdict.
DEFAULT_CRATE = "ow-sim"
DEFAULT_PROBE = "crates/ow-sim/src/bin/verify_probe.rs"

OW_SIM_ROOT = os.path.join(REPO_ROOT, "crates", "ow-sim")
CARGO_PROBE_SRC = os.path.join(OW_SIM_ROOT, "src", "bin", "verify_probe.rs")

# Which source file implements each probe entry. The ledger's candidate digest
# covers the file(s) listed here plus the probe itself and the crate revision.
# A new entry must add itself here. An unknown entry falls back to the named
# crate's lib.rs; that does not digest the function's own file, so a cargo unit
# outside this table is unfinished, not verified.
ENTRY_SOURCES: Dict[str, List[str]] = {
    "orthonormalize_camera_basis": ["crates/ow-sim/src/camera.rs"],
    "move_toward": ["crates/ow-sim/src/movement.rs"],
    "controller_advance": ["crates/ow-sim/src/movement.rs"],
    "salsa20_xor": ["crates/ow-casc/src/blte.rs"],
    "salsa20_state_init": ["crates/ow-casc/src/blte.rs"],
    "dynamic_index_hash": ["crates/ow-casc/src/index.rs"],
    "dynamic_index_key_hash": ["crates/ow-casc/src/index.rs"],
    "encoding_header_parse": ["crates/ow-casc/src/encoding.rs"],
    "chunk_guid": ["crates/ow-tank/src/mapguid.rs"],
    "cmf_key_154504": ["crates/ow-tank/src/manifest.rs"],
    "cmf_iv_154504": ["crates/ow-tank/src/manifest.rs"],
    "bundle_lookup": ["crates/ow-tank/src/bundle.rs"],
    "bundle_directory_lookup": ["crates/ow-tank/src/bundle.rs"],
    "payload_guid": ["crates/ow-tank/src/guid.rs"],
    "crc32_seeded": ["crates/ow-tank/src/material.rs"],
    "texture_request_count": ["crates/ow-tank/src/texture.rs"],
    "texture_count_bits_shifted": ["crates/ow-tank/src/texture.rs"],
    "texture_format_family_bpp": ["crates/ow-tank/src/texture.rs"],
    "texture_descriptor": ["crates/ow-tank/src/texture.rs"],
    "texture_descriptor_size_walk": ["crates/ow-tank/src/texture.rs"],
}

# The mutation stage rewrites the *Rust function body*, so a probe entry that
# is a thin wrapper (controller_advance calls Controller::advance) must name
# the function whose lines mutate.py should edit.
ENTRY_MUTATION: Dict[str, str] = {
    "controller_advance": "advance",
    "salsa20_xor": "salsa20",
    "dynamic_index_hash": "hash_index",
    "dynamic_index_key_hash": "key_hash",
    "encoding_header_parse": "write_header_fields",
    "chunk_guid": "chunk_guid",
    "cmf_key_154504": "key_154504",
    "cmf_iv_154504": "iv_154504",
    # The entry is a thin wrapper; the search the unit proves is `offset_of`.
    "bundle_lookup": "offset_of",
    # The entry is a thin wrapper; the walk the unit proves is `directory_lookup`.
    "bundle_directory_lookup": "directory_lookup",
    "payload_guid": "payload_of",
    "crc32_seeded": "crc32_seeded",
    "texture_request_count": "request_count",
    "texture_count_bits_shifted": "count_bits_shifted",
    "texture_format_family_bpp": "format_family_and_bpp",
    "texture_descriptor": "descriptor",
    "texture_descriptor_size_walk": "descriptor_size_walk",
}


def mutation_entry(entry: str) -> str:
    return ENTRY_MUTATION.get(entry, entry)

# A probe that deadlocks (or wedges on a poisoned pipe) must fail the call,
# not hang the harness forever. The oracle has MAX_STEPS; this is the
# candidate side's equivalent budget.
DEFAULT_CALL_TIMEOUT = 300.0

# A mutant is deliberately broken code, so an unbounded loop is an EXPECTED
# outcome and the call must fail fast. The mutation stage passes its own
# timeout; every startup roundtrip (ping, state load) keeps the normal budget
# so a cold first launch is never mistaken for a defect.
DEFAULT_STARTUP_TIMEOUT = DEFAULT_CALL_TIMEOUT

# Mutant builds share a small pool of cargo target directories per crate. The
# dependency graph (registry crates, ow-casc, ow-stu) is compiled once per
# slot instead of once per mutant, and up to MUTANT_BUILD_SLOTS mutants compile
# at the same time. A slot is checked out for one build at a time, and the
# mutant's own binary is copied out before the slot is returned, so no build
# can ever replace the binary a running probe was launched from.
MUTANT_BUILD_SLOTS = 4
_MUTANT_SLOT_QUEUES: Dict[str, "queue.Queue[int]"] = {}
_MUTANT_SLOT_GUARD = threading.Lock()


def _mutant_slot_queue(crate_key: str) -> "queue.Queue[int]":
    with _MUTANT_SLOT_GUARD:
        q = _MUTANT_SLOT_QUEUES.get(crate_key)
        if q is None:
            q = queue.Queue()
            for i in range(MUTANT_BUILD_SLOTS):
                q.put(i)
            _MUTANT_SLOT_QUEUES[crate_key] = q
        return q


def _artifact_stamp(path: str) -> Optional[Tuple[int, int]]:
    """(mtime_ns, size) of an artifact, or None when it does not exist.

    Two consecutive mutant builds must produce two different stamps: if the
    stamp is unchanged, cargo did not relink and the artifact belongs to an
    earlier build. That is a hard error, never a silent stale probe.
    """
    try:
        st = os.stat(path)
    except OSError:
        return None
    return (st.st_mtime_ns, st.st_size)


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


def entry_sources(entry: str, crate: str = DEFAULT_CRATE) -> List[str]:
    """Repo-relative source files that implement ``entry``."""
    paths = ENTRY_SOURCES.get(entry)
    if paths is None:
        return [f"crates/{crate}/src/lib.rs"]
    return list(paths)


def candidate_binding(
    entry: str,
    crate: str = DEFAULT_CRATE,
    probe: Optional[str] = None,
) -> Tuple[str, List[str], str]:
    """Digest of the candidate artifact, its source list, and the crate rev.

    The digest is over the *function's own source file(s)*, the probe source
    that calls them, and the git revision. A ledger verdict therefore expires
    when any of those change - not when an unrelated crate file changes.
    """
    import hashlib

    if probe is None:
        probe = f"crates/{crate}/src/bin/verify_probe.rs"
    rel = entry_sources(entry, crate)
    h = hashlib.sha256()
    resolved: List[str] = []
    for r in rel:
        p = os.path.join(REPO_ROOT, r)
        resolved.append(r)
        if os.path.exists(p):
            with open(p, "rb") as f:
                h.update(f.read())
        h.update(b"\0")
    probe_path = probe if os.path.isabs(probe) else os.path.join(REPO_ROOT, probe)
    if os.path.exists(probe_path):
        with open(probe_path, "rb") as f:
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
    edited in place. ``crate_dir`` / ``cargo_probe`` select the shipped crate
    and probe; they default to ow-sim's.
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
        startup_timeout: float = DEFAULT_STARTUP_TIMEOUT,
        crate_dir: Optional[str] = None,
        cargo_probe: Optional[str] = None,
    ):
        self.entry = entry
        self.arg_kinds = list(arg_kinds)
        self.ret_kind = "u64" if ret_kind in ("void", None) else ret_kind
        self.is_void = ret_kind in ("void", None)
        self.call_timeout = call_timeout
        self.startup_timeout = startup_timeout
        self.loader = loader
        self.state_mode = state_base is not None
        self.state_base = state_base
        self.state_bytes = state_bytes or b""
        self.crate_dir = (
            os.path.abspath(crate_dir) if crate_dir else OW_SIM_ROOT
        )
        self.cargo_probe = (
            os.path.abspath(cargo_probe) if cargo_probe else CARGO_PROBE_SRC
        )
        self.probe_bin = os.path.splitext(os.path.basename(self.cargo_probe))[0]
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
        resp = self._roundtrip("ping", timeout=self.startup_timeout)
        if resp != "pong":
            self.close()
            raise RuntimeError(f"rust probe handshake failed: {resp!r}")
        if self.state_mode:
            if not self.state_bytes:
                self.close()
                raise RuntimeError("state mode requested with an empty snapshot")
            resp = self._roundtrip(
                f"state {self.state_base:x} {self.state_bytes.hex()}",
                timeout=self.startup_timeout,
            )
            if resp != "ok":
                self.close()
                raise RuntimeError(f"rust probe state load failed: {resp!r}")

    @staticmethod
    def _read_loop(proc: subprocess.Popen, q: "queue.Queue[str]") -> None:
        assert proc.stdout is not None
        for line in iter(proc.stdout.readline, ""):
            q.put(line)
        q.put("")  # EOF sentinel

    def _cargo_bin_path(self, target_dir: str) -> str:
        name = self.probe_bin + (".exe" if os.name == "nt" else "")
        return os.path.join(target_dir, "release", name)

    def _mutant_target_dir(self, slot: int) -> str:
        """One of this crate's warm cargo target directories for mutants.

        Keyed by crate because two crates can each ship a `verify_probe`
        binary; mixing them in one target dir would let one crate's build
        answer the other's probe. A slot is a cache: it only ever supplies
        compiled dependencies. The mutant's own binary is copied out before
        the slot is released (see `_build_cargo`), never launched from here.
        """
        base_target = os.environ.get("CARGO_TARGET_DIR") or os.path.join(
            REPO_ROOT, "target"
        )
        return os.path.join(
            base_target,
            "verify_probe",
            os.path.basename(self.crate_dir),
            "_mutants",
            f"slot{slot}",
        )

    def _package_name(self) -> str:
        """The crate's cargo package name, for `cargo clean -p`."""
        if not hasattr(self, "_pkg_name"):
            import tomllib

            with open(os.path.join(self.crate_dir, "Cargo.toml"), "rb") as f:
                self._pkg_name = tomllib.load(f)["package"]["name"]
        return self._pkg_name

    @staticmethod
    def _ensure_cache_tag(target_dir: str) -> None:
        """Mark the slot as a cargo target dir so `cargo clean` accepts it.

        `cargo clean` refuses a directory without CACHEDIR.TAG - the same tag
        `cargo build` writes - and the first build in a fresh slot runs clean
        before it runs build.
        """
        tag = os.path.join(target_dir, "CACHEDIR.TAG")
        if os.path.exists(tag):
            return
        with open(tag, "w", encoding="utf-8") as f:
            f.write(
                "Signature: 8a477f597d28d172789f06886806bc55\n"
                "# This file is a cache directory tag created by cargo.\n"
            )

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
            manifest = os.path.join(self.crate_dir, "Cargo.toml")
            if not os.path.exists(manifest):
                raise RuntimeError(f"cargo crate manifest not found: {manifest}")
            # A per-crate target directory: two crates can each ship a
            # `verify_probe` binary, and one shared output path would let a
            # stale build of the other crate's probe answer a call.
            base_target = os.environ.get("CARGO_TARGET_DIR") or os.path.join(
                REPO_ROOT, "target"
            )
            target_dir = os.path.join(
                base_target, "verify_probe", os.path.basename(self.crate_dir)
            )
            cmd = [
                "cargo",
                "build",
                "--release",
                "--offline",
                "--manifest-path",
                manifest,
                "--target-dir",
                target_dir,
                "--bin",
                self.probe_bin,
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
        shutil.copytree(
            self.crate_dir,
            crate_dir,
            ignore=shutil.ignore_patterns("target", "*.pdb"),
        )
        # The copy is outside the workspace, so it needs a workspace root that
        # supplies the `workspace = true` package fields, lints and the
        # dependencies the crate actually uses. Writing that root is what lets
        # a crate with registry dependencies (ow-casc -> flate2) be mutated;
        # the old loader assumed a dependency-free crate.
        self._write_mutant_workspace(work_dir, crate_dir)
        # source_path is repo-relative (e.g. crates/ow-casc/src/blte.rs);
        # translate it into the copy.
        rel = os.path.relpath(source_path, self.crate_dir)
        if rel.startswith(".."):
            raise RuntimeError(
                f"mutant source outside {os.path.basename(self.crate_dir)}: {source_path}"
            )
        dst = os.path.join(crate_dir, rel)
        with open(dst, "w", encoding="utf-8") as f:
            f.write(source_override)
        # A fresh mutant workspace path would normally make cargo rebuild, but
        # cargo's freshness check is mtime-based and two builds inside one
        # second have been observed to be considered fresh. `cargo clean -p`
        # removes this package's artifacts/fingerprints from the slot's target
        # dir (the compiled dependencies stay), so the mutant is ALWAYS
        # recompiled from the source just written.
        slot = _mutant_slot_queue(os.path.basename(self.crate_dir)).get()
        try:
            target_dir = self._mutant_target_dir(slot)
            os.makedirs(target_dir, exist_ok=True)
            self._ensure_cache_tag(target_dir)
            clean_cmd = [
                "cargo",
                "clean",
                "--release",
                "--offline",
                "--manifest-path",
                os.path.join(work_dir, "Cargo.toml"),
                "--target-dir",
                target_dir,
                "-p",
                self._package_name(),
            ]
            cleaned = subprocess.run(clean_cmd, capture_output=True, text=True)
            if cleaned.returncode != 0:
                raise RuntimeError(
                    "cargo mutant clean failed:\n"
                    + (cleaned.stderr.strip() or cleaned.stdout.strip())
                )
            cmd = [
                "cargo",
                "build",
                "--release",
                "--offline",
                "--manifest-path",
                os.path.join(work_dir, "Cargo.toml"),
                "--target-dir",
                target_dir,
                "--bin",
                self.probe_bin,
            ]
            exe = self._cargo_bin_path(target_dir)
            before = _artifact_stamp(exe)
            proc = subprocess.run(cmd, capture_output=True, text=True)
            if proc.returncode != 0:
                raise RuntimeError(
                    "cargo mutant build failed:\n"
                    + (proc.stderr.strip() or proc.stdout.strip())
                )
            if not os.path.exists(exe):
                raise RuntimeError(f"cargo mutant built but not found: {exe}")
            # The artifact must have been relinked by THIS build. If the
            # stamp is unchanged, the slot served an earlier mutant's binary:
            # refuse it rather than run a stale probe.
            if _artifact_stamp(exe) == before:
                raise RuntimeError(
                    "cargo reported success but did not relink "
                    f"{exe}; refusing a possibly stale probe binary"
                )
            private_exe = os.path.join(
                work_dir, self.probe_bin + (".exe" if os.name == "nt" else "")
            )
            shutil.copy2(exe, private_exe)
        finally:
            _mutant_slot_queue(os.path.basename(self.crate_dir)).put(slot)
        return private_exe

    @staticmethod
    def _toml_value(v) -> str:
        """A TOML value for the scalar/inline-table forms a manifest uses."""
        if isinstance(v, bool):
            return "true" if v else "false"
        if isinstance(v, str):
            return json.dumps(v)
        if isinstance(v, (int, float)):
            return repr(v)
        if isinstance(v, list):
            return "[" + ", ".join(RustCandidate._toml_value(x) for x in v) + "]"
        if isinstance(v, dict):
            body = ", ".join(
                f"{k} = {RustCandidate._toml_value(x)}" for k, x in v.items()
            )
            return "{ " + body + " }"
        raise TypeError(f"unsupported TOML value: {v!r}")

    @classmethod
    def _write_mutant_workspace(cls, work_dir: str, crate_dir: str) -> None:
        """Write a standalone workspace root for the temporary crate copy.

        The crate's manifest uses `workspace = true` for its package fields,
        its lints and (for ow-casc) its dependencies. A copy outside the repo
        cannot see the real workspace root, so reproduce the parts it needs.
        Only the workspace dependencies the crate actually references are
        copied; a path dependency would point outside the temporary tree and
        is refused rather than silently broken.
        """
        import tomllib

        with open(os.path.join(REPO_ROOT, "Cargo.toml"), "rb") as f:
            root = tomllib.load(f)
        with open(os.path.join(crate_dir, "Cargo.toml"), "rb") as f:
            crate = tomllib.load(f)
        ws = root.get("workspace", {})
        used = set()
        for section in ("dependencies", "dev-dependencies", "build-dependencies"):
            for name, value in crate.get(section, {}).items():
                if isinstance(value, dict) and value.get("workspace") is True:
                    used.add(name)
        deps = {
            name: value
            for name, value in ws.get("dependencies", {}).items()
            if name in used
        }
        for name, value in deps.items():
            if isinstance(value, dict) and "path" in value:
                # A workspace path dependency (ow-tank -> ow-casc / ow-stu) is
                # a real crate in this repository, not a file the mutation
                # writes. Point the temporary workspace at the read-only
                # original with an absolute path; the mutated source lives in
                # the copied `crate/` member, so the original tree is never
                # edited. Relative paths would be resolved against the temp
                # root and silently point nowhere.
                p = value["path"]
                if not os.path.isabs(p):
                    p = os.path.join(REPO_ROOT, p)
                value = dict(value)
                value["path"] = os.path.normpath(p).replace("\\", "/")
                deps[name] = value
        lines = [
            "[workspace]",
            'resolver = "2"',
            'members = ["crate"]',
            "",
        ]

        def dump_table(prefix: str, table: dict) -> None:
            scalars = {k: v for k, v in table.items() if not isinstance(v, dict)}
            if scalars:
                lines.append(f"[{prefix}]")
                for k, v in scalars.items():
                    lines.append(f"{k} = {cls._toml_value(v)}")
                lines.append("")
            for k, v in table.items():
                if isinstance(v, dict):
                    dump_table(f"{prefix}.{k}", v)

        if ws.get("package"):
            dump_table("workspace.package", ws["package"])
        if ws.get("lints"):
            dump_table("workspace.lints", ws["lints"])
        if deps:
            lines.append("[workspace.dependencies]")
            for name, value in deps.items():
                lines.append(f"{name} = {cls._toml_value(value)}")
            lines.append("")
        with open(os.path.join(work_dir, "Cargo.toml"), "w", encoding="utf-8") as f:
            f.write("\n".join(lines))

    def _roundtrip(self, line: str, timeout: Optional[float] = None) -> str:
        assert self.proc.stdin and self.proc.stdout
        if timeout is None:
            timeout = self.call_timeout
        try:
            self.proc.stdin.write(line + "\n")
            self.proc.stdin.flush()
        except (BrokenPipeError, OSError) as e:
            raise RuntimeError(f"rust probe stdin closed: {e}") from e
        try:
            resp = self._resp_q.get(timeout=timeout)
        except queue.Empty:
            raise RuntimeError(
                f"rust probe timed out after {timeout:.0f}s on {line[:60]!r}"
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
                    # `-` is the empty byte string: an empty hex field would
                    # disappear under whitespace splitting, which made a
                    # zero-length pointee unrepresentable on the wire.
                    mem_hex = a.mem.hex() if a.mem else "-"
                    parts += ["ptr", mem_hex, relocs, guest]
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
            after[i] = b"" if toks[oi] == "-" else bytes.fromhex(toks[oi])
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
