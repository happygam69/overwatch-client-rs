"""Which function collects a given object offset and hands it to a helper.

    python dev/pool_lea_args.py 0x108 0x118 0xf8 0xb0

A displacement scan cannot tell one object's `+0x108` from another's, but the
*shape* of an array-append call can: the caller materialises the array struct
address (`lea rcx,[obj+off]`) and then calls the helper. This lists every
`lea reg,[base+off]` with a non-stack base for the requested offsets, together
with the next call/indirect call in the same basic block, so a candidate helper
can be identified and then read. It is how the two collision-entry append
helpers (`0x142136010`, `0x1421360a0`) and their arguments were pinned; see
`docs/formats/shape-ingest.md`.

Read-only against the offline image; writes nothing.
"""
import sys

sys.path.insert(0, "dev")
import importlib.util

import capstone

spec = importlib.util.spec_from_file_location("ps", "dev/pool_scan.py")
ps = importlib.util.module_from_spec(spec)
spec.loader.exec_module(ps)
ps.load()

BASE = ps.BASE
offs = {int(a, 0) for a in sys.argv[1:]}

hits = []
for start, end in ps.RUNS:
    try:
        after = []
        for ins in ps.decode_function(start, end):
            op = ps.mem_operand(ins)
            if (
                ins.mnemonic == "lea"
                and op is not None
                and op.mem.index == 0
                and op.mem.disp in offs
            ):
                base = ps.mem_base_name(op)
                if base and base not in ps.STACK:
                    hits.append((ins.address, start, ins.op_str))
    except capstone.CapstoneError:
        continue

print(len(hits), "lea sites for", sorted(hex(o) for o in offs))
for address, start, text in hits:
    # The next four instructions decide what the address feeds.
    code = ps.DATA[address - BASE : address - BASE + 0x30]
    follow = []
    for ins in ps.MD.disasm(code, address):
        if ins.address == address:
            continue
        follow.append(ins.mnemonic + " " + ins.op_str)
        if ins.mnemonic in ("call", "jmp") or len(follow) >= 3:
            break
    print("%x  in %x  %-28s -> %s" % (address, BASE + start, text, " ; ".join(follow)))
