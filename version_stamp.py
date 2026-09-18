#!/usr/bin/env python3
# -*- coding: utf-8 -*-
"""
版本管理（四段式：主.次.修订.日期戳）
----------------------------------------
版本号格式：主.次.修订.编译日期(YYYYMMDD)
- 前三段（主.次.修订）：每次编译 修订号+1，满10进1（次号/主号随之进位）。
  保留“编译推进”的语义，但用满10进1限制位数，不会无限累加成 1.0.0.100。
- 第四段（日期戳）：编译当天的日期 YYYYMMDD，直观可追溯是哪天编的。
  （注：Windows FILEVERSION 第四段是 16 位字，上限 65535，故数值用 YYMMDD
   防溢出；FileVersion 字符串用完整 YYYYMMDD 供人阅读。）
version.ini 只存前三段（如 1.0.0），日期每次编译生成，不入库。

命令：
  python version_stamp.py                       平时编译：修订+1、满10进1、第四段=今天
  python version_stamp.py --set 1.2.3          手动设定前三段（发版里程碑用）
  python version_stamp.py --bump patch|minor|major  手动升某段
"""
import os
import re
import sys
import datetime

ROOT = os.path.dirname(os.path.abspath(__file__))
VER_FILE = os.path.join(ROOT, "version.ini")
RC_FILES = [
    os.path.join(ROOT, "app.rc"),       # 管理器 exe 版本信息 + 界面版本字符串
    os.path.join(ROOT, "shell.rc"),     # 内嵌的 Shell COM 服务器 DLL 版本信息
]


def today():
    d = datetime.date.today()
    full = d.year * 10000 + d.month * 100 + d.day          # YYYYMMDD
    short = (d.year % 100) * 10000 + d.month * 100 + d.day  # YYMMDD（<65535，防溢出）
    return full, short


def read_version():
    if not os.path.exists(VER_FILE):
        with open(VER_FILE, "w", encoding="utf-8") as f:
            f.write("1.0.0")
    with open(VER_FILE, "r", encoding="utf-8") as f:
        txt = f.read().strip()
    parts = txt.split(".")
    while len(parts) < 3:
        parts.append("0")
    try:
        maj, minv, patch = (int(x) for x in parts[:3])
    except Exception:
        maj, minv, patch = 1, 0, 0
    return maj, minv, patch


def write_version(maj, minv, patch):
    with open(VER_FILE, "w", encoding="utf-8") as f:
        f.write(f"{maj}.{minv}.{patch}")


def stamp_rc(maj, minv, patch, date_full, date_short):
    comma = f"{maj},{minv},{patch},{date_short}"
    dotted = f"{maj}.{minv}.{patch}.{date_full}"
    for RC_FILE in RC_FILES:
        if not os.path.exists(RC_FILE):
            print(f"[version_stamp] 未找到 {os.path.basename(RC_FILE)}，跳过")
            continue
        with open(RC_FILE, "r", encoding="utf-8") as f:
            s = f.read()
        s = re.sub(r"FILEVERSION\s+\d+,\d+,\d+,\d+",
                   f"FILEVERSION {comma}", s)
        s = re.sub(r"PRODUCTVERSION\s+\d+,\d+,\d+,\d+",
                   f"PRODUCTVERSION {comma}", s)
        s = re.sub(r'VALUE\s+"FileVersion",\s*"[^"]*"',
                   f'VALUE "FileVersion", "{dotted}"', s)
        s = re.sub(r'VALUE\s+"ProductVersion",\s*"[^"]*"',
                   f'VALUE "ProductVersion", "{dotted}"', s)
        # 界面版权行用的版本字符串（仅 app.rc 含 IDS_VERSION；shell.rc 无则跳过）
        s = re.sub(r'IDS_VERSION\s+"[^"]*"',
                   f'IDS_VERSION "{dotted}"', s)
        with open(RC_FILE, "w", encoding="utf-8") as f:
            f.write(s)
        print(f"[version_stamp] 已写入 {os.path.basename(RC_FILE)}: {dotted}")
    print(f"[version_stamp] 版本 -> {dotted}")


def bump(maj, minv, patch, kind):
    if kind == "major":
        maj += 1
        minv = 0
        patch = 0
    elif kind == "minor":
        minv += 1
        patch = 0
    else:
        patch += 1
    return maj, minv, patch


def main():
    args = sys.argv[1:]
    maj, minv, patch = read_version()
    date_full, date_short = today()

    if "--set" in args:
        i = args.index("--set")
        val = args[i + 1] if i + 1 < len(args) else ""
        try:
            p = val.split(".")
            maj, minv, patch = int(p[0]), int(p[1]), int(p[2])
        except Exception:
            print("[version_stamp] --set 参数错误，应为 X.Y.Z（前三段）")
            sys.exit(1)
        write_version(maj, minv, patch)
        stamp_rc(maj, minv, patch, date_full, date_short)
        return

    if "--bump" in args:
        i = args.index("--bump")
        kind = (args[i + 1] if i + 1 < len(args) else "patch").lower()
        maj, minv, patch = bump(maj, minv, patch, kind)
        write_version(maj, minv, patch)
        stamp_rc(maj, minv, patch, date_full, date_short)
        return

    # 默认（无参数）：每次编译 修订+1、满10进1；第四段=今天
    patch += 1
    if patch >= 10:
        patch = 0
        minv += 1
        if minv >= 10:
            minv = 0
            maj += 1
    write_version(maj, minv, patch)
    stamp_rc(maj, minv, patch, date_full, date_short)


if __name__ == "__main__":
    main()
