"""Offline Unicorn emulation of the loader's TLS callback chain (current
build). Fully offline: maps the plaintext loader from the disk flatten, forges
a minimal PEB/TEB, and watches which engine blocks run, what they read from
the .eid/.data, and what they write into the fake main-module image.

Recon run: nothing is stubbed semantically - every OS-API thunk call lands in
a ret-stub page and gets logged.

Usage: python dev/emu_tls.py [--steps N]
"""
import struct
import sys

from unicorn import (UC_ARCH_X86, UC_MODE_64, UC_HOOK_CODE, UC_HOOK_MEM_WRITE,
                     UC_HOOK_MEM_READ, UC_HOOK_MEM_UNMAPPED, Uc, UcError)
from unicorn.arm64_const import *  # noqa: F401,F403  (import cost check)
import unicorn.x86_const as x

LOADER_BASE = 0x180000000
LOADER_SIZE = 0x2410000
MAIN_BASE = 0x140000000
MAIN_SIZE = 0x24000000          # 576 MB fake main-module arena? keep 64 MiB
MAIN_SIZE = 0x4000000
STACK_BASE = 0x7FF000000000
STACK_SIZE = 0x100000
SLOT_BASE = 0x2A1A00000000      # fake heap-thunk page for IAT values
SENTINEL = 0x7F0000F0000        # return-trap page full of 'ret'
TEB_BASE = 0x7F000010000
PEB_BASE = 0x7F000020000
LDR_BASE = 0x7F000030000
ENTRY_NODE = 0x7F000031000
TAG_RET = (LOADER_BASE + 0x7F0000000)

log = []


def note(s):
    log.append(s)


def build_teb_peb(u):
    # gg (thread env block): x64: gs:[0x30]=TEB? no - x64 TEB base = gs itself,
    # PEB pointer at +0x60
    u.mem_map(TEB_BASE, 0x1000)
    u.mem_map(PEB_BASE, 0x1000)
    u.mem_map(LDR_BASE, 0x10000)
    u.mem_write(TEB_BASE + 0x60, struct.pack("<Q", PEB_BASE))
    # PEB minimal: ImageBase +0x10, Ldr +0x18
    u.mem_write(PEB_BASE + 0x10, struct.pack("<Q", LOADER_BASE))
    u.mem_write(PEB_BASE + 0x18, struct.pack("<Q", LDR_BASE))
    # LDR_DATA_TABLE_ENTRY: InLoadOrderLinks at +0; DllBase +0x30
    # list head -> node self-terminating
    head = LDR_BASE + 0x10
    node0 = ENTRY_NODE
    node1 = ENTRY_NODE + 0x80
    entry = struct.pack("<QQ", node1, head)          # Flink/BLINK node0
    entry += struct.pack("<Q", head)                 # Blink node0 -> head
    entry += struct.pack("<QQ", head, node0)
    u.mem_write(node0, entry)
    # node: DllBase +0x30 = loader base, then name UNICODE_STRING +0x60
    u.mem_write(node0 + 0x30, struct.pack("<Q", LOADER_BASE))
    name0 = b"L\x00o\x00a\x00d\x00e\x00r\x00"
    u.mem_write(node0 + 0x60, struct.pack("<IIQ", len(name0), len(name0),
                                          node0 + 0x70))
    u.mem_write(node0 + 0x70, name0)
    # node1: main module
    entry1 = struct.pack("<QQ", head, node0)         # node1 flink=head? whatever
    entry1 += struct.pack("<QQ", head, node1)
    u.mem_write(node1, entry1)
    u.mem_write(node1 + 0x30, struct.pack("<Q", MAIN_BASE))
    name1 = b"S\x00h\x00o\x00o\x00t\x00e\x00r\x00"
    u.mem_write(node1 + 0x60, struct.pack("<IIQ", len(name1), len(name1),
                                          node1 + 0x70))
    u.mem_write(node1 + 0x70, name1)


def map_image(u, flat_path, base):
    data = open(flat_path, "rb").read()
    size = (len(data) + 0xFFF) & ~0xFFF
    u.mem_map(base, size)
    u.mem_write(base, data)
    return len(data)


def main():
    steps = 20_000_000
    if "--steps" in sys.argv:
        steps = int(sys.argv[sys.argv.index("--steps") + 1])
    flatpath = r"D:\ow-dumps\loader_disk_rva.bin"
    if "--flat" in sys.argv:
        flatpath = sys.argv[sys.argv.index("--flat") + 1]
    u = Uc(UC_ARCH_X86, UC_MODE_64)

    n = map_image(u, flatpath, LOADER_BASE)
    note(f"mapped loader {n:#x} bytes at {LOADER_BASE:#x}")
    # main module ciphertext: use the real current exe sections
    exe = open(r"E:\Overwatch\_retail_\Overwatch.exe", "rb").read()
    pe = struct.unpack_from("<I", exe, 0x3C)[0]
    nsec = struct.unpack_from("<H", exe, pe + 6)[0]
    opt = pe + 0x18
    img_base = struct.unpack_from("<Q", exe, opt + 0x18)[0]
    sec_off = opt + struct.unpack_from("<H", exe, pe + 20)[0]
    secs = []
    for i in range(nsec):
        s = sec_off + i * 40
        name = exe[s:s + 8].rstrip(b"\0").decode(errors="replace")
        vsz, va, rsz, ro = struct.unpack_from("<IIII", exe, s + 8)
        secs.append((name, va, vsz, ro, rsz))
    note(f"main exe: image base runtime-shown {img_base:#x}; sections "
         f"{[(nm, hex(va), hex(vsz)) for nm, va, vsz, _, _ in secs]}")
    image_end = max((va + vsz) for _, va, vsz, _, _ in secs)
    MAIN_SIZE = (image_end + 0xFFF) & ~0xFFF
    u.mem_map(MAIN_BASE, MAIN_SIZE)
    note(f"main image size {MAIN_SIZE:#x}")
    for name, va, vsz, ro, rsz in secs:
        if rsz:
            blob = exe[ro:ro + min(rsz, vsz)]
            u.mem_write(MAIN_BASE + va, blob)
    note("main ciphertext mapped with sections")

    u.mem_map(STACK_BASE, STACK_SIZE)
    u.mem_map(SLOT_BASE, 0x100000)             # fake heap-thunk page = ret stubs
    u.mem_write(SLOT_BASE, b"\xC3" * 0x1000)
    u.mem_map(SENTINEL, 0x1000)
    u.mem_write(SENTINEL, b"\xC3" * 0x1000)

    build_teb_peb(u)
    u.reg_write(x.UC_X86_REG_FS_BASE, TEB_BASE)   # FS base
    u.reg_write(x.UC_X86_REG_GS_BASE, TEB_BASE)   # GS base

    IAT_LO, IAT_HI = LOADER_BASE + 0x1DCDD58, LOADER_BASE + 0x1DCDEB8

    hist = {}
    hist_reads = {}
    sites = {}

    def bump(hst, addr):
        key = (addr - LOADER_BASE) >> 12 if LOADER_BASE <= addr < \
            LOADER_BASE + 0x2410000 else addr
        hst[key] = hst.get(key, 0) + 1

    hook_state = {"n": 0, "samples": []}

    def hook_code(uu, addr, size, ud):
        st = hook_state
        st["n"] += 1
        if st["n"] % 200000 == 0:
            st["samples"].append((st["n"], addr))
            note(f"@{st['n']} rip {addr:#x}")
        if SLOT_BASE <= addr < SLOT_BASE + 0x1000 or \
                0x2A1A00000000 <= addr < 0x2A1A00001000:
            r = [uu.reg_read(rr) for rr in
                 (x.UC_X86_REG_RCX, x.UC_X86_REG_RDX, x.UC_X86_REG_R8, x.UC_X86_REG_R9)]
            sp = uu.reg_read(x.UC_X86_REG_RSP) + 8
            note(f"THUNK call at {addr:#x} args "
                 f"{[hex(a) for a in r]} [rsp+..]{hex(sp)}")
        elif MAIN_BASE <= addr < MAIN_BASE + MAIN_SIZE:
            note(f"MAIN-EXEC at {addr:#x}")
        elif LOADER_BASE + 0x1F80000 <= addr < LOADER_BASE + 0x2410000:
            note(f"EID-EXEC at {addr:#x}")

    def hook_write(uu, access, address, size, value, ud):
        if LOADER_BASE + 0x1DC00000 <= address < LOADER_BASE + 0x1E50000:
            note(f"DATA-WRITE {address - LOADER_BASE:#x} size {size} "
                 f"val {value:#x}")
        elif MAIN_BASE <= address < MAIN_BASE + MAIN_SIZE:
            note(f"MAIN-WRITE {address - MAIN_BASE:#x} size {size} "
                 f"val {value:#x}")

    def hook_unmapped(uu, access, address, size, value, ud):
        note(f"UNMAPPED access {access} at {address:#x} sz{size} "
             f"rip {uu.reg_read(x.UC_X86_REG_RIP):#x}")
        uu.mem_map(address & ~0xFFF, 0x1000)
        return True

    readlog = {}

    def hook_read(uu, access, address, size, value, ud):
        bump(hist_reads, address)
        pg = (address - LOADER_BASE) >> 12
        if pg in (0x1e99, 0x1e86, 0x1e52, 0x1e1c, 0x1df0, 0x1e79):
            r = uu.reg_read(x.UC_X86_REG_RIP)
            sites[r] = sites.get(r, 0) + 1
            key = r - LOADER_BASE
            if key in (0x1a7e12e, 0x1a7e140, 0x1a7e2f4, 0x1a7e80d, 0x1a7e53c):
                if readlog.get(key, 0) < 6:
                    readlog[key] = readlog.get(key, 0) + 1
                    v = uu.mem_read(address, size)
                    note(f"READ@rva {key:#x} addr {address - LOADER_BASE:#x} "
                         f"sz{size} val {v.hex(' ')}")

    def hook_w(uu, access, address, size, value, ud):
        bump(hist, address)
        if LOADER_BASE + 0x1DC00000 <= address < LOADER_BASE + 0x1E50000:
            note(f"DATA-WRITE {address - LOADER_BASE:#x} size {size} "
                 f"val {value:#x}")
        elif MAIN_BASE <= address < MAIN_BASE + MAIN_SIZE:
            note(f"MAIN-WRITE {address - MAIN_BASE:#x} size {size} "
                 f"val {value:#x}")

    u.hook_add(UC_HOOK_CODE, hook_code)
    u.hook_add(UC_HOOK_MEM_WRITE, hook_w,
               begin=LOADER_BASE, end=LOADER_BASE + 0x2410000)
    u.hook_add(UC_HOOK_MEM_READ, hook_read,
               begin=LOADER_BASE, end=LOADER_BASE + 0x2410000)
    u.hook_add(UC_HOOK_MEM_UNMAPPED, hook_unmapped)

    # context: TLS callback (hinst, reason=1, reserved)
    rsp = STACK_BASE + 0x80000
    u.mem_write(rsp, struct.pack("<Q", SENTINEL))
    u.reg_write(x.UC_X86_REG_RSP, rsp)
    u.reg_write(x.UC_X86_REG_RCX, LOADER_BASE)
    u.reg_write(x.UC_X86_REG_RDX, 1)
    u.reg_write(x.UC_X86_REG_R8, 0)
    for r in (x.UC_X86_REG_RBP, x.UC_X86_REG_RBX, x.UC_X86_REG_RDI, x.UC_X86_REG_RSI):
        u.reg_write(r, 0)
    note("start TLS cb[0]")

    start_rva = int(sys.argv[sys.argv.index("--start") + 1], 16) \
        if "--start" in sys.argv else 0x48FFD0
    reason = 1
    note(f"start entry rva {start_rva:#x}")
    rsp = STACK_BASE + 0x80000
    u.mem_write(rsp, struct.pack("<Q", SENTINEL))
    u.reg_write(x.UC_X86_REG_RSP, rsp)
    u.reg_write(x.UC_X86_REG_RCX, LOADER_BASE)
    u.reg_write(x.UC_X86_REG_RDX, reason)
    u.reg_write(x.UC_X86_REG_R8, 0)
    for r in (x.UC_X86_REG_RBP, x.UC_X86_REG_RBX, x.UC_X86_REG_RDI,
              x.UC_X86_REG_RSI):
        u.reg_write(r, 0)

    try:
        u.emu_start(LOADER_BASE + start_rva, SENTINEL, timeout=120_000_000,
                    count=steps)
        note(f"emu returned after {hook_state['n']} instructions; "
             f"last rip {u.reg_read(x.UC_X86_REG_RIP):#x}")
        rregs = {}
        for rn, rr in (("rax", x.UC_X86_REG_RAX), ("rbx", x.UC_X86_REG_RBX),
                       ("rcx", x.UC_X86_REG_RCX), ("rdx", x.UC_X86_REG_RDX),
                       ("rsi", x.UC_X86_REG_RSI), ("rdi", x.UC_X86_REG_RDI),
                       ("r8", x.UC_X86_REG_R8), ("r9", x.UC_X86_REG_R9),
                       ("r10", x.UC_X86_REG_R10), ("r11", x.UC_X86_REG_R11),
                       ("r12", x.UC_X86_REG_R12), ("r13", x.UC_X86_REG_R13),
                       ("r14", x.UC_X86_REG_R14), ("r15", x.UC_X86_REG_R15),
                       ("rbp", x.UC_X86_REG_RBP), ("rsp", x.UC_X86_REG_RSP)):
            rregs[rn] = u.reg_read(rr)
        note("final regs: " + " ".join(f"{k}={v:#x}" for k, v in
                                       rregs.items()))
        sp = rregs["rsp"]
        stk = u.mem_read(sp, 0x200)
        note("stack@rsp: " + stk.hex(" "))
        full_stack = u.mem_read(STACK_BASE, 0x100000)
        with open(r"D:\ow-dumps\emu_stack.bin", "wb") as sf:
            sf.write(full_stack)
        xmm = []
        for i in range(16):
            lo = u.reg_read(x.UC_X86_REG_XMM0 + i * 2)
            hi = u.reg_read(x.UC_X86_REG_XMM0 + i * 2 + 1)
            xmm.append(struct.pack("<QQ", lo, hi))
        note("xmm16: " + "".join(v.hex() for v in xmm))
        note("STACK-DUMP written emu_stack.bin")
    except UcError as e:
        rip = u.reg_read(x.UC_X86_REG_RIP)
        note(f"UC ERROR at rip {rip:#x}: {e}")
        code = u.mem_read(rip, 16)
        note("bytes: " + code.hex(" "))

    with open(r"D:\ow-dumps\emu_tls_log1.txt", "w") as f:
        f.write("\n".join(log))
    print("\n".join(log[-40:]))
    print(f"... {len(log)} log lines total")
    print("== read-page histogram (top 24) ==")
    for k, v in sorted(hist_reads.items(), key=lambda kv: -kv[1])[:24]:
        print(f"  {k:#x}: {v}")
    print("== write-page histogram ==", hist)
    print("== top read-sites (rip) ==")
    for k, v in sorted(sites.items(), key=lambda kv: -kv[1])[:20]:
        print(f"  site rva {k - LOADER_BASE:#x}: {v}")


if __name__ == "__main__":
    main()




