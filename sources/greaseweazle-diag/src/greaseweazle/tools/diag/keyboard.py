# greaseweazle/tools/diag/keyboard.py
#
# Unbuffered, non-echoing keyboard input for the interactive diagnostic,
# on both Windows and POSIX (Linux/macOS).
#
# Based on the work of Keir Fraser
#
# This is free and unencumbered software released into the public domain.
# See the file COPYING for more details, or visit <http://unlicense.org>.

import sys
from typing import Any, List, Optional

from greaseweazle import error

# Key names produced by read_key(): 'up', 'down', 'left', 'right', 'enter',
# 'esc', 'backspace'. Anything else is the literal character typed, as a
# one-character string. None means a key we have no name for.

# The platform split is written as `sys.platform == 'win32'` rather than the
# `os.name == 'nt'` used elsewhere in the tree because that is the form mypy
# narrows on: it type-checks only the branch matching the platform it runs on
# and treats the other as unreachable. Without that, checking on Linux trips
# over msvcrt (whose typeshed stub is empty off Windows) and checking on
# Windows trips over termios.

if sys.platform == 'win32':

    import msvcrt

    # Windows reports arrow keys as two bytes: a 0x00/0xe0 lead-in followed
    # by a scan code.
    _ARROWS = {b'H': 'up', b'P': 'down', b'K': 'left', b'M': 'right'}

    class _Impl:

        def open(self) -> None:
            pass

        def close(self) -> None:
            pass

        def kbhit(self) -> bool:
            return bool(msvcrt.kbhit())

        def read_key(self) -> Optional[str]:
            ch = msvcrt.getch()
            if ch in (b'\x00', b'\xe0'):
                return _ARROWS.get(msvcrt.getch())
            if ch in (b'\r', b'\n'):
                return 'enter'
            if ch == b'\x1b':
                return 'esc'
            if ch == b'\x08':
                return 'backspace'
            try:
                return ch.decode('ascii')
            except UnicodeDecodeError:
                return None

else:

    import os, select, termios, tty

    # POSIX terminals report arrow keys as ANSI escape sequences, with the
    # final byte selecting the key. Both the normal (CSI, "ESC [") and
    # application-cursor (SS3, "ESC O") forms are seen in the wild -- some
    # terminals switch to the latter under a full-screen application, so
    # accept either.
    _ARROWS = {'A': 'up', 'B': 'down', 'C': 'right', 'D': 'left'}

    # How long to wait for the rest of an escape sequence after a bare ESC
    # byte before deciding the user really did press Escape (which quits).
    # Sequences arrive in a single burst, so this only has to beat terminal
    # latency; a human cannot press ESC and '[' within it by hand.
    _ESC_TIMEOUT = 0.05

    class _Impl:
        """Puts the terminal in cbreak mode for the life of the session.

        cbreak rather than raw on purpose: it clears ECHO and ICANON (so keys
        arrive one at a time and aren't echoed into the middle of the status
        output) while leaving OPOST alone, so ordinary print() newlines still
        become CRLF and the scrolling status lines stay aligned at column
        zero. It also leaves ISIG alone on current Python versions, so Ctrl-C
        still works as an escape hatch alongside q/Esc.
        """

        def __init__(self) -> None:
            self.fd = -1
            self.saved: Optional[List[Any]] = None

        def open(self) -> None:
            error.check(sys.stdin.isatty(),
                        'gw diag needs an interactive terminal for keyboard '
                        'input (stdin is not a tty -- it is redirected or '
                        'piped)')
            self.fd = sys.stdin.fileno()
            self.saved = termios.tcgetattr(self.fd)
            tty.setcbreak(self.fd)

        def close(self) -> None:
            # Always restore, even on an exception: leaving the terminal in
            # cbreak mode hands the user an un-echoed shell afterwards.
            if self.saved is not None:
                termios.tcsetattr(self.fd, termios.TCSADRAIN, self.saved)
                self.saved = None

        def kbhit(self) -> bool:
            return bool(select.select([self.fd], [], [], 0)[0])

        def _getch(self, timeout: Optional[float] = None) -> Optional[str]:
            """One byte as a str, or None if `timeout` expires first."""
            if timeout is not None and not select.select(
                    [self.fd], [], [], timeout)[0]:
                return None
            ch = os.read(self.fd, 1)
            if not ch:
                return None
            try:
                return ch.decode('ascii')
            except UnicodeDecodeError:
                return None

        def read_key(self) -> Optional[str]:
            ch = self._getch()
            if ch is None:
                return None
            if ch in ('\r', '\n'):
                return 'enter'
            if ch in ('\x7f', '\x08'):
                return 'backspace'
            if ch != '\x1b':
                return ch
            return self._read_escape()

        def _read_escape(self) -> Optional[str]:
            """Decode what follows a lone ESC byte.

            A bare Escape means quit, so only treat it as the start of a
            sequence if more bytes turn up promptly. Whatever does turn up is
            consumed to the end of the sequence even when we have no name for
            that key: an unconsumed tail would otherwise be read as separate
            keystrokes, and the leftover digits of, say, Page-Up (ESC [ 5 ~)
            would step the head to track 50.
            """
            ch = self._getch(_ESC_TIMEOUT)
            if ch is None:
                return 'esc'
            if ch == 'O':  # SS3: exactly one more byte, e.g. ESC O A = up
                return _ARROWS.get(self._getch(_ESC_TIMEOUT) or '')
            if ch != '[':  # Alt-<key> and friends -- not a key we act on
                return None
            # CSI: parameter and intermediate bytes, terminated by a final
            # byte in the range 0x40-0x7e.
            while True:
                ch = self._getch(_ESC_TIMEOUT)
                if ch is None:
                    return None
                if '\x40' <= ch <= '\x7e':
                    return _ARROWS.get(ch)


class Keyboard:
    """Context manager wrapping the platform key reader.

    Use as `with Keyboard() as kb:`, then poll kb.kbhit() and kb.read_key().
    """

    def __init__(self) -> None:
        self.impl = _Impl()

    def __enter__(self) -> 'Keyboard':
        self.impl.open()
        return self

    def __exit__(self, *exc: Any) -> None:
        self.impl.close()

    def kbhit(self) -> bool:
        """True if a keystroke is waiting to be read."""
        return self.impl.kbhit()

    def read_key(self) -> Optional[str]:
        """The pending keystroke, or None for a key we don't name."""
        return self.impl.read_key()


# Local variables:
# python-indent: 4
# End:
