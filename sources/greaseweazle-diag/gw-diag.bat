@echo off
setlocal

rem This batch file lives in the root of the greaseweazle fork and runs the
rem gw diag tool straight from the source tree next to it.
set "ROOT=%~dp0"
if "%ROOT:~-1%"=="\" set "ROOT=%ROOT:~0,-1%"

rem Always show which commit this copy is at, first thing, before anything
rem else can go wrong or print help and exit -- so a copied/synced tree
rem never leaves you guessing which version you're actually running.
where git >nul 2>nul
if not errorlevel 1 (
    for /f "delims=" %%v in ('git -C "%ROOT%" log -1 --format^="%%h %%ci %%s" 2^>nul') do echo gw-diag: %%v
    if errorlevel 1 echo gw-diag: ^(not a git checkout -- can't identify commit^)
) else (
    echo gw-diag: ^(git not on PATH -- can't identify commit^)
)

if "%~1"=="" goto :usage
if /I "%~1"=="/?" goto :usage
if /I "%~1"=="/h" goto :usage

if not exist "%ROOT%\scripts\win\gw.py" (
    echo ERROR: gw-diag.bat must sit in the root of the greaseweazle fork,
    echo next to the scripts\ and src\ folders. "%ROOT%\scripts\win\gw.py"
    echo was not found.
    exit /b 1
)

call :check_python
if errorlevel 1 exit /b 1

call :check_deps
if errorlevel 1 exit /b 1

rem src\greaseweazle\__init__.py is gitignored and only gets written by a
rem real build/version step. "make mypy" overwrites it with a type-stub-only
rem line that breaks "from greaseweazle import __version__" at runtime, so
rem always rewrite a working one here before running. No "+" in the string --
rem cli.py prints a "TEST/PRE-RELEASE" banner whenever one is present.
> "%ROOT%\src\greaseweazle\__init__.py" echo __version__ = '0.1.dev0local'

set "PYTHONPATH=%ROOT%\src"
set "GW_OPT=n"

pushd "%ROOT%"
python scripts\win\gw.py diag %*
set "RC=%ERRORLEVEL%"
popd

endlocal & exit /b %RC%

:check_python
where python >nul 2>nul
if errorlevel 1 (
    echo ERROR: Python was not found on your PATH.
    echo.
    echo gw-diag.bat needs Python 3.8 or newer. Install it from
    echo https://www.python.org/downloads/ and check "Add Python to PATH"
    echo during setup, then try again.
    exit /b 1
)
python -c "import sys; sys.exit(0 if sys.version_info >= (3, 8) else 1)" >nul 2>nul
if errorlevel 1 (
    echo ERROR: Python 3.8 or newer is required.
    python --version
    echo.
    echo Install a newer Python from https://www.python.org/downloads/ and
    echo try again.
    exit /b 1
)
exit /b 0

:check_deps
python -c "import crcmod, bitarray, serial, requests" >nul 2>nul
if not errorlevel 1 exit /b 0
echo First-time setup: installing the required Python packages
echo (crcmod, bitarray, pyserial, requests)...
echo.
python -m pip install crcmod "bitarray>=3" pyserial requests
if errorlevel 1 (
    echo.
    echo ERROR: automatic install failed. Install the packages by hand with:
    echo   python -m pip install crcmod "bitarray^>=3" pyserial requests
    exit /b 1
)
echo.
exit /b 0

:usage
echo.
echo gw-diag.bat - run the gw diag tool straight from source, no build or
echo               install needed. Handy on Windows machines that don't have
echo               the MSVC C++ Build Tools required to compile a full pipx
echo               install. Just clone the fork and run this file.
echo.
echo Needs Python 3.8 or newer on your PATH. The required Python packages
echo (crcmod, bitarray^>=3, pyserial, requests) are installed automatically
echo on first run.
echo.
echo USAGE:
echo   gw-diag.bat --rate KBPS [other options]
echo.
echo PARAMETERS:
echo   --device NAME    Serial port name, such as COM3. Optional, only needed
echo                     if the Greaseweazle isn't auto-detected or you have
echo                     more than one plugged in.
echo   --drive ID       Which physical drive to use. A or B for an IBM/PC
echo                     cable, 0-3 for a Shugart cable. Default: A.
echo   --cyls N         Number of cylinders the drive has. Default: 84. You
echo                     can still step past this to probe a drive's real
echo                     mechanical limit. It's just the default range.
echo   --heads N        Number of heads/sides: 1 or 2. Default: 2.
echo   --double-step    Step two physical cylinders per logical track, for
echo                     an 80-track drive reading a 40-track disk.
echo   --step-delay N   Step Delay in microseconds for this session only
echo                     (same units as "gw delays --step"). Optional --
echo                     without it, diag preserves whatever "gw delays"
echo                     already has set, instead of reverting to the
echo                     firmware default the way it used to.
echo   --encoding TYPE  mfm or fm. Default: mfm. Almost everything from the
echo                     PC, Amiga and Atari ST era is mfm. Old 8-inch or
echo                     single-density disks are fm.
echo   --rate KBPS      Data rate in kilobits per second. Required. Common
echo                     values: 250 for double density, 500 for high
echo                     density. Rate alone doesn't fully determine the
echo                     format (500kbps is both 1.2MB 5.25" HD and 1.44MB
echo                     3.5" HD, at different rpm), so this is the one
echo                     value you must always supply yourself.
echo   --secs N         Sectors expected per track. Optional: left out, the
echo                     tool guesses it from --rate and the measured rpm
echo                     for standard formats. Set it yourself for a
echo                     nonstandard or copy-protected disk.
echo   --rpm N          Force a fixed spindle speed instead of using the
echo                     live measurement. Optional, normally leave this
echo                     out.
echo   --gen-tg43       Auto-drive pin 2 as a TG43 signal for 8-inch
echo                     drives: high below track 60, low from track 60
echo                     up (matches --gen-tg43 in gw read/write/align).
echo                     Updated automatically on every seek. Disables
echo                     the d key while active, since pin 2 is then
echo                     under automatic control, not manual toggle.
echo.
echo OUTPUT LINE FORMAT:
echo   Drive A: T0, H0, RPM 297.53, S9/9, OT NO, SEL:ON, MOT:ON,
echo   WP:H Unprot, TK0:L ON, DEN 2:L, DC34:?
echo.
echo   Drive       Drive ID you selected.
echo   T           Current track/cylinder number.
echo   H           Current head/side.
echo   RPM         Measured spindle speed this update. Green if within 5rpm
echo               of a standard speed (300 or 360), red otherwise, only
echo               once there's an actual reading. Shows ERR if no disk or
echo               index signal was found, or "off" if you turned the
echo               motor off with the m key.
echo   S           Sectors read cleanly from the current track, out of how
echo               many were expected (from --secs, or the guess).
echo   OT          Off-track sectors: sectors that decoded fine but whose
echo               header said a different track than where the head
echo               actually is. Shows NO if there aren't any. Otherwise
echo               shows which track(s) they actually came from, as
echo               T^<cyl^>/S^<count^>, for example T11/S2 means 2 sectors read
echo               cleanly but tagged as track 11 while the head is on a
echo               different track. More than one wrong track in the same
echo               read shows as a comma list, such as T11/S2,T12/S1. Any
echo               non-NO value usually means the head is mistracking or
echo               stepping to the wrong place.
echo   SEL         Drive-select line: ON while selected, OFF after the s
echo               key deselects it. Independent of MOT -- some drives
echo               gate their head load/unload solenoid off drive-select
echo               rather than motor-on, so this lets you test that
echo               without also stopping the spindle.
echo   MOT         Motor-on line: ON while the motor is running, OFF
echo               after the m key turns it off.
echo   WP          Write-protect pin (28), shown as the raw H/L level plus
echo               whether that means Prot or Unprot on this drive.
echo   TK0         Track-0 sensor pin (26): raw H/L level plus ON when the
echo               head is at track 0 (L), OFF otherwise (H).
echo   DEN 2       Density-select output pin (2) and the level you've set
echo               it to with the d key. Note: pin 2 isn't always density
echo               select. On 8-inch drives, and on most 34-to-50-pin
echo               adapter cables, the same pin is commonly wired as TG43
echo               instead (write precompensation enable past a given
echo               cylinder). The d key just toggles the raw pin either
echo               way. Use --gen-tg43 instead if you want it driven
echo               automatically by cylinder rather than by hand.
echo   DC34        Disk-change/ready pin (34). Shown as ? if your
echo               Greaseweazle can't read this particular pin back.
echo.
echo The tool itself prints a key legend when it starts (step, jump, head,
echo recalibrate, motor, drive-select, density, quit).
echo.
exit /b 1
