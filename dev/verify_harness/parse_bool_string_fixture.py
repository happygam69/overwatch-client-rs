"""Deterministic (text, len, flag) cases for `fn/ParseBoolString`.

The client's `ParseBoolString` (`0x1424d8800`) is a case-insensitive **prefix**
match: it walks the caller's (text, len) against eight true spellings
(`1 on enable enabled yes y true t`, at `0x143472f10`), then against eight false
spellings (`0 off disable disabled no n false f`, at `0x143472f50`), and on a
match writes the flag byte - 1 for the true table, 0 for the false one - and
returns 1. With no match it returns 0 and never touches the flag, so the caller
can tell "no answer" from "false".

Every edge the machine has is pinned here:

* the length check runs before the first byte, so `""` matches (flag 1);
* either side may run out - an input prefix (`"tr"`), a whole spelling inside a
  longer input (`"enablex"`), and a spelling that ends exactly on a NUL in the
  input (`"t\\0"`);
* `on` is tested before `off`, so `"o"` is true and `"of"` is false;
* nothing trims: `" true"` matches nothing and leaves the flag at its sentinel;
* eight mismatches move to the false table, sixteen to the `0` return.

The flag starts at `0xA5`, so "no match leaves it alone" is observable.
"""

from __future__ import annotations

from typing import List, Tuple

from emu_oracle import ArgSpec

TEXT_OFFSET = 0
FLAG_OFFSET = 0xE000
SENTINEL = 0xA5
NUL = bytes([0x00])

# (text, len, flag)
Case = Tuple[bytes, int, bytes]


def _flag(v: int = SENTINEL) -> bytes:
    return bytes([v])


def _cases() -> List[Case]:
    f = _flag()
    return [
        # The empty string: the length check precedes the first byte.
        (b"", 0, f),
        # Every true spelling, exactly.
        (b"1", 1, f),
        (b"on", 2, f),
        (b"enable", 6, f),
        (b"enabled", 7, f),
        (b"yes", 3, f),
        (b"y", 1, f),
        (b"true", 4, f),
        (b"t", 1, f),
        # Every false spelling, exactly.
        (b"0", 1, f),
        (b"off", 3, f),
        (b"disable", 7, f),
        (b"disabled", 8, f),
        (b"no", 2, f),
        (b"n", 1, f),
        (b"false", 5, f),
        (b"f", 1, f),
        # Case folding: A..Z only, nothing else.
        (b"TRUE", 4, f),
        (b"Yes", 3, f),
        (b"eNaBlEd", 7, f),
        (b"OFF", 3, f),
        (b"FaLsE", 5, f),
        (b"ENABLE", 6, f),
        # The table order: `on` is tried before `off`, `1` before `0`.
        (b"o", 1, f),
        (b"of", 2, f),
        (b"oF", 2, f),
        # Input prefixes: the input runs out first, so these match.
        (b"tr", 2, f),
        (b"enab", 4, f),
        (b"dis", 3, f),
        (b"di", 2, f),
        (b"n", 1, f),
        (b"fa", 2, f),
        # A spelling inside a longer text: the text's byte is compared with the
        # spelling's NUL, so nothing here matches (except `0x`/`1 ` - no).
        (b"enablex", 7, f),
        (b"true1", 5, f),
        (b"yesno", 5, f),
        (b"onward", 6, f),
        (b"disabled_for_real", 17, f),
        (b"false.", 6, f),
        (b"0x", 2, f),
        (b"1 ", 2, f),
        # A NUL inside the input: `t\\0` matches `t`, `tr\\0` matches nothing.
        (b"t" + NUL, 2, f),
        (b"tr" + NUL, 3, f),
        (b"n" + NUL, 2, f),
        # No trimming anywhere: a leading or trailing space is a mismatch.
        (b" true", 5, f),
        (b"true ", 5, f),
        (b" y", 2, f),
        # Nothing matches: eight true candidates and eight false ones fail.
        (b"x", 1, f),
        (b"maybe", 5, f),
        (b"2", 1, f),
        (b"yessir", 6, f),
        (b"disable d", 9, f),
        (b" ", 1, f),
        # High-bit bytes are not folded into ASCII letters.
        (bytes([0xD4]) + b"true", 5, f),
        (bytes([0x80]), 1, f),
        # A long input whose whole spelling is a prefix, and a spelling mixed
        # with a length that stops before the input's buffer end.
        (b"enabled_then_some_padding", 10, f),
        (b"false" + NUL + b"trailing", 5, f),
        (b"disable" + b"xyz", 7, f),
    ]


SPECIALS = _cases()


def _random(r: int) -> Case:
    """A generated case: a spelling with a random prefix/suffix or a
    case/punctuation mutation, so the walk is exercised away from the edges."""
    words = [b"1", b"on", b"enable", b"enabled", b"yes", b"y", b"true", b"t",
             b"0", b"off", b"disable", b"disabled", b"no", b"n", b"false", b"f",
             b"", b"x", b"maybe", b"truthy", b"o", b"of"]
    base = bytearray(words[r % len(words)])
    shape = (r // len(words)) % 6
    if shape == 1:
        # Prefix cut: an input that runs out mid-spelling.
        cut = (r * 5) % (len(base) + 1)
        base = base[:cut]
    elif shape == 2:
        # Toggle the case of one character.
        if base:
            i = r % len(base)
            base[i] = base[i] ^ 0x20 if base[i] >= 0x41 else base[i]
    elif shape == 3:
        base += bytes([[0x20], [0x2E], [0x31], [0x5F], [0x00]][r % 5])
    elif shape == 4:
        # A trailing byte that is not part of any spelling.
        base += bytes([[0x7A], [0x2D], [0x39]][r % 3])
    elif shape == 5 and base:
        base[r % len(base)] = [0x00, 0x20, 0xD4][r % 3]
    data = bytes(base)
    return (data, len(data), _flag())


def make_case(i: int) -> Case:
    if i < len(SPECIALS):
        return SPECIALS[i]
    return _random(i - len(SPECIALS))


def make_args(i: int) -> List[ArgSpec]:
    text, length, flag = make_case(i)
    return [
        ArgSpec(kind="ptr", mem=text, arena_offset=TEXT_OFFSET),
        ArgSpec(kind="u64", bits=length),
        ArgSpec(kind="ptr", mem=flag, arena_offset=FLAG_OFFSET),
    ]
