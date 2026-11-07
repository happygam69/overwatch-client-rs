"""Decompile one client function by address and write it under dev/decompiles.

Dev-only convenience around dev/gh.py (the MCP bridge). Output is decompiled
client code: it stays on this machine (dev/ is gitignored) and is never quoted
into docs/ - docs record addresses and conclusions only.

    python dev/gh_decompile.py 0x142667be0 [more...]
"""
from __future__ import annotations

import json
import os
import sys

sys.path.insert(0, os.path.dirname(os.path.abspath(__file__)))
import gh  # noqa: E402

OUT = os.path.join(os.path.dirname(os.path.abspath(__file__)), "decompiles")


def main(argv: list[str]) -> int:
    os.makedirs(OUT, exist_ok=True)
    for raw in argv[1:]:
        addr = raw if raw.startswith("0x") else "0x" + raw
        body = gh.call(
            "/get_functions",
            {
                "function": addr,
                "fields": "decompiled_code,signature,entry_point,callers,callees,body_end",
                "program": "Overwatch.exe",
            },
        )
        if "error" in body:
            print(addr, "ERROR", body["error"])
            continue
        code = body.get("decompiled_code") or ""
        name = body.get("name") or "fn"
        path = os.path.join(OUT, f"{name}_{addr}.c")
        with open(path, "w", encoding="utf-8") as f:
            f.write(code)
        print(f"{addr} {name} {len(code.splitlines())} lines -> {path}")
        callers = [c.get("name") for c in (body.get("callers") or [])]
        callees = [c.get("name") for c in (body.get("callees") or [])]
        if callers:
            print("   callers:", ", ".join(x for x in callers if x)[:200])
        if callees:
            print("   callees:", ", ".join(x for x in callees if x)[:200])
    return 0


if __name__ == "__main__":
    raise SystemExit(main(sys.argv))
