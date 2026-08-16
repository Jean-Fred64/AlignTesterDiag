# Greaseweazle Host Tools: Diagnostic Edition

## About this fork: the `gw-diag` command

This is a fork of [keirf/greaseweazle][upstream] that adds **one new command,
`gw-diag`**: an interactive, live disk/drive diagnostic for bench-testing
floppy drives and disks in real time. Everything else in this repository is
unchanged from upstream, and the change is self-contained (a new
`tools/diag/` package plus a single dispatch line in `cli.py`).

### What it does

Where `gw read` takes a single pass and exits, `gw-diag` keeps the spindle
spinning and lets you drive the head interactively while it continuously
decodes the track currently under the head. It is built for diagnosing
drives: RPM, head movement, head alignment, etc.
For the current track it reports:

* **On-track vs off-track sector counts**, decoded live from the flux with
  an IBM MFM/FM decoder, so you can see at a glance whether the head is
  reading clean data (colored green when the full sector count reads
  clean, red otherwise). When sectors are off-track, it also tells you
  *which* track those stray sectors actually belong to.
* **Drive status pins**: Write-Protect, Disk-Change, Track-0 and Density,
  plus the host-driven Drive-Select and Motor-On lines.
* **Live RPM**, self-correcting the read window as the measured speed
  drifts, colored green within 5rpm of a standard spindle speed (300 or
  360) and red otherwise.
* **`--double-step`** for reading a 40-track disk in an 80-track drive.
* **`--step-delay`**, and delay settings from `gw delays` are now preserved
  across `gw-diag`'s internal resets instead of silently reverting to the
  firmware default partway through a session.

Interactive keys: number keys jump to a track, `+` / `-` / arrow keys step a
single track, `r` recalibrates, `h` toggles head, `m` toggles the motor,
`s` toggles drive-select independently of the motor (useful on drives that
gate their head load/unload solenoid off drive-select rather than
motor-on), `d` toggles density-select, and `q` / `Esc` quits.

![gw-diag stepping across a disk, most tracks reading cleanly with three empty reads at the unformatted track 40](screenshots/gw-diag.png)

*Stepping across a double-sided 40-track 5.25-inch disk at 250 kbps. The
formatted tracks read clean (green `S9/9`) with no off-track sectors
(`OT NO`), while track 40 is past the formatted area and unformatted, so all
three reads there come back empty (red `S0/9`). The drive can step to about
track 42, typical of later drives. The spindle holds a steady ~297 rpm, `TK0`
reads `ON` only at track 0, and `r` recalibrates the head back there.*

### Reading the live status line

Each line is one fresh read of the track currently under the head, printed a
few times a second.

```
Drive A: T0, H0, RPM 297.30, S9/9, OT NO, SEL:ON, MOT:ON, WP:H Unprot, TK0:L ON, DEN 2:L, DC34:?
```

| Field | Meaning |
|-------|---------|
| `Drive A` | The drive being read (set with `--drive`). |
| `T0` | Current track (cylinder) under the head. |
| `H0` | Current head/side. |
| `RPM 297.30` | Measured spindle speed, colored green within 5rpm of a standard spindle speed (300 or 360rpm) and red otherwise. Shows `ERR` when no disk or index pulse is seen, or `off` when you stop the motor with the `m` key. |
| `SX/X` | Sectors read cleanly out of the number expected, colored green when complete and red otherwise. `9/9` means every sector decoded. `S7/9` would mean two were missing or unreadable. The total sector count will vary depending on the type of disk being read. |
| `OT NO` | Off-track sectors. `NO` means none. Otherwise it lists which track the stray sectors claim to come from, as `T<cyl>/S<count>` (for example `T11/S2`), which points to the head mistracking or stepping to the wrong place. |
| `SEL:ON` | Drive-select line: `ON` while selected, `OFF` after the `s` key deselects it. Independent of `MOT`. Some drives gate their head load/unload solenoid off drive-select rather than motor-on. |
| `MOT:ON` | Motor-on line: `ON` while the motor is running, `OFF` after the `m` key turns it off. |
| `WP:H Unprot` | Write-protect line (34-pin connector pin 28): the raw level (`H` or `L`) and what it means on this drive (`Prot` or `Unprot`). |
| `TK0:L ON` | Track-0 sensor (pin 26): the raw level plus `ON`/`OFF` for whether the head is at track 0 (active-low, so `L` is `ON`). |
| `DEN 2:L` | Density-select output on pin 2 and the level you have set with the `d` key. On some dual-speed drives toggling `d` also changes the spindle speed: many 1.2MB 5.25-inch drives switch between 360 and 300 rpm with this pin, which you will see reflected live in the `RPM` field. Note too that on many 8-inch drives and 34-to-50-pin adapter cables pin 2 is wired as TG43 rather than density select. See `--gen-tg43`. |
| `DC34:?` | Disk-change/ready line on pin 34. `?` means your Greaseweazle cannot read this pin back. |

For a meaningful test, put a **known-good, standard IBM PC MFM formatted
floppy** in the drive, ideally one written on a drive known to be in good
order. A clean full-count read of a double density MFM disk (here `S9/9`)
with `OT NO` is then unambiguous confirmation that the drive under test is
reading correctly, and anything worse points at the drive or disk you are
checking rather than at the reference disk.

### Usage

```
gw-diag --rate 500 [options]
```

`--rate` (data rate in kbps) is required. The rest have sensible defaults.
Common options:

| Option | Purpose |
|--------|---------|
| `--rate KBPS` | Data rate, such as `250`, `500`, `1000` (**required**) |
| `--secs N` | Expected sectors/track (guessed from rate + rpm for standard formats if omitted) |
| `--rpm N` | Fix the spindle speed instead of tracking the live measurement |
| `--encoding mfm\|fm` | Track encoding (default `mfm`) |
| `--cyls N` / `--heads N` | Geometry limits |
| `--drive` | Which drive to diagnose |
| `--double-step` | Step two physical cylinders per logical track, for an 80-track drive reading a 40-track disk |
| `--step-delay N` | Step delay (usecs) for this session only, as `gw delays --step`. Without it, whatever `gw delays` already has set is preserved rather than reverting to the firmware default partway through the session |
| `--gen-tg43` | Auto-drive pin 2 as a TG43 signal for 8-inch drives |

Run `gw-diag --help` for the full list.

In these examples `gw-diag` is the launcher from Option A below:
`gw-diag.bat` on Windows, `./gw-diag.sh` on macOS and Linux. If you installed
with pipx (Option B), the command is `gw diag` with a space.

### Getting started

`gw-diag` runs on **Windows, macOS and Linux**. Live keyboard input goes
through a small per-platform layer
([`src/greaseweazle/tools/diag/keyboard.py`](src/greaseweazle/tools/diag/keyboard.py)):
the `msvcrt` console API on Windows, and `termios` cbreak mode on POSIX.
Either way it needs an interactive terminal. On macOS and Linux it says so
and exits straight away if stdin has been redirected or piped.

#### Windows

Pick whichever matches what you have installed.

**Option A, run from source with `gw-diag.bat` (no compiler needed).** The
speed-up extension is optional at runtime, so you can skip building it and
run the pure-Python code directly. The included `gw-diag.bat` launcher does
the setup for you: it writes the version stub, sets the environment, and
installs the four runtime packages on first run. Requires only
[Python 3.8 or newer](https://www.python.org/downloads/windows/) on your PATH.

`gw-diag.bat` runs the diagnostic by itself, but you will usually also want
the standard `gw.exe` tool for reading, writing, and everything else. To get
both, first download the latest Greaseweazle release:

[https://github.com/keirf/greaseweazle/releases](https://github.com/keirf/greaseweazle/releases)

Unzip it to a location on your hard drive, then copy this fork's files into
that same folder, overwriting any files when asked. You now have `gw.exe` and
`gw-diag.bat` side by side.

If you only want the diagnostic and have git installed, you can skip the
release download and clone this fork on its own instead:

```
git clone -b diag https://github.com/misterblack1/greaseweazle.git
cd greaseweazle
gw-diag.bat --rate 500
```

Run `gw-diag.bat` on its own (no arguments) for full help on every option
and the meaning of each field in the output line.

**Option B, install with pipx (needs a C compiler).** Requires
[Python 3.8 or newer](https://www.python.org/downloads/windows/) and the
[Microsoft C++ Build Tools](https://visualstudio.microsoft.com/visual-cpp-build-tools/),
because the install compiles a small optional speed-up extension:

```
pip install pipx
pipx install git+https://github.com/misterblack1/greaseweazle@diag
gw diag --rate 500
```

#### macOS and Linux

Pick whichever matches what you have installed.

**Option A, run from source with `gw-diag.sh` (no compiler needed).** The
speed-up extension is optional at runtime, so you can skip building it and
run the pure-Python code directly. The included `gw-diag.sh` launcher does
the setup for you: it writes the version stub, sets the environment, and
installs the four runtime packages on first run. Requires only Python 3.8 or
newer.

Most current distributions mark the system Python "externally managed"
(PEP 668) and refuse a plain `pip install` into it, so the launcher puts
those packages in a `.venv` folder beside itself instead. That needs no root,
touches nothing outside the clone, and is undone by deleting the folder. On
Debian and Ubuntu, `python3 -m venv` is a separate package
(`sudo apt install python3-venv`), and the launcher says so if it is missing.

```
git clone -b diag https://github.com/misterblack1/greaseweazle.git
cd greaseweazle
./gw-diag.sh --rate 500
```

Run `./gw-diag.sh` on its own (no arguments) for full help on every option
and the meaning of each field in the output line.

If the Greaseweazle is detected but cannot be opened (a permission error on
`/dev/tty*`), install the udev rules shipped with the repo and then unplug
and reconnect the device:

```
sudo cp scripts/49-greaseweazle.rules /etc/udev/rules.d/
```

**Option B, install with pipx (needs a C compiler).** This also gets you the
standard `gw` tool for reading, writing, and everything else:

```
pipx install git+https://github.com/misterblack1/greaseweazle@diag
gw diag --rate 500
```

### Status and LLM Usage

`gw diag` like the rest of Greaseweazle, is released into the public domain. See [COPYING](COPYING).

**LLM assistance was used in the creation of the `gw diag` code. Design, QA and testing were done by hand.**

[upstream]: https://github.com/keirf/greaseweazle

---

*Tools for accessing a floppy drive at the raw flux level.*

![CI Badge][ci-badge]
![Downloads Badge][downloads-badge]
![Version Badge][version-badge]

<img src="https://raw.githubusercontent.com/wiki/keirf/greaseweazle/assets/banner2.jpg">

---

This repository contains the host tools for controlling Greaseweazle:
an [Open Source][designfiles] USB device capable of reading and
writing raw data on nearly any type of floppy disk.

For more info see the following links:

* [Download the Greaseweazle software][Downloads]
* [Purchase a Greaseweazle][rmb]
* [Read the GitHub wiki](https://github.com/keirf/greaseweazle/wiki)
* [Greaseweazle firmware repository][firmware]

## Installation

**Windows:** Simply [download][Downloads] and unzip the latest release
of the host tools. You can now open a CMD window and run the `gw.exe` tool
from inside the unzipped release folder.

**macOS, Linux:** You can install the latest host tools release directly
from GitHub using Python Pipx:
```
pipx install git+https://github.com/keirf/greaseweazle@latest
```
See the [software installation wiki page][siwp] for more details.

## Usage

Type `gw --help` for on-line help.

Read the [GitHub wiki](https://github.com/keirf/greaseweazle/wiki)
for more detailed usage instructions.

## Redistribution

Greaseweazle source code, and all binary releases, are freely redistributable
in any form. Please see the [license](COPYING).

[designfiles]: https://github.com/keirf/greaseweazle/wiki/Design-Files
[firmware]: https://github.com/keirf/greaseweazle-firmware
[rmb]: https://github.com/keirf/greaseweazle/wiki/Purchase-a-Greaseweazle
[Downloads]: https://github.com/keirf/greaseweazle/wiki/Download-Host-Tools
[siwp]: https://github.com/keirf/greaseweazle/wiki/Software-Installation

[ci-badge]: https://github.com/keirf/greaseweazle/workflows/CI/badge.svg
[downloads-badge]: https://img.shields.io/github/downloads/keirf/greaseweazle/total
[version-badge]: https://img.shields.io/github/v/release/keirf/greaseweazle
