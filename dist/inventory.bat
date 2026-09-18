@echo off
chcp 65001 >nul 2>&1
setlocal
set "HERE=%~dp0"
set "EXE=%HERE%winsuitex.exe"
set "DEBUG_MODE=0"
if /i "%~1"=="debug" set "DEBUG_MODE=1"
if /i "%~1"=="-debug" set "DEBUG_MODE=1"

if not exist "%EXE%" (
    echo ERROR: winsuitex.exe not found next to this bat.
    echo Expected location: %EXE%
    pause
    exit /b 2
)

REM 切到 dist 目录：`wmi_debug.log` 写在工作目录下，这样日志也落在 dist。
cd /d "%HERE%"

echo ============================================================
echo  EXE 信息（用于确认跑的是不是新编译产物）
echo ============================================================
"%EXE%" --version
for %%F in ("%EXE%") do echo   mtime = %%~tF      size = %%~zF bytes
echo.

if "%DEBUG_MODE%"=="1" (
    set WSX_WMI_DEBUG=1
    echo [调试模式] 已开启，将生成 wmi_debug.log
    if exist "wmi_debug.log" del /f /q "wmi_debug.log" >nul 2>&1
    echo.
)

REM --- 记录改动前的 mtime。回拷是 best-effort 且失败会被静默丢弃，
REM     因此跑完后比对一次：没变就说明回拷失败、dist 下仍是旧数据。
set "JSON_BEFORE="
set "HTML_BEFORE="
if exist "%HERE%inventory.json" for %%F in ("%HERE%inventory.json") do set "JSON_BEFORE=%%~tF"
if exist "%HERE%inventory.html" for %%F in ("%HERE%inventory.html") do set "HTML_BEFORE=%%~tF"

echo Target folder: %HERE%
echo Collecting hardware info (keyboard/mouse, audio, nic, disk)...
echo.

REM --- KEY WORKAROUND ---
REM Let the exe print to stdout, and let cmd redirect it to a file in TEMP.
REM cmd.exe creates the file as a trusted process, so a process-based filter
REM that watches winsuitex.exe writes will not delete it. TEMP is also on a
REM different volume than the working folder.

set "JSON_OUT=%TEMP%\winsuitex_inventory_%RANDOM%.json"
set "HTML_OUT=%TEMP%\winsuitex_inventory_%RANDOM%.html"

"%EXE%" --json - > "%JSON_OUT%"
if errorlevel 1 (
    echo ERROR: hardware collection failed - exe exit nonzero.
    pause
    exit /b 1
)
if not exist "%JSON_OUT%" (
    echo ERROR: JSON output missing after exe returned success.
    pause
    exit /b 1
)
set "JSZ=0"
for %%F in ("%JSON_OUT%") do set "JSZ=%%~zF"
if %JSZ% LSS 1000 (
    echo ERROR: JSON is only %JSZ% bytes - looks like an OLD exe.
    pause
    exit /b 1
)

"%EXE%" --report - > "%HTML_OUT%"
if errorlevel 1 (
    echo ERROR: report generation failed - exe exit nonzero.
    pause
    exit /b 1
)
if not exist "%HTML_OUT%" (
    echo ERROR: HTML output missing after exe returned success.
    pause
    exit /b 1
)
set "HSZ=0"
for %%F in ("%HTML_OUT%") do set "HSZ=%%~zF"
if %HSZ% LSS 1000 (
    echo ERROR: HTML is only %HSZ% bytes - looks like an OLD exe.
    pause
    exit /b 1
)

REM Best-effort copy back to the working folder —— 失败必须显式暴露，
REM 否则 dist 下留着上一轮的结果却看起来"刚跑过"，会把排查带沟里。
copy /y "%JSON_OUT%" "%HERE%inventory.json" >nul 2>&1
copy /y "%HTML_OUT%" "%HERE%inventory.html" >nul 2>&1

echo.
echo ============================================================
echo  回拷校验（比对 mtime 是否真的刷新）
echo ============================================================
set "COPYFAIL=0"
if not exist "%HERE%inventory.json" (
    echo [FAIL] inventory.json 不存在，回拷失败！
    set "COPYFAIL=1"
) else (
    for %%F in ("%HERE%inventory.json") do (
        echo   inventory.json : %%~tF  ^(%%~zF bytes^)
        if "%%~tF"=="%JSON_BEFORE%" (
            echo   [WARN] mtime 未刷新 —— 回拷可能失败，这是上一轮的旧数据！
            set "COPYFAIL=1"
        )
    )
)
if not exist "%HERE%inventory.html" (
    echo [FAIL] inventory.html 不存在，回拷失败！
    set "COPYFAIL=1"
) else (
    for %%F in ("%HERE%inventory.html") do (
        echo   inventory.html : %%~tF  ^(%%~zF bytes^)
        if "%%~tF"=="%HTML_BEFORE%" (
            echo   [WARN] mtime 未刷新 —— 回拷可能失败，这是上一轮的旧数据！
            set "COPYFAIL=1"
        )
    )
)

REM 数据自身的采集时间戳（JSON 内字段），比文件 mtime 更权威。
echo.
echo 数据内采集时间 collected_at:
findstr /c:"collected_at" "%HERE%inventory.json"
echo.

if "%COPYFAIL%"=="1" (
    echo [FAIL] 产物未正确回拷到 dist，请把 %%TEMP%% 下的文件手动拷出：
    echo   %JSON_OUT%
    echo   %HTML_OUT%
    pause
    exit /b 3
)

echo Done.
echo   JSON : %HERE%inventory.json ^(%JSZ% bytes^)
echo   HTML : %HERE%inventory.html ^(%HSZ% bytes^)

if "%DEBUG_MODE%"=="1" (
    echo.
    echo ------------------------------------------------------------
    echo  调试日志 wmi_debug.log :
    if exist "wmi_debug.log" (
        for %%F in ("wmi_debug.log") do echo    %HERE%wmi_debug.log  ^(%%~zF bytes^)
        echo  （把这个文件一起发我，可一次定位 NVMe 采集失败原因）
    ) else (
        echo    [WARN] 未生成 —— 检查 exe 是否为含诊断代码的新版本
    )
    echo ------------------------------------------------------------
)

echo Opening report...
start "" "%HERE%inventory.html"
pause
endlocal
