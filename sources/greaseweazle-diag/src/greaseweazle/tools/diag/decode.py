# greaseweazle/tools/diag/decode.py
#
# Per-tick flux decode for the interactive diagnostic: counts sectors whose
# IDAM cylinder matches the head's current (expected) cylinder versus
# sectors that decoded cleanly but came from a different cylinder.
#
# Based on the work of Keir Fraser
#
# This is free and unencumbered software released into the public domain.
# See the file COPYING for more details, or visit <http://unlicense.org>.

from collections import Counter
from typing import List, Tuple

# Importing codec.codec first forces its module-level imports (which load
# ibm.py in full) to run before we touch ibm.py directly -- ibm.py and
# codec.hp.hp_mmfm import each other, so a direct `from ...ibm import ...`
# as the first touch of this package raises a partial-init ImportError.
from greaseweazle.codec import codec  # noqa: F401
from greaseweazle.codec.ibm.ibm import IBMTrack, Mode
from greaseweazle.flux import Flux


def decode_tick(
        flux: Flux,
        cyl: int,
        head: int,
        mode: Mode,
        rate_kbps: int,
        time_per_rev: float
) -> Tuple[int, List[Tuple[int, int]]]:
    """Decode one tick's flux capture.

    Returns (sect, off_track): sect = cleanly-decoded sectors whose IDAM
    cylinder matches `cyl`. off_track = a list of (cyl, count) pairs, one
    per distinct cylinder actually found among the cleanly-decoded sectors
    that don't match `cyl` (head mistracking). Empty if none.

    A fresh IBMTrack is built every call: IBMTrack.decode_raw() appends to
    self.sectors across calls without ever clearing it, so reusing one
    instance across ticks would accumulate stale sectors from earlier
    reads/cylinders.
    """
    t = IBMTrack(cyl, head, mode)
    t.clock = 5e-4 / rate_kbps
    t.time_per_rev = time_per_rev
    t.decode_flux(flux)

    sect = sum(1 for s in t.sectors if s.crc == 0 and s.idam.c == cyl)
    wrong_cyls = Counter(s.idam.c for s in t.sectors
                         if s.crc == 0 and s.idam.c != cyl)
    off_track = sorted(wrong_cyls.items())
    return sect, off_track
