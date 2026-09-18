@echo off
REM WinSuiteX hardware inventory launcher
REM Put winsuitex.exe and this .bat in the SAME folder, then double-click.
REM Produces inventory.json (machine-readable) and inventory.html (human report).

setlocal
set "HERE=%~dp0"
set "EXE=%HERE%winsuitex.exe"

if not exist "%EXE%" (
    echo ERROR: winsuitex.exe not found. Put it next to this .bat.
    pause
    exit /b 2
)

echo Collecting hardware info (keyboard/mouse, audio, nic, disk)...
"%EXE%" --json "%HERE%inventory.json"
"%EXE%" --report "%HERE%inventory.html"

echo.
echo Done. Output files:
echo   %HERE%inventory.json
echo   %HERE%inventory.html
echo.
echo Press any key to open the HTML report...
pause >nul
start "" "%HERE%inventory.html"
