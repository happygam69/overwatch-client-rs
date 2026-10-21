# Automated movement for a live capture.
#
# Holds keys down in the Overwatch window for a scripted, repeatable sequence -
# the input side of `dev/live_trace.py`'s keyed trace. Scancodes are sent
# (games read raw input and ignore virtual-key-only events), the window is
# forced to the foreground first (AttachThreadInput dance, because a background
# process may not call SetForegroundWindow), and every block prints a timestamp
# so a log can be aligned with the trace.
#
# The sequence is deliberately shaped for ramp fitting: a long standstill, a
# long steady walk (captures the accel ramp at its start), a stop (decel
# ramp), diagonals, backpedal, a crouch walk with *both* crouch transitions
# (walk -> crouch and crouch -> walk, ctrl toggled while W stays held), crouch
# from standstill then crouch walk from rest, jumps while moving, and a
# standing jump with W pressed mid-air (air accel from zero) — the crouch and
# air-control transitions `ctrl_probe.py` measures.
#
# Usage:
#   powershell -ExecutionPolicy Bypass -File dev\inject_movement.ps1 `
#       -TargetPid <overwatch pid> -Seconds 300
#
# `-Seconds 300` (five minutes) is what the movement capture needs: the cycle
# below is ~45 s, so it gives six or more repetitions of every transition, and
# the repetition is what bounds the sub-frame input timing.
#
# Read-only with respect to the game process: this only *sends input* to the
# window, exactly as a keyboard would. No memory is touched.

param(
    [Parameter(Mandatory = $true)][int]$TargetPid,
    [int]$Seconds = 90,
    [int]$InitialIdleSeconds = 3,
    [string]$Mode = "script"
)

Add-Type @"
using System;
using System.Runtime.InteropServices;
public class Win {
    [DllImport("user32.dll")] public static extern bool SetForegroundWindow(IntPtr h);
    [DllImport("user32.dll")] public static extern bool ShowWindow(IntPtr h, int cmd);
    [DllImport("user32.dll")] public static extern IntPtr GetForegroundWindow();
    [DllImport("user32.dll")] public static extern uint GetWindowThreadProcessId(IntPtr h, out uint pid);
    [DllImport("user32.dll")] public static extern bool AttachThreadInput(uint a, uint b, bool attach);
    [DllImport("kernel32.dll")] public static extern uint GetCurrentThreadId();
    [DllImport("user32.dll")] public static extern void keybd_event(byte vk, byte scan, uint flags, UIntPtr extra);
    [DllImport("user32.dll")] public static extern void mouse_event(uint flags, int dx, int dy, uint data, UIntPtr extra);
}
"@

# Scancodes (set 1). Games read these; virtual keys alone are often ignored.
$SC = @{ w = 0x11; a = 0x1E; s = 0x1F; d = 0x20; space = 0x39; ctrl = 0x1D; shift = 0x2A }
$VK = @{ w = 0x57; a = 0x41; s = 0x53; d = 0x44; space = 0x20; ctrl = 0x11; shift = 0x10 }
$KEYUP = 0x0002
$SCANCODE = 0x0008

function Send-Key([string]$name, [bool]$down) {
    $flags = 0
    if (-not $down) { $flags = $flags -bor $KEYUP }
    $flags = $flags -bor $SCANCODE
    [Win]::keybd_event($VK[$name], $SC[$name], $flags, [UIntPtr]::Zero)
}

function Hold([string[]]$keys, [double]$seconds) {
    foreach ($k in $keys) { Send-Key $k $true }
    Start-Sleep -Milliseconds ([int]($seconds * 1000))
    foreach ($k in $keys) { Send-Key $k $false }
}

function Tap([string]$key, [int]$ms = 120) {
    Send-Key $key $true
    Start-Sleep -Milliseconds $ms
    Send-Key $key $false
}

function Stamp([string]$label) {
    Write-Output ("{0}  {1}" -f (Get-Date -Format "HH:mm:ss.fff"), $label)
}

# --- focus the game window -------------------------------------------------
$proc = Get-Process -Id $TargetPid -ErrorAction Stop
$h = $proc.MainWindowHandle
if ($h -eq [IntPtr]::Zero) { Write-Error "pid $TargetPid has no main window"; exit 1 }
[Win]::ShowWindow($h, 9) | Out-Null          # SW_RESTORE
$fg = [Win]::GetForegroundWindow()
$fgPid = 0
[Win]::GetWindowThreadProcessId($fg, [ref]$fgPid) | Out-Null
$me = [Win]::GetCurrentThreadId()
$theirThread = [Win]::GetWindowThreadProcessId($fg, [ref]$fgPid)
[Win]::AttachThreadInput($me, $theirThread, $true) | Out-Null
[Win]::SetForegroundWindow($h) | Out-Null
[Win]::AttachThreadInput($me, $theirThread, $false) | Out-Null
Start-Sleep -Milliseconds 400

$fgPid2 = 0
[Win]::GetWindowThreadProcessId([Win]::GetForegroundWindow(), [ref]$fgPid2) | Out-Null
if ($fgPid2 -ne $TargetPid) {
    Write-Output "WARNING: foreground is pid $fgPid2, not $TargetPid - key input may not reach the game"
} else {
    Write-Output "Overwatch is foreground; injecting for $Seconds s"
}

# --- fire mode: exercise the weapon code path (volley registration + fire),
# --- which is what commits the client's lazily-decrypted code pages --------
function MouseFire([int]$ms = 80) {
    [Win]::mouse_event(0x0002, 0, 0, 0, [UIntPtr]::Zero)   # LEFTDOWN
    Start-Sleep -Milliseconds $ms
    [Win]::mouse_event(0x0004, 0, 0, 0, [UIntPtr]::Zero)   # LEFTUP
}
if ($Mode -eq "fire") {
    Stamp "fire mode for $Seconds s (LMB every 0.7 s, W held, R reload every 6 shots)"
    Start-Sleep -Seconds $InitialIdleSeconds
    $t0 = Get-Date
    $shots = 0
    while (((Get-Date) - $t0).TotalSeconds -lt $Seconds) {
        Send-Key "w" $true
        MouseFire 80
        $shots++
        Start-Sleep -Milliseconds 620
        if ($shots % 6 -eq 0) {
            Send-Key "w" $false
            Tap "r" 150     # reload
            Start-Sleep -Milliseconds 900
        }
    }
    Send-Key "w" $false
    Stamp "done, $shots shot(s)"
    exit 0
}

# --- steady walk mode: for memory scans that need a long, clean walk -----
if ($Mode -eq "walk") {
    Stamp "steady walk mode for $Seconds s (W held, 1 s stop every 8 s)"
    Start-Sleep -Seconds $InitialIdleSeconds
    $t0 = Get-Date
    while (((Get-Date) - $t0).TotalSeconds -lt $Seconds) {
        Send-Key "w" $true
        Start-Sleep -Milliseconds 7000
        Send-Key "w" $false
        Start-Sleep -Milliseconds 1000
    }
    Send-Key "w" $false
    Stamp "done"
    exit 0
}

# --- crouch/air mode: the transitions the 2026-10-15 capture targets.  The
# --- script mode's long walks (6 s of W per cycle) drift the player into
# --- walls within a few cycles, and a wall-slide is exactly what the sim
# --- cannot reproduce (no collision: 55% of that trace's writes were
# --- environment-screened and the gate still diverged at the slides).
# --- Alternating W and S alone is not enough: the two jumps move the player
# --- ~18 m *forward* with no return, so the operator watched it pin against
# --- the map edge.  This cycle therefore turns the view ~180 degrees with an
# --- injected mouse drag (relative mouse_event motion *does* turn the view:
# --- 3000 px measured -143.5 deg) and walks back, which zeroes the drift.
# --- Both crouch transitions happen inside the walks, and both jump shapes
# --- every round, in alternating directions.
function MouseDragX([int]$px) {
    $step = if ($px -gt 0) { 25 } else { -25 }
    for ($i = 0; $i -lt [math]::Abs($px) / 25; $i++) {
        [Win]::mouse_event(0x0001, $step, 0, 0, [UIntPtr]::Zero)   # MOUSEEVENTF_MOVE
        Start-Sleep -Milliseconds 6
    }
}
if ($Mode -eq "ctrl") {
    Stamp "crouch/air mode for $Seconds s (W/S with crouch both ways, jumps, 180 deg turn-back)"
    Start-Sleep -Seconds $InitialIdleSeconds
    $t0 = Get-Date
    while (((Get-Date) - $t0).TotalSeconds -lt $Seconds) {
        Stamp "W 2.5 s (walk ramp + plateau)"
        Send-Key "w" $true
        Start-Sleep -Milliseconds 2500
        Stamp "+Ctrl 2.5 s (walk -> crouch), -Ctrl 2 s (crouch -> walk), -W"
        Send-Key "ctrl" $true
        Start-Sleep -Milliseconds 2500
        Send-Key "ctrl" $false
        Start-Sleep -Milliseconds 2000
        Send-Key "w" $false

        Stamp "S 2.5 s (backpedal: decel, reverse, back plateau)"
        Send-Key "s" $true
        Start-Sleep -Milliseconds 2500
        Send-Key "s" $false

        Stamp "idle 1 s, then moving jump: W, space, W 1.5 s"
        Start-Sleep -Milliseconds 1000
        Send-Key "w" $true
        Start-Sleep -Milliseconds 250
        Tap "space"
        Start-Sleep -Milliseconds 1500
        Send-Key "w" $false

        Stamp "idle 1 s, then standing jump and W pressed mid-air"
        Start-Sleep -Milliseconds 1000
        Tap "space"
        Start-Sleep -Milliseconds 200
        Send-Key "w" $true
        Start-Sleep -Milliseconds 1500
        Send-Key "w" $false
        Start-Sleep -Milliseconds 500

        # ~180 degrees: at the measured ~0.048 deg/px this is ~3760 px.
        Stamp "mouse turn ~180 deg, then W 7 s back (cancels the cycle's drift)"
        MouseDragX 3760
        Start-Sleep -Milliseconds 300
        Send-Key "w" $true
        Start-Sleep -Milliseconds 7000
        Send-Key "w" $false
        Start-Sleep -Milliseconds 700
    }
    Send-Key "w" $false; Send-Key "s" $false; Send-Key "ctrl" $false
    Stamp "done"
    exit 0
}

# --- the movement script ---------------------------------------------------
Stamp "idle $InitialIdleSeconds s (standing baseline)"
Start-Sleep -Seconds $InitialIdleSeconds

$t0 = Get-Date
while (((Get-Date) - $t0).TotalSeconds -lt $Seconds) {
    Stamp "W hold 6 s (walk: accel ramp at the start)"
    Hold @("w") 6

    Stamp "idle 2 s (decel ramp)"
    Start-Sleep -Seconds 2

    Stamp "W+D hold 5 s (diagonal composition)"
    Hold @("w", "d") 5

    Stamp "D hold 3 s (pure strafe)"
    Hold @("d") 3

    Stamp "S hold 4 s (backpedal, back-speed cap)"
    Hold @("s") 4

    Stamp "W hold 2 s, +Ctrl 3 s (walk -> crouch), -Ctrl 1.5 s (crouch -> walk), -W"
    Send-Key "w" $true
    Start-Sleep -Seconds 2
    Send-Key "ctrl" $true
    Start-Sleep -Seconds 3
    Send-Key "ctrl" $false
    Start-Sleep -Milliseconds 1500
    Send-Key "w" $false

    Stamp "Ctrl hold 1 s, then +W 2 s (crouch from standstill, crouch walk from rest), -Ctrl 1.5 s (crouch -> walk), -W"
    Send-Key "ctrl" $true
    Start-Sleep -Seconds 1
    Send-Key "w" $true
    Start-Sleep -Seconds 2
    Send-Key "ctrl" $false
    Start-Sleep -Milliseconds 1500
    Send-Key "w" $false

    Stamp "Space tap + W hold 3 s (jump while moving)"
    Send-Key "w" $true
    Tap "space"
    Start-Sleep -Milliseconds 2900
    Send-Key "w" $false

    Stamp "standing jump, then W pressed mid-air (air accel from zero)"
    Start-Sleep -Seconds 1
    Tap "space"
    Start-Sleep -Milliseconds 150
    Send-Key "w" $true
    Start-Sleep -Milliseconds 1500
    Send-Key "w" $false

    Stamp "W hold 4 s"
    Hold @("w") 4

    Stamp "idle 2 s"
    Start-Sleep -Seconds 2
}
Stamp "done"
