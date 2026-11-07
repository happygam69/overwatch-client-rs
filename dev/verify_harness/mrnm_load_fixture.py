"""`MrnmHandler` (`0x141b88f10`) inputs: payload (with its relocation slots
declared), a model object carrying the `+0xaa` default byte, a zeroed
destination, and the two arena bases in the client's dead argument slots.
See `mrnm_fixture.build`."""

from __future__ import annotations

import mrnm_fixture


def make_args(i: int):
    return mrnm_fixture.load_args(i)
