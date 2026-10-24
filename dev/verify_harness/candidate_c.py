"""Tier-1 candidate: compile a Ghidra decompile, call it through ctypes.

The candidate is the decompile from dev/decompiles/, compiled next to
ghidra_shim.h and a generated wrapper. The wrapper exists only to re-encode
the return value as raw bits (a float return in XMM0 does not survive a
ctypes round-trip through a Python float without risking NaN-payload loss);
it contains no behaviour of its own.

A compile failure is a load-stage failure of the unit, never a silent pass.
"""

from __future__ import annotations

import ctypes
import os
import struct
import subprocess
import sys
import tempfile
from dataclasses import dataclass
from typing import Dict, List, Optional, Sequence

from emu_oracle import ArgSpec, f32_bits

SHIM_PATH = os.path.join(os.path.dirname(__file__), "ghidra_shim.h")

_CTYPES_BY_KIND = {
    "f32": ctypes.c_float,
    "f64": ctypes.c_double,
    "u64": ctypes.c_uint64,
    "ptr": ctypes.c_void_p,
}


@dataclass
class CandidateResult:
    ok: bool
    ret_bits: int
    pointee_after: Dict[int, bytes]  # arg index -> bytes after the call
    fault: Optional[str] = None


class CCandidate:
    """One compiled decompile, loaded as a shared library."""

    def __init__(
        self,
        c_file: str,
        entry: str,
        arg_kinds: Sequence[str],
        ret_kind: str,
        work_dir: Optional[str] = None,
        source_override: Optional[str] = None,
    ):
        self.entry_name = entry
        self.arg_kinds = list(arg_kinds)
        # Accept "void" as an alias for no return value.
        self.ret_kind = "u64" if ret_kind in ("void", None) else ret_kind
        self.is_void = ret_kind in ("void", None)
        self.work_dir = work_dir or tempfile.mkdtemp(prefix="verify_c_")
        os.makedirs(self.work_dir, exist_ok=True)
        src = source_override
        if src is None:
            with open(c_file, "r", encoding="utf-8") as f:
                src = f.read()
        self.dll_path = self._compile(c_file, src, entry)
        self.dll = ctypes.CDLL(self.dll_path)
        self.fn = getattr(self.dll, "harness_entry")
        # The wrapper re-encodes the return as raw bits in a uint32_t (f32),
        # uint64_t (f64/u64). restype MUST match: with a fixed c_uint32, a
        # f64/u64 return was truncated to its low 32 bits and every such
        # unit diverged (or worse, agreed on the truncated half).
        self.fn.restype = (
            ctypes.c_uint64 if self.ret_kind in ("f64", "u64") else ctypes.c_uint32
        )
        self.fn.argtypes = [_CTYPES_BY_KIND[k] for k in arg_kinds]
        # Keep pointee buffers alive across the call.
        self._bufs: List[Optional[ctypes.Array]] = []

    def _compile(self, c_file: str, src: str, entry: str) -> str:
        shim_dir = os.path.dirname(SHIM_PATH)
        decomp_dir = os.path.dirname(os.path.abspath(c_file))
        body_path = os.path.join(self.work_dir, "body.c")
        wrap_path = os.path.join(self.work_dir, "wrap.c")
        dll_path = os.path.join(self.work_dir, "candidate.dll")
        with open(body_path, "w", encoding="utf-8") as f:
            f.write(src)
        # Wrapper: call the entry by its raw FUN_* name, return bits.
        # The __declspec fallback keeps the same wrapper buildable on hosts
        # where gcc does not target Windows (verification still compares
        # against the same bytes; the dll/so naming is the harness's choice).
        call_args = self._call_args_decl()
        compat = "\n".join([
            "#ifndef _WIN32",
            "#define __declspec(x)",
            "#endif",
            "",
        ])
        if self.is_void:
            wrap = f"""#include <stdint.h>
{compat}#include "ghidra_shim.h"
#include "body.c"
__declspec(dllexport) uint32_t harness_entry({self._c_params()}) {{
    {entry}({call_args});
    return 0;
}}
"""
        elif self.ret_kind == "f32":
            wrap = f"""#include <stdint.h>
{compat}#include "ghidra_shim.h"
#include "body.c"
__declspec(dllexport) uint32_t harness_entry({self._c_params()}) {{
    union {{ float f; uint32_t u; }} u; u.f = {entry}({call_args}); return u.u;
}}
"""
        elif self.ret_kind == "f64":
            wrap = f"""#include <stdint.h>
{compat}#include "ghidra_shim.h"
#include "body.c"
__declspec(dllexport) uint64_t harness_entry({self._c_params()}) {{
    union {{ double d; uint64_t u; }} u; u.d = {entry}({call_args}); return u.u;
}}
"""
        elif self.ret_kind == "u64":
            wrap = f"""#include <stdint.h>
{compat}#include "ghidra_shim.h"
#include "body.c"
__declspec(dllexport) uint64_t harness_entry({self._c_params()}) {{
    return (uint64_t){entry}({call_args});
}}
"""
        with open(wrap_path, "w", encoding="utf-8") as f:
            f.write(wrap)
        cmd = [
            "gcc",
            "-shared",
            "-O0",
            "-fno-strict-aliasing",
            # A sibling FUN_* the decompile calls but that no include defines
            # is a load-stage failure of the unit (the protocol's stance),
            # not a runtime crash inside unknown code.
            "-Werror=implicit-function-declaration",
            "-o",
            dll_path,
            wrap_path,
            f"-I{shim_dir}",
            f"-I{self.work_dir}",
            f"-I{decomp_dir}",
        ]
        proc = subprocess.run(cmd, capture_output=True, text=True)
        if proc.returncode != 0:
            raise RuntimeError(
                f"candidate compile failed:\n{proc.stderr.strip()}"
            )
        return dll_path

    def _c_params(self) -> str:
        parts = []
        for i, k in enumerate(self.arg_kinds):
            if k == "ptr":
                parts.append(f"const void *a{i}")
            elif k == "f32":
                parts.append(f"float a{i}")
            elif k == "f64":
                parts.append(f"double a{i}")
            else:
                parts.append(f"uint64_t a{i}")
        return ", ".join(parts) if parts else "void"

    def _call_args_decl(self) -> str:
        # Pointer args go through as void*; C implicitly converts void* to
        # any object pointer type at the call, and the decompile's own
        # prototype (body.c) is what the compiler checks against. A wrong
        # pointee type is then a compile error or a divergence, never a
        # silent cast to the wrong shape.
        return ", ".join(f"a{i}" for i in range(len(self.arg_kinds)))

    def call(self, args: Sequence[ArgSpec]) -> CandidateResult:
        cargs = []
        self._bufs = []
        for a, k in zip(args, self.arg_kinds):
            if k == "ptr":
                mem = a.mem
                if a.relocs:
                    # Rebase relative pointers to this buffer's host address.
                    # The buffer address is only known after allocation, so
                    # build it, patch, then hand the patched bytes out.
                    buf = (ctypes.c_uint8 * len(mem))()
                    ctypes.memmove(buf, mem, len(mem))
                    base = ctypes.addressof(buf)
                    for off in a.relocs:
                        rel = struct.unpack_from("<Q", buf, off)[0]
                        struct.pack_into("<Q", buf, off, base + rel)
                else:
                    buf = (ctypes.c_uint8 * len(mem)).from_buffer_copy(mem)
                self._bufs.append(buf)
                cargs.append(ctypes.cast(buf, ctypes.c_void_p))
            elif k == "f32":
                cargs.append(ctypes.c_float(a.value))
            elif k == "f64":
                cargs.append(ctypes.c_double(a.value))
            else:
                cargs.append(ctypes.c_uint64(a.bits))
        try:
            ret = self.fn(*cargs)
        except Exception as e:  # a crash inside the DLL must not kill the run
            return CandidateResult(ok=False, ret_bits=0, pointee_after={}, fault=str(e))
        after: Dict[int, bytes] = {}
        for i, (a, k) in enumerate(zip(args, self.arg_kinds)):
            if k == "ptr":
                buf = self._bufs[i]
                after[i] = bytes(bytearray(buf))
        return CandidateResult(ok=True, ret_bits=int(ret), pointee_after=after, fault=None)

    def close(self) -> None:
        """Unload the library so a per-unit or per-mutant work dir can be
        deleted (a loaded DLL/so pins its file on Windows)."""
        dll = getattr(self, "dll", None)
        if dll is None:
            return
        handle = getattr(dll, "_handle", None)
        if handle is None:
            return
        # Drop the ctypes objects that pin the module before freeing.
        # Keep a local reference to the raw handle; FreeLibrary needs it
        # pointer-sized (c_void_p), not the default c_int, on Win64.
        self.fn = None
        self.dll = None
        try:
            if sys.platform == "win32":
                kernel32 = ctypes.windll.kernel32
                kernel32.FreeLibrary.argtypes = [ctypes.c_void_p]
                kernel32.FreeLibrary.restype = ctypes.c_int
                kernel32.FreeLibrary(ctypes.c_void_p(handle))
            else:
                import _ctypes

                _ctypes.dlclose(handle)
        except Exception:
            pass
        # Encourage CPython to drop any remaining CDLL internals so the
        # OS refcount can hit zero and Windows will allow the unlink.
        try:
            import gc

            gc.collect()
        except Exception:
            pass
