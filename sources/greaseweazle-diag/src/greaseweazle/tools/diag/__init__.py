# greaseweazle/tools/diag/__init__.py
#
# Greaseweazle control script: Interactive live disk/drive diagnostic.
#
# Based on the work of Keir Fraser
#
# This is free and unencumbered software released into the public domain.
# See the file COPYING for more details, or visit <http://unlicense.org>.

description = "Interactive live disk/drive diagnostic."

import os, sys, time
from typing import List, Optional, Tuple

from greaseweazle import error
from greaseweazle import usb as USB
# See decode.py for why codec.codec must be imported before ibm.ibm.
from greaseweazle.codec import codec  # noqa: F401
from greaseweazle.codec.ibm.ibm import Mode
from greaseweazle.tools import util
from greaseweazle.tools.delays import Delays
from greaseweazle.tools.diag import pinmap, decode, keyboard

# ANSI color for the live sector count: bright green on a complete read,
# bright red otherwise. Windows 10+ consoles support these once virtual-
# terminal processing is enabled (see enable_vt_colors); POSIX terminals
# handle them natively.
_GREEN = '\x1b[92m'
_RED = '\x1b[91m'
_RESET = '\x1b[0m'


def enable_vt_colors() -> None:
    """Turn on ANSI escape handling in the Windows console (no-op if it
    isn't a real console, such as output redirected to a file)."""
    if os.name != 'nt':
        return
    try:
        import ctypes
        kernel32 = ctypes.windll.kernel32
        handle = kernel32.GetStdHandle(-11)  # STD_OUTPUT_HANDLE
        mode = ctypes.c_uint32()
        if not kernel32.GetConsoleMode(handle, ctypes.byref(mode)):
            return
        # ENABLE_VIRTUAL_TERMINAL_PROCESSING = 0x0004
        kernel32.SetConsoleMode(handle, mode.value | 0x0004)
    except Exception:
        pass  # colors are cosmetic -- never fail the session over them

KEYLEGEND = """\
Keys: 0-9=goto track N0  +/-/<-/->=step 1  r=recalibrate
      h=head  m=motor  s=drive-select  d=density-select  q/Esc=quit"""

# (rate_kbps, nominal_rpm) -> (sectors/track, description). Sector count
# depends on rate *and* rpm together -- for example 500kbps is 15 sec/trk at
# 360rpm (1.2MB 5.25" HD) but 18 sec/trk at 300rpm (1.44MB 3.5" HD) -- so
# rate alone can't disambiguate it. Used both for the startup cheat sheet
# and to guess --secs when the user doesn't supply it.
STANDARD_FORMATS = {
    (250, 300): (9, '360KB 5.25" DD / 720KB 3.5" DD'),
    # 300kbps is the odd one: a 1.2MB 5.25" HD drive spins at 360rpm, so
    # reading a 300rpm-written DD disk in it scales the data rate up by
    # 360/300 = 1.2 (250 -> 300kbps) while the content stays 9 sec/trk.
    (300, 360): (9, 'DD disk in 1.2MB 5.25" HD drive @ 360rpm'),
    (500, 360): (15, '1.2MB 5.25" HD'),
    (500, 300): (18, '1.44MB 3.5" HD'),
    (1000, 300): (36, '2.88MB 3.5" ED'),
}


def cheatsheet() -> str:
    lines = ['Typical formats (rate @ rpm = sec/trk):']
    for (rate, rpm), (secs, desc) in sorted(STANDARD_FORMATS.items()):
        lines.append('  %5d kbps @ %3d rpm = %2d sec/trk  (%s)' %
                     (rate, rpm, secs, desc))
    return '\n'.join(lines)


def guess_secs(rate: int, rpm: Optional[float]) -> Optional[int]:
    if rpm is None:
        return None
    nominal_rpm = 360 if rpm >= 330 else 300
    entry = STANDARD_FORMATS.get((rate, nominal_rpm))
    return entry[0] if entry else None


class State:
    def __init__(self, args, delays: Delays) -> None:
        self.args = args
        self.delays = delays
        self.cyl = 0
        self.head = 0
        self.motor = True
        self.selected = True  # util.with_drive_selected() selects before run()
        self.density = False
        self.last_rpm: Optional[float] = None  # self-corrects the read window
        self.err_streak = 0  # consecutive ticks with no index (for recovery)


def sync_density_pin(usb: USB.Unit, st: State) -> None:
    # Pin 2 is an output we drive ourselves -- there's no hardware
    # read-back for it (see pinmap.py), so we can't just trust that
    # whatever level the firmware defaults to after a reset matches
    # st.density. Force a real transition (away, then back) so the GW is
    # actually driving the level the status line claims, rather than a
    # same-value set_pin() silently being a no-op against a stale register.
    usb.set_pin(pinmap.DENSITY_SELECT_PIN, not st.density)
    usb.set_pin(pinmap.DENSITY_SELECT_PIN, st.density)


def try_seek(usb: USB.Unit, st: State, new_cyl: int) -> None:
    # No upper clamp -- the user can deliberately probe past the declared
    # --cyls (for example to find a drive's real mechanical limit). usb.seek()
    # itself rejects nonsense values, and a too-far seek on real hardware
    # surfaces as a CmdError/Fatal we catch below rather than a crash.
    new_cyl = max(0, new_cyl)
    # Double-step: an 80-track drive reading a 40-track disk moves two
    # physical cylinders per logical track. st.cyl stays *logical* (that's
    # what the sector IDAMs and the decoder compare against). Only the
    # physical seek target is doubled.
    phys_cyl = new_cyl * 2 if st.args.double_step else new_cyl
    try:
        usb.seek(phys_cyl, st.head)
        st.cyl = new_cyl
    except (error.Fatal, USB.CmdError) as e:
        print(str(e))
        return
    if st.args.gen_tg43:
        st.density = st.cyl < pinmap.TG43_TRACK_THRESHOLD
        usb.set_pin(pinmap.DENSITY_SELECT_PIN, st.density)


# Cap on recalibration steps -- more cylinders than any real drive has, so
# a genuinely stuck head or dead TK0 sensor is reported instead of looping
# forever.
MAX_RECAL_STEPS = 80


def recalibrate(usb: USB.Unit, st: State) -> None:
    print('Recalibrating to track 0')
    prior = st.cyl

    # A previous failed/aborted seek can leave the firmware's internal
    # cylinder counter out of sync with the real head position, so a plain
    # seek(0) computes a zero (or wrong) step delta and never physically
    # moves the head -- this is exactly the state "gw reset" has always
    # been observed to clear. power_on_reset() (Cmd.Reset) wipes that
    # internal state, so redo the per-session setup it also resets.
    try:
        usb.power_on_reset()
        st.delays.update()  # power_on_reset() wipes step/settle/etc back to
                            # firmware defaults -- restore the session's delays
        usb.set_bus_type(st.args.drive.bus.value)
        # Respect a manual 's' deselect -- don't silently re-select just
        # because the reset forces the bus type to be reasserted.
        if st.selected:
            usb.drive_select(st.args.drive.unit_id)
        else:
            usb.drive_deselect()
        usb.drive_motor(st.args.drive.unit_id, st.motor)
        sync_density_pin(usb, st)
    except USB.CmdError as e:
        print('Recalibration reset failed: %s' % e)
        return

    for cyl in range(0, -MAX_RECAL_STEPS, -1):
        # Negative cylinders bypass usb.seek()'s own TRK0 check (it only
        # validates when target==0), so this always "succeeds" while still
        # physically stepping the head one track further each time -- a
        # fallback in case the reset alone doesn't fully re-home the head.
        try:
            usb.seek(cyl, st.head)
        except (error.Fatal, USB.CmdError):
            pass
        try:
            trk0 = not usb.get_pin(pinmap.TK0_PIN)
        except USB.CmdError:
            trk0 = False
        if trk0:
            st.cyl = 0
            try_seek(usb, st, prior)
            return
    print('Track 0 signal never asserted after %d steps -- '
          'bad TK0 sensor or heads stuck?' % MAX_RECAL_STEPS)
    # prior is very likely 0 here (that's the common way this loop gets
    # triggered) -- don't re-run the same failing seek(0) and dump the raw
    # firmware error a second time right after our own diagnostic.
    if prior != 0:
        try_seek(usb, st, prior)


def handle_key(usb: USB.Unit, st: State, key: Optional[str]) -> bool:
    """Returns False to request quit."""

    if key in ('q', 'esc'):
        return False
    elif key in ('left', ',', '-'):
        try_seek(usb, st, st.cyl - 1)
    elif key in ('right', '.', '+'):
        try_seek(usb, st, st.cyl + 1)
    elif key is not None and key.isdigit():
        try_seek(usb, st, int(key) * 10)
    elif key == 'h':
        if st.args.heads == 2:
            st.head = 1 - st.head
            # usb.seek() is what actually emits the head-select command. Just
            # flipping st.head leaves the device reading the *old* head until
            # the next physical step. Re-seek the current cylinder so the new
            # head takes effect immediately (no movement, same cyl).
            try_seek(usb, st, st.cyl)
    elif key == 'r':
        recalibrate(usb, st)
    elif key == 'm':
        st.motor = not st.motor
        usb.drive_motor(st.args.drive.unit_id, st.motor)
    elif key == 's':
        # Deliberately independent of the motor: some drives gate their
        # head load/unload solenoid off drive-select rather than motor-on,
        # so this lets that be tested on its own, with the motor left
        # running (or not) either way.
        st.selected = not st.selected
        if st.selected:
            usb.drive_select(st.args.drive.unit_id)
        else:
            usb.drive_deselect()
    elif key == 'd':
        if not st.args.gen_tg43:  # pin 2 is auto-tracked, 'd' is a no-op
            st.density = not st.density
            usb.set_pin(pinmap.DENSITY_SELECT_PIN, st.density)
    return True


def drive_label(drive: util.Drive) -> str:
    if drive.bus == USB.BusType.IBMPC:
        return 'B' if drive.unit_id == 1 else 'A'
    return str(drive.unit_id)


def status_line(usb: USB.Unit, st: State) -> str:

    args = st.args

    sigs = {}
    pin_of = {label: pin for label, pin, _ in pinmap.SIGNALS}
    wp_level: Optional[bool] = None
    tk0_level: Optional[bool] = None
    for label, pin, ambiguous in pinmap.SIGNALS:
        try:
            level = usb.get_pin(pin)
        except USB.CmdError:
            sigs[label] = '?'
            continue
        sigs[label] = ('H' if level else 'L') + ('?' if ambiguous else '')
        if label == 'WP':
            wp_level = level
        elif label == 'TK0':
            tk0_level = level

    wp_str = sigs['WP']
    if wp_level is not None:
        # This interface is active-low: WP asserted (L) == write-protected.
        wp_str += ' Unprot' if wp_level else ' Prot'

    tk0_str = sigs['TK0']
    if tk0_level is not None:
        # Active-low: TK0 asserted (L) == head is at track 0.
        tk0_str += ' OFF' if tk0_level else ' ON'

    rpm_str, rpm_val, sect = 'off', None, 0
    off_track: List[Tuple[int, int]] = []
    if st.motor:
        # Bound the capture by *time*, not by index pulses: with no disk
        # inserted there is never an index pulse, so usb.read_track(revs=N)
        # would block waiting for one that will never come. Sizing the
        # window from the last known RPM (falling back to --rpm, then a
        # generic guess) keeps it self-correcting once a real disk is in.
        assumed_rpm = args.rpm or st.last_rpm or 250.0
        # Cap the assumed speed at 400rpm so the window is always at least
        # 2 revs of the slowest standard drive. A stale/high last_rpm must
        # never size the window below one real revolution, or the read can
        # never catch a second index pulse to recover from.
        assumed_rpm = min(assumed_rpm, 400.0)
        ticks = int(usb.sample_freq * (60 / assumed_rpm) * 2.0)
        try:
            flux = usb.read_track(revs=0, ticks=ticks)
            # Need two index pulses for a genuine index-to-index period.
            # index_list[-1] with only one index is the partial capture-
            # start-to-index time, which reads as a spuriously high RPM.
            # Storing that in last_rpm shrinks the next window into a
            # spiral the read never climbs back out of (this is what makes
            # RPM sometimes never come back after a disk is pulled and
            # reinserted). Fewer than two indexes means no reading.
            if len(flux.index_list) >= 2:
                tpr = flux.index_list[-1] / flux.sample_freq
                rpm_val = 60 / tpr
                rpm_str = '%.2f' % rpm_val
                st.last_rpm = rpm_val
                time_per_rev = (60 / args.rpm) if args.rpm else tpr
                mode = Mode.MFM if args.encoding == 'mfm' else Mode.FM
                sect, off_track = decode.decode_tick(
                    flux, st.cyl, st.head, mode, args.rate, time_per_rev)
            else:
                rpm_str = 'ERR'
        except USB.CmdError:
            rpm_str = 'ERR'
        except Exception:
            # No disk / no index found, or garbage flux -- never let this
            # kill the session, just report nothing decoded this tick.
            rpm_str = 'ERR'
            sect, off_track = 0, []

        if rpm_val is not None:
            st.err_streak = 0
        elif st.selected:
            # No index this tick while we are selected and meant to be
            # spinning. A brief run of these is just an empty or spinning-up
            # drive, but a sustained run can also mean the device stopped
            # reporting the index until re-poked (not simply an absent disk),
            # which is why RPM sometimes stays ERR after a disk is pulled and
            # reinserted. Every few ticks, re-assert select/motor and re-seek
            # the current cylinder to kick it, without spamming a command
            # every tick.
            st.err_streak += 1
            if st.err_streak % 4 == 0:
                try:
                    usb.drive_select(args.drive.unit_id)
                    usb.drive_motor(args.drive.unit_id, True)
                    try_seek(usb, st, st.cyl)
                except Exception:
                    pass

    ot_str = ('NO' if not off_track else
             ','.join('T%d/S%d' % (c, n) for c, n in off_track))

    secs = args.secs if args.secs is not None else guess_secs(args.rate, rpm_val)
    secs_str = str(secs) if secs is not None else '?'

    # Color the sector field: bright green on a complete read (every
    # expected sector decoded), bright red on anything short of that. Only
    # when the motor is spinning and we actually know the expected count --
    # a guessed/unknown '?' or a stopped motor leaves it uncolored.
    sect_field = 'S%d/%s' % (sect, secs_str)
    if st.motor and secs is not None:
        color = _GREEN if sect == secs else _RED
        sect_field = '%s%s%s' % (color, sect_field, _RESET)

    # Color RPM green within +-5 of either standard spindle speed (300rpm
    # for 5.25"/8", 360rpm for 1.2MB HD), red otherwise. Left uncolored
    # when there's no reading at all ('off'/'ERR').
    rpm_field = rpm_str
    if rpm_val is not None:
        in_range = 295 <= rpm_val <= 305 or 355 <= rpm_val <= 365
        rpm_field = '%s%s%s' % (_GREEN if in_range else _RED, rpm_str, _RESET)

    return ('Drive %s: T%d, H%d, RPM %s, %s, OT %s, SEL:%s, MOT:%s, '
            'WP:%s, TK0:%s, DEN %d:%s, DC%d:%s' %
            (drive_label(args.drive), st.cyl, st.head, rpm_field, sect_field,
             ot_str, 'ON' if st.selected else 'OFF',
             'ON' if st.motor else 'OFF', wp_str, tk0_str,
             pinmap.DENSITY_SELECT_PIN, 'H' if st.density else 'L',
             pin_of['DC'], sigs['DC']))


def run(usb: USB.Unit, args, delays: Delays) -> None:

    enable_vt_colors()
    st = State(args, delays)
    print(cheatsheet())
    print(KEYLEGEND)
    if args.gen_tg43:
        print('TG43 auto-tracking enabled on pin 2 (threshold T%d). '
              'The d key is disabled' % pinmap.TG43_TRACK_THRESHOLD)

    # Take the keyboard before touching the drive, so a terminal that can't
    # provide key input fails immediately rather than after a recalibrate.
    # The context manager restores the terminal on every exit path, including
    # Ctrl-C and an unexpected error.
    with keyboard.Keyboard() as kb:
        sync_density_pin(usb, st)  # make sure GW really drives what we assume
        recalibrate(usb, st)  # known starting position for the session

        next_tick = time.monotonic()
        while True:
            if kb.kbhit():
                key = kb.read_key()
                if key is not None and not handle_key(usb, st, key):
                    break

            now = time.monotonic()
            if now >= next_tick:
                print(status_line(usb, st))
                next_tick = now + 0.5
            else:
                time.sleep(0.02)


def main(argv) -> None:

    epilog = (util.drive_desc)
    parser = util.ArgumentParser(usage='%(prog)s [options]', epilog=epilog)
    parser.add_argument("--device", help="greaseweazle device name")
    parser.add_argument("--drive", type=util.Drive(), default='A',
                        help="drive to diagnose")
    parser.add_argument("--cyls", type=util.min_int(1), default=84,
                        metavar="N", help="number of cylinders")
    parser.add_argument("--heads", type=int, choices=[1, 2], default=2,
                        help="number of heads")
    parser.add_argument("--double-step", action="store_true",
                        help="step two physical cylinders per track, for an "
                        "80-track drive reading a 40-track disk")
    parser.add_argument("--step-delay", type=util.uint, metavar="N",
                        help="Step Delay (usecs) for this session, as "
                        "'gw delays --step' (overrides the persisted "
                        "setting. Otherwise it is preserved across diag's "
                        "internal resets rather than reverting to the "
                        "firmware default)")
    parser.add_argument("--encoding", choices=['mfm', 'fm'], default='mfm',
                        help="track encoding")
    parser.add_argument("--rate", type=util.min_int(1), required=True,
                        metavar="KBPS", help="data rate, in kbps")
    parser.add_argument("--secs", type=util.min_int(1), default=None,
                        metavar="N", help="expected sectors per track "
                        "(omit to guess from rate/rpm for standard formats)")
    parser.add_argument("--rpm", type=float, default=None,
                        help="fixed spindle speed, in rpm "
                        "(omit to track the live measurement)")
    parser.add_argument("--gen-tg43", action="store_true",
                        help="auto-drive pin 2 as a TG43 signal for "
                        "8-inch drives (low from T%d up, high below), "
                        "matching --gen-tg43 in read/write/align. "
                        "Disables the d key, since pin 2 is then "
                        "under automatic control" % pinmap.TG43_TRACK_THRESHOLD)
    parser.description = description
    parser.prog += ' ' + argv[1]
    args = parser.parse_args(argv[2:])

    try:
        usb = util.usb_open(args.device)
        delays = Delays(usb)  # capture the persisted "gw delays" settings
        if args.step_delay is not None:
            delays.step = args.step_delay
        usb.power_on_reset()
        delays.update()  # power_on_reset() wiped them -- restore/apply now
        util.with_drive_selected(lambda: run(usb, args, delays), usb,
                                 args.drive)
    except USB.CmdError as err:
        print("Command Failed: %s" % err)


# Local variables:
# python-indent: 4
# End:
