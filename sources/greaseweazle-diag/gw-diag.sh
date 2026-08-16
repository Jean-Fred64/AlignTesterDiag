#!/usr/bin/env bash
#
# POSIX counterpart of gw-diag.bat: run the gw diag tool straight from the
# source tree this script lives in, with no build or install step.

set -u

# This script lives in the root of the greaseweazle fork and runs the gw diag
# tool straight from the source tree next to it.
ROOT=$(cd -- "$(dirname -- "${BASH_SOURCE[0]}")" && pwd)

# Always show which commit this copy is at, first thing, before anything else
# can go wrong or print help and exit -- so a copied/synced tree never leaves
# you guessing which version you're actually running.
if command -v git >/dev/null 2>&1; then
    banner=$(git -C "$ROOT" log -1 --format='%h %ci %s' 2>/dev/null)
    if [ -n "$banner" ]; then
        echo "gw-diag: $banner"
    else
        echo "gw-diag: (not a git checkout -- can't identify commit)"
    fi
else
    echo "gw-diag: (git not on PATH -- can't identify commit)"
fi

usage() {
    cat <<'EOF'

gw-diag.sh - run the gw diag tool straight from source, no build or install
             needed. Handy on machines without a C compiler set up, since a
             full pip install builds an optional speed-up extension. Just
             clone the fork and run this file.

Needs Python 3.8 or newer. The required Python packages (crcmod,
bitarray>=3, pyserial, requests) are installed automatically on first run,
into a virtual environment beside this script if your distribution manages
its system Python.

USAGE:
  ./gw-diag.sh --rate KBPS [other options]

PARAMETERS:
  --device NAME    Serial port name, such as /dev/ttyACM0. Optional, only
                    needed if the Greaseweazle isn't auto-detected or you
                    have more than one plugged in.
  --drive ID       Which physical drive to use. A or B for an IBM/PC
                    cable, 0-3 for a Shugart cable. Default: A.
  --cyls N         Number of cylinders the drive has. Default: 84. You
                    can still step past this to probe a drive's real
                    mechanical limit. It's just the default range.
  --heads N        Number of heads/sides: 1 or 2. Default: 2.
  --double-step    Step two physical cylinders per logical track, for
                    an 80-track drive reading a 40-track disk.
  --step-delay N   Step Delay in microseconds for this session only
                    (same units as "gw delays --step"). Optional --
                    without it, diag preserves whatever "gw delays"
                    already has set, instead of reverting to the
                    firmware default the way it used to.
  --encoding TYPE  mfm or fm. Default: mfm. Almost everything from the
                    PC, Amiga and Atari ST era is mfm. Old 8-inch or
                    single-density disks are fm.
  --rate KBPS      Data rate in kilobits per second. Required. Common
                    values: 250 for double density, 500 for high
                    density. Rate alone doesn't fully determine the
                    format (500kbps is both 1.2MB 5.25" HD and 1.44MB
                    3.5" HD, at different rpm), so this is the one
                    value you must always supply yourself.
  --secs N         Sectors expected per track. Optional: left out, the
                    tool guesses it from --rate and the measured rpm
                    for standard formats. Set it yourself for a
                    nonstandard or copy-protected disk.
  --rpm N          Force a fixed spindle speed instead of using the
                    live measurement. Optional, normally leave this
                    out.
  --gen-tg43       Auto-drive pin 2 as a TG43 signal for 8-inch
                    drives: high below track 60, low from track 60
                    up (matches --gen-tg43 in gw read/write/align).
                    Updated automatically on every seek. Disables
                    the d key while active, since pin 2 is then
                    under automatic control, not manual toggle.

OUTPUT LINE FORMAT:
  Drive A: T0, H0, RPM 297.53, S9/9, OT NO, SEL:ON, MOT:ON,
  WP:H Unprot, TK0:L ON, DEN 2:L, DC34:?

  Drive       Drive ID you selected.
  T           Current track/cylinder number.
  H           Current head/side.
  RPM         Measured spindle speed this update. Green if within 5rpm
              of a standard speed (300 or 360), red otherwise, only
              once there's an actual reading. Shows ERR if no disk or
              index signal was found, or "off" if you turned the
              motor off with the m key.
  S           Sectors read cleanly from the current track, out of how
              many were expected (from --secs, or the guess).
  OT          Off-track sectors: sectors that decoded fine but whose
              header said a different track than where the head
              actually is. Shows NO if there aren't any. Otherwise
              shows which track(s) they actually came from, as
              T<cyl>/S<count>, for example T11/S2 means 2 sectors read
              cleanly but tagged as track 11 while the head is on a
              different track. More than one wrong track in the same
              read shows as a comma list, such as T11/S2,T12/S1. Any
              non-NO value usually means the head is mistracking or
              stepping to the wrong place.
  SEL         Drive-select line: ON while selected, OFF after the s
              key deselects it. Independent of MOT -- some drives
              gate their head load/unload solenoid off drive-select
              rather than motor-on, so this lets you test that
              without also stopping the spindle.
  MOT         Motor-on line: ON while the motor is running, OFF
              after the m key turns it off.
  WP          Write-protect pin (28), shown as the raw H/L level plus
              whether that means Prot or Unprot on this drive.
  TK0         Track-0 sensor pin (26): raw H/L level plus ON when the
              head is at track 0 (L), OFF otherwise (H).
  DEN 2       Density-select output pin (2) and the level you've set
              it to with the d key. Note: pin 2 isn't always density
              select. On 8-inch drives, and on most 34-to-50-pin
              adapter cables, the same pin is commonly wired as TG43
              instead (write precompensation enable past a given
              cylinder). The d key just toggles the raw pin either
              way. Use --gen-tg43 instead if you want it driven
              automatically by cylinder rather than by hand.
  DC34        Disk-change/ready pin (34). Shown as ? if your
              Greaseweazle can't read this particular pin back.

The tool itself prints a key legend when it starts (step, jump, head,
recalibrate, motor, drive-select, density, quit).

If the Greaseweazle is found but can't be opened (a permission error on
/dev/tty*), install the udev rules shipped with this repo, from the folder
this script is in:
  sudo cp scripts/49-greaseweazle.rules /etc/udev/rules.d/
then unplug and reconnect the device.

EOF
    exit 1
}

case "${1-}" in
    ''|-h|--help|-\?) usage ;;
esac

if [ ! -f "$ROOT/scripts/win/gw.py" ]; then
    echo "ERROR: gw-diag.sh must sit in the root of the greaseweazle fork," >&2
    echo "next to the scripts/ and src/ folders. \"$ROOT/scripts/win/gw.py\"" >&2
    echo "was not found." >&2
    exit 1
fi

# Pick an interpreter: python3 by preference, python only if it is really 3.x.
PY=""
for candidate in python3 python; do
    if command -v "$candidate" >/dev/null 2>&1 &&
       "$candidate" -c 'import sys; sys.exit(0 if sys.version_info >= (3, 8) else 1)' \
           >/dev/null 2>&1; then
        PY=$(command -v "$candidate")
        break
    fi
done

if [ -z "$PY" ]; then
    echo "ERROR: Python 3.8 or newer was not found on your PATH." >&2
    echo >&2
    if [ "$(uname -s)" = "Darwin" ]; then
        echo "Install it with Homebrew <https://brew.sh>:" >&2
        echo "  brew install python3" >&2
    else
        echo "Install it with your package manager, for example:" >&2
        echo "  sudo apt install python3 python3-pip python3-venv   # Debian/Ubuntu" >&2
        echo "  sudo dnf install python3 python3-pip                # Fedora" >&2
        echo "  sudo pacman -S python python-pip                    # Arch" >&2
    fi
    exit 1
fi

DEPS='import crcmod, bitarray, serial, requests'
VENV="$ROOT/.venv"

# Prefer a virtual environment we made earlier over the system interpreter:
# once the packages live there, that is where they stay.
if [ -x "$VENV/bin/python" ] && "$VENV/bin/python" -c "$DEPS" >/dev/null 2>&1; then
    PY="$VENV/bin/python"
elif ! "$PY" -c "$DEPS" >/dev/null 2>&1; then
    echo "First-time setup: installing the required Python packages"
    echo "(crcmod, bitarray, pyserial, requests)..."
    echo
    # Most current distributions mark the system Python "externally managed"
    # (PEP 668) and refuse a plain pip install into it, so go straight to a
    # virtual environment beside this script. It keeps the packages out of
    # the system site-packages, needs no root, and is easy to delete: it is
    # just the .venv folder here.
    if ! "$PY" -m venv "$VENV" >/dev/null 2>&1; then
        echo "ERROR: could not create a virtual environment at $VENV." >&2
        echo >&2
        echo "On Debian/Ubuntu the venv module is a separate package:" >&2
        echo "  sudo apt install python3-venv" >&2
        exit 1
    fi
    if ! "$VENV/bin/python" -m pip install --quiet --upgrade pip >/dev/null 2>&1; then
        : # a pip too old to upgrade itself is usually still good enough
    fi
    if ! "$VENV/bin/python" -m pip install --quiet \
            crcmod 'bitarray>=3' pyserial requests; then
        echo >&2
        echo "ERROR: automatic install failed. Install the packages by hand with:" >&2
        echo "  $VENV/bin/python -m pip install crcmod 'bitarray>=3' pyserial requests" >&2
        exit 1
    fi
    PY="$VENV/bin/python"
    echo
fi

# src/greaseweazle/__init__.py is gitignored and only gets written by a real
# build/version step. "make mypy" overwrites it with a type-stub-only line
# that breaks "from greaseweazle import __version__" at runtime, so always
# rewrite a working one here before running. No "+" in the string -- cli.py
# prints a "TEST/PRE-RELEASE" banner whenever one is present.
echo "__version__ = '0.1.dev0local'" > "$ROOT/src/greaseweazle/__init__.py"

export PYTHONPATH="$ROOT/src"
export GW_OPT=n

cd "$ROOT" || exit 1
exec "$PY" scripts/win/gw.py diag "$@"
