"""Filter the stub table by target ranges.

Usage: python dev/stub_filter.py <stubs.csv> [--r8 LO,HI] [--rdx LO,HI]
       [--rcx LO,HI] [--stub LO,HI]
Prints matching rows (stub,r8,rdx,rcx) and counts.
"""
import sys

csv_path = sys.argv[1]
filters = {}
i = 2
while i < len(sys.argv):
    key = sys.argv[i].lstrip("-")
    lo, hi = sys.argv[i + 1], sys.argv[i + 2]
    filters[key] = (int(lo, 16), int(hi, 16))
    i += 3

rows = []
with open(csv_path, "rb") as fh:
    raw = fh.read()
if raw[:2] in (b"\xff\xfe", b"\xfe\xff"):
    text = raw.decode("utf-16")
else:
    text = raw.decode("utf-8", "replace")
for line in text.splitlines():
    line = line.strip()
    if not line or line.startswith(";"):
        continue
    stub, r8, rdx, rcx = line.split(",")
    rec = {"stub": int(stub, 16), "r8": int(r8, 16),
           "rdx": int(rdx, 16), "rcx": int(rcx, 16)}
    ok = True
    for key, (lo, hi) in filters.items():
        if not (lo <= rec[key] <= hi):
            ok = False
            break
    if ok:
        rows.append(rec)

for r in rows[:40]:
    print(f"stub {r['stub']:#x} r8 {r['r8']:#x} rdx {r['rdx']:#x} "
          f"rcx {r['rcx']:#x}")
print(f"; matches: {len(rows)}")
