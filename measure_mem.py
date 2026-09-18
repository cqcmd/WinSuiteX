# -*- coding: utf-8 -*-
"""测量 WinSuiteX_glass*.exe 的内存占用（WorkingSet / PrivateUsage）。
用法：python measure_mem.py [exe路径]
仅测量，不修改任何文件；结束一定终止子进程。"""
import ctypes
import ctypes.wintypes as wt
import os
import subprocess
import sys
import time

BASE = r"D:\project\WorkBuddy\内置工具\WinSuiteX"
EXE = sys.argv[1] if len(sys.argv) > 1 else os.path.join(BASE, "WinSuiteX_glass4.exe")

u = ctypes.windll.user32
k = ctypes.windll.kernel32
psapi = ctypes.windll.psapi

class PMC(ctypes.Structure):
    _fields_ = [
        ("cb", ctypes.c_ulong),
        ("PageFaultCount", ctypes.c_ulong),
        ("PeakWorkingSetSize", ctypes.c_size_t),
        ("WorkingSetSize", ctypes.c_size_t),
        ("QuotaPeakPagedPoolUsage", ctypes.c_size_t),
        ("QuotaPagedPoolUsage", ctypes.c_size_t),
        ("QuotaPeakNonPagedPoolUsage", ctypes.c_size_t),
        ("QuotaNonPagedPoolUsage", ctypes.c_size_t),
        ("PagefileUsage", ctypes.c_size_t),
        ("PeakPagefileUsage", ctypes.c_size_t),
        ("PrivateUsage", ctypes.c_size_t),
    ]

proc = None
try:
    proc = subprocess.Popen([EXE])
    time.sleep(4)
    hwnd = u.FindWindowW("GlassShowcase", None)
    if not hwnd:
        print("!! 未找到窗口")
        sys.exit(2)
    # PROCESS_QUERY_INFORMATION(0x400) | PROCESS_VM_READ(0x10)
    ph = k.OpenProcess(0x410, False, proc.pid)
    if not ph:
        print("!! OpenProcess 失败")
        sys.exit(3)
    c = PMC()
    c.cb = ctypes.sizeof(c)
    if psapi.GetProcessMemoryInfo(ph, ctypes.byref(c), c.cb):
        print("exe = %s" % EXE)
        print("WorkingSetSize = %.2f MB" % (c.WorkingSetSize / 1048576.0))
        print("PrivateUsage    = %.2f MB" % (c.PrivateUsage / 1048576.0))
        print("PagefileUsage   = %.2f MB" % (c.PagefileUsage / 1048576.0))
    else:
        print("!! GetProcessMemoryInfo 失败")
    k.CloseHandle(ph)
finally:
    if proc is not None:
        try:
            proc.terminate()
            time.sleep(0.6)
            if proc.poll() is None:
                proc.kill()
        except Exception:
            pass
