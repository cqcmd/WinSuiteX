#!/usr/bin/env python3
# -*- coding: utf-8 -*-
"""发布打包脚本：把 dist 下的 Rust 产物 + 发布说明打成便携 zip。

用法：
  python tools/release_zip.py                 # 默认版本 0.5.5.20261023
  python tools/release_zip.py 0.5.6.20261024 # 指定版本（与主版本号一致即可）

产物：releases/WinSuiteX-GUI-v<ver>.zip（解压即用）。
注意：dist 目录保持"不打 zip"整洁规则，分发包只落在 releases/。
"""
import sys
import os
import zipfile

ROOT = os.path.dirname(os.path.dirname(os.path.abspath(__file__)))  # 项目根
REL = os.path.join(ROOT, "releases")
os.makedirs(REL, exist_ok=True)

VER = sys.argv[1] if len(sys.argv) > 1 else "0.5.5.20261023"

FILES = [
    ("dist/winsuitex-gui-gdi.exe", "winsuitex-gui-gdi.exe"),
    ("dist/winsuitex.exe", "winsuitex.exe"),
    ("releases/README.txt", "README.txt"),
]

OUT = os.path.join(REL, f"WinSuiteX-GUI-v{VER}.zip")

with zipfile.ZipFile(OUT, "w", zipfile.ZIP_DEFLATED) as z:
    for src, arc in FILES:
        path = os.path.join(ROOT, src)
        if not os.path.exists(path):
            print(f"[warn] 跳过不存在: {src}")
            continue
        z.write(path, arc)
        print(f"[+] {arc}  ({os.path.getsize(path)} bytes)")

print(f"created {OUT}  ({os.path.getsize(OUT)} bytes)")
