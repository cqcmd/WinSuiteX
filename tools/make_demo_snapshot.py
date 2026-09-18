#!/usr/bin/env python3
"""把真机采集快照注入一组"网吧真实外设"，生成用于演示渲染的快照。

底座取 winsuitex_real.json（QEMU VM 的 CPU/主板/网卡等真实字段），
只覆盖 `info.input_devices` 与 `info.audio` 两个数组，其余保持真机原样。
这样渲染出的报告 = 同一台机器插上真实外设后的样子，最能代表网吧客户机输出。
"""
import json
import sys

base = sys.argv[1] if len(sys.argv) > 1 else r"D:/project/WorkBuddy/WinSuiteX/winsuitex_real.json"
out = sys.argv[2] if len(sys.argv) > 2 else r"D:/project/WorkBuddy/WinSuiteX/winsuitex_demo.json"

with open(base, "r", encoding="utf-8") as f:
    snap = json.load(f)

# 网吧典型键鼠：干净品牌 + 共享 ODM（达摩鲨，靠运行时产品串兜底）+ 蓝牙键盘
snap["info"]["input_devices"] = [
    {
        "name": "Logitech G502 HERO Gaming Mouse",
        "kind": "Mouse",
        "vid": "046D",
        "pid": "C07E",
        "manufacturer": "Logitech",
        "is_bluetooth": False,
        "location": None,
    },
    {
        "name": "Cherry KC 1000 Corded Keyboard",
        "kind": "Keyboard",
        "vid": "046A",
        "pid": "0011",
        "manufacturer": "Cherry",
        "is_bluetooth": False,
        "location": None,
    },
    {
        # 共享 ODM VID 248A（达摩鲨/黑爵共用）：运行时产品串取不到时，
        # 落到 VID+PID 种子（248A:1007）或品牌+VID:PID 兜底，不做品牌误报。
        "name": "Darmoshark M3",
        "kind": "Mouse",
        "vid": "248A",
        "pid": "1007",
        "manufacturer": "Darmoshark",
        "is_bluetooth": True,
        "location": None,
    },
    {
        "name": "Apple Magic Keyboard",
        "kind": "Keyboard",
        "vid": "004C",
        "pid": "026C",
        "manufacturer": "Apple",
        "is_bluetooth": True,
        "location": None,
    },
    {
        # 种子表命中：1038:0472 -> SteelSeries Rival 150 Gaming Mouse
        "name": "SteelSeries Rival 150 Gaming Mouse",
        "kind": "Mouse",
        "vid": "1038",
        "pid": "0472",
        "manufacturer": "SteelSeries",
        "is_bluetooth": False,
        "location": None,
    },
]

# 音频：USB 游戏耳机（种子命中）、板载音箱、蓝牙耳麦
snap["info"]["audio"] = [
    {
        # 种子表命中：1038:1251 -> SteelSeries Arctis 5
        "name": "SteelSeries Arctis 5",
        "role": 1,
        "kind": "Headphone",
        "is_bluetooth": False,
        "vid": "1038",
        "pid": "1251",
        "manufacturer": "SteelSeries",
    },
    {
        "name": "Realtek High Definition Audio",
        "role": 1,
        "kind": "Speaker",
        "is_bluetooth": False,
        "vid": None,
        "pid": None,
        "manufacturer": None,
    },
    {
        "name": "JBL Quantum 400",
        "role": 3,  # 默认播放+录音 -> 耳麦
        "kind": "Headset",
        "is_bluetooth": True,
        "vid": "0D8C",
        "pid": "01DA",
        "manufacturer": "C-Media",
    },
]

with open(out, "w", encoding="utf-8") as f:
    json.dump(snap, f, ensure_ascii=False, indent=2)

print(f"demo snapshot written: {out}")
print(f"  input_devices = {len(snap['info']['input_devices'])}")
print(f"  audio         = {len(snap['info']['audio'])}")
