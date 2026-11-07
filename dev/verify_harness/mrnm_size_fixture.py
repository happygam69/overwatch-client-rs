"""`MrnmRuntimeSize` (`0x141b89390`) inputs: the same synthetic payloads the
load uses, one pointer and nothing else. See `mrnm_fixture.build`."""

from __future__ import annotations

import mrnm_fixture


def make_args(i: int):
    return mrnm_fixture.size_args(i)
