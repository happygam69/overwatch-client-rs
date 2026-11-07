"""`MrnmFixup` (`0x141b893e0`) inputs: a structurally valid MRNM payload with
relative pointer slots and the payload's own address as the dead second
argument. See `mrnm_fixture.build` for the layout."""

from __future__ import annotations

import mrnm_fixture


def make_args(i: int):
    return mrnm_fixture.fixup_args(i)
