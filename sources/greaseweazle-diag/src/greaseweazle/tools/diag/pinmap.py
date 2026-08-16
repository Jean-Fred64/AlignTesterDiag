# greaseweazle/tools/diag/pinmap.py
#
# 34-pin floppy interface signals displayed by the interactive diagnostic.
#
# Based on the work of Keir Fraser
#
# This is free and unencumbered software released into the public domain.
# See the file COPYING for more details, or visit <http://unlicense.org>.

from typing import List, Tuple

# Density-select is an output we drive ourselves (no read-back pin exists
# in this firmware), so it's tracked in software rather than polled here.
#
# Pin 2 isn't always density-select: on 8-inch drives (and on 34-to-50-pin
# adapter cables, which almost always carry it through) the same pin is
# often wired as TG43 instead, asserted past a given cylinder to enable
# write precompensation. See the existing --gen-tg43 option in tools/read.py,
# write.py and align.py for the convention this codebase already follows
# (asserted from track 60 up). The 'd' key here just toggles the raw pin;
# what that means electrically depends on which signal your cable/drive
# actually wired to it.
DENSITY_SELECT_PIN = 2

# Matches the threshold already hardcoded in tools/read.py, write.py and
# align.py's --gen-tg43 option: pin 2 driven high below this cylinder, low
# from it upward.
TG43_TRACK_THRESHOLD = 60

# Input signals polled once per tick, in status-line display order.
# (label, pin, ambiguous)
#
# Pin numbers are the standard Shugart/IBM-PC 34-pin assignments and don't
# differ between the two cabling conventions for these particular signals.
# Pin 34's *meaning* does vary by drive family (Disk-Change on IBM/PC,
# often Ready on Shugart, remapped further on some drives such as Amiga) --
# hence 'ambiguous': show the raw level, not an asserted semantic.
TK0_PIN = 26

SIGNALS: List[Tuple[str, int, bool]] = [
    ('WP',  28, False),
    ('DC',  34, True),
    ('TK0', TK0_PIN, False),
]
