@echo off
setlocal
set "HERE=%~dp0"
set "EXE=%HERE%winsuitex.exe"
set "WSX_TRACE=1"

title WinSuiteX crash locator
echo ==================================================
echo  WSX crash locator
echo  Goal: find which collector kills the process.
echo ==================================================
echo.

if not exist "%EXE%" (
    echo ERROR: winsuitex.exe not found next to this bat.
    pause
    exit /b 2
)

REM --- 打印编译时间戳：确认跑的是新 exe（文件 mtime 拷贝后会变，不能当判据）
echo Build:
"%EXE%" --version
echo.


echo Running with per-source tracing enabled.
echo The LAST trace line before death names the culprit.
echo.

"%EXE%" --json - > "%HERE%crash.json" 2> "%HERE%crash.err"
set "EC=%errorlevel%"

echo ================== EXIT CODE ==================
echo EXITCODE=%EC%
echo   0                      = all good, ignore everything below
echo   -1073741819 or 3221225477 = access violation 0xC0000005
echo                               = hard crash inside one collector
echo   -1073740940 or 3221225356 = heap corruption 0xC0000374
echo                               = someone wrote past the end of a buffer
echo   -1073740791 or 3221225705 = stack buffer overrun, a collector wrote past its buffer
echo   3221225477 style values  = process was killed, not a normal error return
echo ==============================================
echo.
echo If you see heap corruption, read the [heap] lines below. They are the key:
echo   before X = CORRUPT          = the heap was ALREADY broken before X ran.
echo                                 The culprit is an EARLIER source, not X.
echo   before X = ok, after X CORRUPT = X itself wrote past its buffer. X is the culprit.
echo   before audio = ok, nothing after = crash is INSIDE audio COM section, see [audio] lines.
echo WARNING: HeapValidate only inspects heap bookkeeping, not every byte of user data.
echo So it can miss an overwrite that has not damaged a nearby block header yet.
echo If all [heap] lines say ok but the process still dies, trust the LAST trace line.
echo ==============================================
echo.

echo ---------- trace ----------
if not exist "%HERE%crash.err" goto :noerr
REM stderr 是 UTF-8：切 65001 再 type，否则中文显示为乱码
chcp 65001 >nul
type "%HERE%crash.err"
chcp 936 >nul
goto :aftererr
:noerr
echo no stderr captured
:aftererr
echo ---------- end ----------
echo.

if not exist "%HERE%crash.json" goto :nojson
set "JSZ=0"
for %%F in ("%HERE%crash.json") do set "JSZ=%%~zF"
echo crash.json = %JSZ% bytes
goto :afterjson
:nojson
echo crash.json 未生成
:afterjson

echo.
echo Files kept in this folder: crash.err and crash.json
echo Send me the contents of crash.err.
echo.
echo Quick workaround once we know the culprit:
echo   set WSX_SKIP=audio
echo   winsuitex.exe --json - ^> out.json
echo Replace audio with the source name shown on the last trace line.
echo Run:  set WSX_SKIP=physical_disk
echo       winsuitex.exe --json - ^> out.json
echo.
pause
endlocal
