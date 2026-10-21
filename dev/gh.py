"""Call the Ghidra bridge over HTTP.

The MCP tool registry in a long-running session can go stale after Ghidra is
restarted (the bridge re-registers its tools, the client keeps the old list).
The bridge serves the same tools as plain HTTP POST endpoints, so this is the
way in when the MCP tool names stop resolving.

Usage:
    python gh.py <path> [json-body]
    python gh.py /list_open_programs
    python gh.py /get_functions '{"functions":"0x7ff7c5e822e0","fields":"decompiled_code","program":"module_v2.bin"}'
    python gh.py --schema <substring>     # find endpoints
"""

from __future__ import annotations

import json
import subprocess
import sys

BASE = "http://127.0.0.1:8089"


def call(path: str, body: dict | None = None, timeout: int = 120) -> dict:
    """POST through curl: this sandbox lets curl reach the bridge but not a
    Python socket (getaddrinfo is refused for raw sockets)."""
    out = subprocess.run(
        ["curl", "-s", "--max-time", str(timeout), "-X", "POST",
         "-H", "Content-Type: application/json",
         "--data-binary", json.dumps(body or {}), BASE + path],
        capture_output=True, text=True,
    )
    raw = out.stdout
    try:
        return json.loads(raw)
    except json.JSONDecodeError:
        return {"raw": raw[:4000], "stderr": out.stderr[:400]}


def get(path: str, timeout: int = 20) -> dict:
    out = subprocess.run(["curl", "-s", "--max-time", str(timeout), BASE + path],
                         capture_output=True, text=True)
    return json.loads(out.stdout)


def main() -> int:
    if len(sys.argv) < 2:
        print(__doc__)
        return 1
    if sys.argv[1] == "--schema":
        schema = get("/mcp/schema")
        needle = sys.argv[2].lower() if len(sys.argv) > 2 else ""
        for t in schema["tools"]:
            if needle in (t["path"] + " " + t["description"]).lower():
                params = ", ".join(p["name"] for p in t.get("params", []))
                print(f"{t['path']}  ({params})")
        return 0
    body = json.loads(sys.argv[2]) if len(sys.argv) > 2 else {}
    out = call(sys.argv[1], body)
    print(json.dumps(out, indent=1)[:8000])
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
