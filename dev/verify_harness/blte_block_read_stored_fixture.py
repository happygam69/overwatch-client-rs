"""Deterministic cases for `fn/BlteBlockReadStored`.

The client's stored block reader (`0x1424d7850`) is a streaming copy with a
one-byte state flag. Signature (MS x64):
`(state /*RCX*/, dst /*RDX*/, dst_len u32 /*R8D*/, src /*R9*/,
  src_len u32 /*stack5*/, out_decoded u32* /*stack6*/, out_consumed u32* /*stack7*/)
  -> u8 status`.

The state object is the client's 0x18-byte block-reader object: `+0x08` is the
declared decoded size (written by the initializer `0x1424d7840`, never read by
the reader), `+0x0c` is the running decoded total the reader advances, and
`+0x10` is the mode-consumed flag. The reader zeroes both out counters, returns
3 for a null `dst`/`src`, consumes the mode byte when the flag is non-zero
(status 0 for a `src_len` below 1, status 8 for a mode other than `N`), returns
0 for a zero `dst_len` or an exhausted `src`, and otherwise copies
`min(end - src, dst_len)` bytes, adding the length to both out counters and to
the state dword at `+0xc`.

The two out buffers start at a sentinel (`0xdeadbeef`) so the client's opening
zero-store is observable, and the state `+0x08` field starts at a sentinel so
an unexpected write to it is observable.

A null pointer cannot be expressed: the harness gives every `ptr` arg a
non-null arena address, so an empty buffer is a non-null pointer to an empty
region. The client's `test rdx,rdx` / `test r9,r9` status-3 path is therefore
an acknowledged gap; the empty-buffer cases below drive the length edges
(`dst_len == 0` and `src_len == 0`) that sit next to it.
"""

from __future__ import annotations

from typing import List, Tuple

from emu_oracle import ArgSpec

SENTINEL32 = b"\xef\xbe\xad\xde"

# (state, dst, dst_len, src, src_len)
Case = Tuple[bytes, bytes, int, bytes, int]


def _state(flag: int, total: int, declared: int) -> bytes:
    s = bytearray(0x18)
    # +0x08: the declared decoded size (never read by the reader).
    s[0x08:0x0C] = declared.to_bytes(4, "little")
    # +0x0c: the running decoded total.
    s[0x0C:0x10] = total.to_bytes(4, "little")
    # +0x10: the mode-consumed flag.
    s[0x10] = flag
    return bytes(s)


def _cases() -> List[Case]:
    cases: List[Case] = []
    # Empty dst buffer with dst_len 0: the mode byte is consumed, then the
    # zero dst_len returns 0.
    cases.append((_state(1, 0, 0), b"", 0, b"Nabc", 4))
    # Empty src buffer with src_len 0: the src_len<1 test returns 0 before
    # the mode byte.
    cases.append((_state(1, 0, 0), b"xxxx", 4, b"", 0))
    # Flag set, a mode other than 'N': status 8.
    cases.append((_state(1, 0, 0), b"xxxx", 4, b"Xabc", 4))
    cases.append((_state(1, 0, 0), b"xxxx", 4, b"Zabc", 4))
    # Flag set, mode 'N', zero dst_len: status 0.
    cases.append((_state(1, 0, 0), b"xxxx", 0, b"Nabc", 4))
    # Flag set, mode 'N', src exhausted by the mode byte: status 0.
    cases.append((_state(1, 0, 0), b"xxxx", 4, b"N", 1))
    # Flag set, mode 'N', a full copy.
    cases.append((_state(1, 0, 0), b"xxxxxxxx", 8, b"Nhello", 6))
    # Flag clear: no mode byte, the whole src is data.
    cases.append((_state(0, 0, 0), b"xxxxxxxx", 8, b"hello", 5))
    # dst_len below the remaining source: a partial copy.
    cases.append((_state(1, 0, 0), b"xxxxxxxx", 5, b"Nhello world", 12))
    # dst_len above the remaining source: the whole remainder.
    cases.append((_state(1, 0, 0), b"xxxxxxxxxxxxxxxx", 16, b"Nhi", 3))
    # A running total in the state dword.
    cases.append((_state(1, 100, 0), b"xxxxxxxx", 8, b"Nhello", 6))
    cases.append((_state(0, 0xFFFFFFFF, 0), b"xxxxxxxx", 8, b"hi", 2))
    # A declared size in +0x08 that must not be written.
    cases.append((_state(1, 0, 0x12345678), b"xxxxxxxx", 8, b"Nhello", 6))
    # One-byte and two-byte payloads.
    cases.append((_state(1, 0, 0), b"xxxxxxxx", 8, b"Na", 2))
    cases.append((_state(1, 0, 0), b"xxxxxxxx", 8, b"Nab", 3))
    # A dst buffer longer than dst_len.
    cases.append((_state(1, 0, 0), b"xxxxxxxxxxxxxxxx", 4, b"Nhello", 6))
    return cases


SPECIALS = _cases()


def _random(r: int) -> Case:
    flag = r & 1
    total = (r * 37) & 0xFFFFFFFF
    declared = (r * 91) & 0xFFFFFFFF
    src_len = 1 + (r % 24)
    body = bytes(((r * 13 + j * 7) & 0xFF) or 0x41 for j in range(src_len))
    src = (b"N" if flag else b"") + body
    dst_len = r % 20
    dst = bytes(((r * 17 + j) & 0xFF) or 0x42 for j in range(max(dst_len, 8)))
    return (_state(flag, total, declared), dst, dst_len, src, len(src))


def make_case(i: int) -> Case:
    if i < len(SPECIALS):
        return SPECIALS[i]
    return _random(i - len(SPECIALS))


def make_args(i: int) -> List[ArgSpec]:
    state, dst, dst_len, src, src_len = make_case(i)
    return [
        ArgSpec(kind="ptr", mem=state),
        ArgSpec(kind="ptr", mem=dst),
        ArgSpec(kind="u64", bits=dst_len),
        ArgSpec(kind="ptr", mem=src),
        ArgSpec(kind="u64", bits=src_len),
        ArgSpec(kind="ptr", mem=SENTINEL32),
        ArgSpec(kind="ptr", mem=SENTINEL32),
    ]
