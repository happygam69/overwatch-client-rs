"""Send the animation-driving keys to a running client, on a schedule.

Same mechanism as the movement-discovery tooling (`keybd_event` with
`KEYEVENTF_SCANCODE`, which is what a game reads), the same discipline: a beat log
so a capture can be aligned to what was pressed, and a guard that refuses to send
anything unless the foreground window belongs to the target process — so a stray
`v` can never land in another application.

Keys: melee (V), jump (Space), crouch (Ctrl by default, `--crouch-key c` for C).

Usage:
    python anim_input.py --pid N --seconds 30 --pattern cycle
    python anim_input.py --pid N --seconds 20 --pattern melee --period 0.5
    python anim_input.py --pid N --pattern cycle --dry-run

Not read-only in the memory sense — it sends input, which is an outward-facing
action — but it never opens the process for write, never reads its memory, and
touches no window other than to ask whether it is focused.
"""

from __future__ import annotations

import ctypes
import sys
import time

SCANCODE = 0x0008
KEYUP = 0x0002

SC = {
    "v": 0x2F,          # melee
    "space": 0x39,      # jump
    "ctrl": 0x1D,       # crouch (default bind)
    "c": 0x2E,          # crouch (alternate bind)
    "shift": 0x2A,
}

u = ctypes.windll.user32


def key(scan: int, down: bool) -> None:
    u.keybd_event(0, scan, SCANCODE | (0 if down else KEYUP), 0)


def tap(scan: int, hold: float = 0.06) -> None:
    key(scan, True)
    time.sleep(hold)
    key(scan, False)


def foreground_pid() -> int:
    pid = ctypes.c_ulong(0)
    u.GetWindowThreadProcessId(u.GetForegroundWindow(), ctypes.byref(pid))
    return pid.value


def main() -> int:
    args = sys.argv[1:]
    def flag(name, default=None):
        return args[args.index(name) + 1] if name in args else default

    pid = int(flag("--pid", "0"))
    if not pid:
        print("pass --pid")
        return 2
    seconds = float(flag("--seconds", "30"))
    pattern = flag("--pattern", "cycle")
    period = float(flag("--period", "0.5"))
    crouch_key = SC[flag("--crouch-key", "ctrl")]
    dry = "--dry-run" in args
    out = flag("--out", "D:/ow-dumps/anim_input.log")

    log = open(out, "w", encoding="utf-8")

    def say(line: str) -> None:
        print(line)
        log.write(f"{time.monotonic():.3f} {line}\n")
        log.flush()

    # the schedule: (label, key or None, presses per period)
    if pattern == "melee":
        plan = [("melee", SC["v"])]
    elif pattern == "jump":
        plan = [("jump", SC["space"])]
    elif pattern == "crouch":
        plan = [("crouch", crouch_key)]
    else:                                   # cycle: a stretch of each
        third = max(1.0, seconds / 3)
        plan = [("melee", SC["v"]), ("jump", SC["space"]), ("crouch", crouch_key)]
        seconds = third

    say(f"pid {pid} pattern {pattern} period {period}s phase {seconds}s key(s) "
        f"{[n for n, _ in plan]}{' (dry run)' if dry else ''}")

    for label, scan in plan:
        say(f"=== {label}")
        beats = max(1, int(seconds / period))
        for i in range(beats):
            fg = foreground_pid()
            if not dry and fg != pid:
                say(f"   {label} beat {i+1}/{beats}: SKIPPED — foreground pid {fg}, not {pid}; focus the game")
                time.sleep(period)
                continue
            if scan is not None:
                tap(scan)
            say(f"   {label} beat {i+1}/{beats} press sent")
            time.sleep(max(0.0, period - 0.06))
    say(f"done; log {out}")
    log.close()
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
