@echo off
chcp 65001 >nul 2>&1
REM ============================================================
REM  一键采集（带诊断日志）
REM ------------------------------------------------------------
REM  与 inventory.bat 唯一区别：自动设置 WSX_WMI_DEBUG=1，
REM  额外生成 dist\wmi_debug.log（记录 NVMe 序列号 / SMART / WMI
REM  各条采集路径的成功与否及失败错误码）。
REM
REM  当前排查 NVMe 序列号与通电时间时请用这个；日常采集用 inventory.bat。
REM
REM  跑完把下面两个文件一起发过去即可：
REM    dist\inventory.json
REM    dist\wmi_debug.log
REM ============================================================
call "%~dp0inventory.bat" debug
