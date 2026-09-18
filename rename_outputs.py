#!/usr/bin/env python3
# -*- coding: utf-8 -*-
"""
宽字符重命名编译产物为中文名。
g++ 的窄字符 -o 在中文系统上会被 ANSI 代码页误转生成乱码文件名；
先用 ASCII 临时名编译，再用此脚本（Python 宽字符 API）重命名，结果与代码页无关。
"""
import os

MAP = {
    'build_main.exe':     'WinSuiteX.exe',
}

for k, v in MAP.items():
    if os.path.exists(k):
        if os.path.exists(v):
            os.remove(v)
        os.replace(k, v)
        print(f"  {k} -> {v}")
