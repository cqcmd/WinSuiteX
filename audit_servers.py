#!/usr/bin/env python3
# -*- coding: utf-8 -*-
import json, os, datetime

BASE = r"D:\project\WorkBuddy\WinSuiteX\dist\网吧服务器配置"
TODAY = datetime.date(2026, 9, 22)

def cell(x, w):
    return str(x)[:w].ljust(w)

def fmt_uptime(sec):
    """秒 → 人类可读（与 Rust format_uptime 同口径：天/小时/分/秒）。"""
    if sec is None:
        return ""
    sec = int(sec)
    d = sec // 86400
    h = (sec % 86400) // 3600
    m = (sec % 3600) // 60
    if d >= 1:
        return ("约 %d 天 %d 小时" % (d, h)) if h else ("约 %d 天" % d)
    if h >= 1:
        return ("约 %d 小时 %d 分" % (h, m)) if m else ("约 %d 小时" % h)
    if m >= 1:
        return "约 %d 分" % m
    return "约 %d 秒" % sec

# 占位/回声串识别：固件常把「空槽」回显成槽位名当 manufacturer/partno
# （如 Supermicro 双路板空槽 vendor="Dimm1_Manufacturer"、part_no="Dimm1_PartNum"），
# 也用 "Undefined"/"NO DIMM" 等占位。这些不是真·已插内存，缺容量是正确状态。
PLACEHOLDER = ("", "None", "Unknown", "unknown", "NO DIMM", "NODIMM",
               "To Be Filled By O.E.M.", "Default string", "N/A", "OEM",
               "Undefined", "undefined")
def is_placeholder(s):
    if s is None:
        return True
    t = (s.strip() if isinstance(s, str) else "")
    if t in PLACEHOLDER:
        return True
    tl = t.lower()
    if tl.endswith("_manufacturer") or tl.endswith("_partnum"):
        return True
    return False

def is_real_stick(m):
    """真实已插内存条 = 厂商不是占位/回声串。空槽（含固件回声）判为非真实。"""
    return not is_placeholder(m.get("vendor"))

# 收集所有 inventory.json（递归，处理 主/副 双服务器子夹）
all_json = []
for root, dirs, files in os.walk(BASE):
    if "inventory.json" in files:
        rel = os.path.relpath(root, BASE)
        parts = rel.split(os.sep)
        site = parts[0]
        sub = parts[1] if len(parts) > 1 else ""
        all_json.append((site, sub, os.path.join(root, "inventory.json")))

def analyze(site, sub, jf):
    rec = {"site": site, "sub": sub}
    rec["path"] = jf
    try:
        d = json.load(open(jf, encoding="utf-8"))
    except Exception as e:
        rec["parse_error"] = str(e)[:80]; return rec
    info = d.get("info", {})
    os_ = info.get("os", {})
    rec["os"] = (os_.get("product","") or "") + " " + (os_.get("version","") or "") + " b" + str(os_.get("build",""))
    rec["app"] = d.get("app_version")
    rec["collected"] = info.get("collected_at")
    rec["admin"] = info.get("admin_elevated")
    rec["server"] = info.get("is_server")
    rec["boot_at"] = os_.get("boot_at")
    rec["uptime_sec"] = os_.get("uptime_seconds")
    cpus = info.get("cpus", []) or []
    rec["cpu"] = len(cpus)
    rec["cpu_model"] = (cpus[0].get("model") if cpus else "") or ""
    sticks = info.get("memory", {}).get("sticks", []) or []
    rec["mem_n"] = len(sticks)
    rec["mem_real"] = sum(1 for m in sticks if is_real_stick(m))          # 真实已插（剔除空槽回声）
    rec["mem_nocap"] = sum(1 for m in sticks if not m.get("capacity_mb"))
    rec["mem_nomanu"] = sum(1 for m in sticks if not m.get("vendor"))
    # 真实口径缺口：只在「真·已插」里数，空槽回声不报成缺容量
    rec["mem_nocap_real"] = sum(1 for m in sticks if is_real_stick(m) and not m.get("capacity_mb"))
    rec["mem_nomanu_real"] = sum(1 for m in sticks if is_real_stick(m) and not m.get("vendor"))
    gpus = info.get("gpus", []) or []
    rec["gpu_n"] = len(gpus)
    rec["gpu_unknown"] = sum(1 for g in gpus if ("标准 VGA" in (g.get("name") or "")) or ("Microsoft 基本" in (g.get("name") or "")) or ("Mirage" in (g.get("name") or "")))
    rec["gpu_names"] = ";".join((g.get("name") or "?") for g in gpus)
    rec["disp"] = len(info.get("displays", []) or [])
    nics = info.get("nics", []) or []
    rec["nic"] = len(nics)
    disks = info.get("disks", []) or []
    rec["disk_n"] = len(disks)
    rec["disk_noserial"] = sum(1 for x in disks if not x.get("serial"))
    rec["disk_noiface"] = sum(1 for x in disks if not x.get("iface"))
    rec["vol"] = len(d.get("volumes", []) or [])
    rec["input"] = len(info.get("input_devices", []) or [])
    rec["usb"] = len(info.get("usb", []) or [])
    rec["audio"] = len(info.get("audio", []) or [])
    rec["diag"] = len(d.get("diagnostics", []) or [])
    tf = os.path.join(os.path.dirname(jf), "trace.log")
    rec["heap_bad"] = 0
    rec["trace_err"] = 0
    if os.path.exists(tf):
        for line in open(tf, encoding="utf-8", errors="replace"):
            if "[heap]" in line and not line.rstrip().endswith("ok"):
                rec["heap_bad"] += 1
            # 真实报错：排除 gpu 遥测的正常“加载失败（预期分支）”
            if ("ERROR" in line) or ("崩溃" in line) or ("panic" in line.lower()):
                rec["trace_err"] += 1
            if ("失败" in line) and ("gpu.tel" not in line):
                rec["trace_err"] += 1
    # stale?
    try:
        cd = datetime.datetime.fromisoformat((rec["collected"] or "")[:19]).date()
        rec["stale_days"] = (TODAY - cd).days
    except Exception:
        rec["stale_days"] = None
    return rec

records = [analyze(s, sub, jf) for (s, sub, jf) in all_json]
records.sort(key=lambda r: (r["site"], r["sub"]))

print("共解析到 inventory.json:", len(records), "个（含双服务器子实例）\n")
print("=== 逐站/逐实例异常表 ===")
header = cell("站点",24)+cell("实例",6)+cell("OS",22)+cell("收集",12)+cell("CPU",4)+cell("内存",7)+cell("GPU",5)+cell("显",4)+cell("盘",4)+cell("网",4)+cell("诊断",5)+cell("堆坏",5)+cell("trErr",6)+"备注"
print(header)
for r in records:
    if "parse_error" in r:
        print(cell(r["site"],24)+cell(r.get("sub",""),6)+" JSON解析失败: "+r["parse_error"]); continue
    note = []
    if r.get("diag"): note.append("采集降级"+str(r["diag"]))
    if r.get("heap_bad"): note.append("堆坏"+str(r["heap_bad"]))
    if r.get("trace_err"): note.append("trErr"+str(r["trace_err"]))
    if r.get("gpu_unknown"): note.append("GPU未装驱"+str(r["gpu_unknown"]))
    if r.get("nic",0) > 6: note.append("网卡偏多"+str(r["nic"]))
    if not r.get("admin"): note.append("未提权")
    if r.get("stale_days") and r["stale_days"] > 0: note.append("陈旧"+str(r["stale_days"])+"天")
    if r.get("uptime_sec") and r["uptime_sec"] > 30*86400: note.append("长时间未重启%d天"%int(r["uptime_sec"]//86400))
    if r.get("gpu_n") == 0: note.append("无GPU")
    memc = str(r["mem_real"]) + "实/" + str(r["mem_nocap_real"]) + "空"
    line = cell(r["site"],24)+cell(r.get("sub","") or "-",6)+cell(r["os"],22)+cell((r["collected"] or "")[5:16],12)+cell(r["cpu"],4)+cell(memc,7)+cell(r["gpu_n"],5)+cell(r["disp"],4)+cell(r["disk_n"],4)+cell(r["nic"],4)+cell(r["diag"],5)+cell(r["heap_bad"],5)+cell(r.get("trace_err",0),6)+";".join(note)
    print(line)

print("\n=== 重点看 ===")
flagged = False
for r in records:
    if "parse_error" in r:
        print("  [解析失败]", r["site"], r.get("sub"), r["parse_error"]); flagged=True; continue
    real = []
    if r.get("heap_bad"): real.append("堆检查非ok "+str(r["heap_bad"]))
    if r.get("trace_err"): real.append("trace真实报错 "+str(r["trace_err"]))
    if r.get("diag"): real.append("diagnostics="+str(r["diag"]))
    if r.get("gpu_unknown"): real.append(str(r["gpu_unknown"])+"块GPU疑似驱动未装")
    if r.get("nic",0) > 6: real.append("网卡偏多="+str(r["nic"]))
    if not r.get("admin"): real.append("未提权")
    if r.get("stale_days") and r["stale_days"] > 0: real.append("收集陈旧"+str(r["stale_days"])+"天")
    if r.get("gpu_n") == 0: real.append("检测到0块GPU")
    if real:
        print("  ["+r["site"]+(" / "+r["sub"] if r.get("sub") else "")+"] "+"; ".join(real)); flagged=True
if not flagged:
    print("  （无严重异常）")

print("\n=== 字段空缺率（后续统一修复备料）===")
tot_disk=tot_noserial=tot_noiface=0
tot_mem=tot_memreal=tot_memnocap=tot_memnomanu=tot_memnocap_real=tot_memnomanu_real=0
for r in records:
    if "parse_error" in r: continue
    tot_disk+=r.get("disk_n",0); tot_noserial+=r.get("disk_noserial",0); tot_noiface+=r.get("disk_noiface",0)
    tot_mem+=r.get("mem_n",0); tot_memreal+=r.get("mem_real",0)
    tot_memnocap+=r.get("mem_nocap",0); tot_memnomanu+=r.get("mem_nomanu",0)
    tot_memnocap_real+=r.get("mem_nocap_real",0); tot_memnomanu_real+=r.get("mem_nomanu_real",0)
echo_blank = tot_mem - tot_memreal   # 固件回声/占位（非真实已插）
print("盘: 总%d, serial空%d(%d%%), iface空%d(%d%%)" % (tot_disk, tot_noserial, 100*tot_noserial//max(tot_disk,1), tot_noiface, 100*tot_noiface//max(tot_disk,1)))
print("内存: 总%d根(真实已插%d, 回声/占位空槽%d)" % (tot_mem, tot_memreal, echo_blank))
print("  容量空(全部)%d(%d%%) / (真实已插)%d(%d%%)" % (tot_memnocap, 100*tot_memnocap//max(tot_mem,1), tot_memnocap_real, 100*tot_memnocap_real//max(tot_memreal,1)))
print("  厂商空(全部)%d(%d%%) / (真实已插)%d(%d%%)" % (tot_memnomanu, 100*tot_memnomanu//max(tot_mem,1), tot_memnomanu_real, 100*tot_memnomanu_real//max(tot_memreal,1)))

with open(os.path.join(BASE, "..", "网吧服务器配置_审核报告.md"), "w", encoding="utf-8") as f:
    f.write("# 网吧服务器配置 批量审核报告\n\n")
    f.write("解析到 inventory.json 共 "+str(len(records))+" 个（含双服务器子实例）。\n\n")
    for r in records:
        f.write("## "+r["site"]+(" / "+r["sub"] if r.get("sub") else "")+"\n")
        if "parse_error" in r:
            f.write("- JSON 解析失败: "+r["parse_error"]+"\n"); continue
        f.write("- OS: "+r["os"]+" | app "+str(r["app"])+" | 收集 "+str(r["collected"])+" | 提权="+str(r["admin"])+" 服务器="+str(r["server"])+"\n")
        f.write("- CPU "+str(r["cpu"])+" 颗: "+r["cpu_model"]+"\n")
        f.write("- 内存 "+str(r["mem_n"])+" 根 (真实已插 "+str(r["mem_real"])+", 回声/占位空槽 "+str(r["mem_n"]-r["mem_real"])+", 容量空(真实) "+str(r["mem_nocap_real"])+", 厂商空(真实) "+str(r["mem_nomanu_real"])+")\n")
        f.write("- GPU "+str(r["gpu_n"])+" 块: "+r["gpu_names"]+" (疑似未装驱 "+str(r["gpu_unknown"])+")\n")
        f.write("- 显示器 "+str(r["disp"])+" / 盘 "+str(r["disk_n"])+" (serial空 "+str(r["disk_noserial"])+", iface空 "+str(r["disk_noiface"])+") / 网 "+str(r["nic"])+" / 分区 "+str(r["vol"])+" / 键鼠 "+str(r["input"])+" / USB "+str(r["usb"])+" / 音频 "+str(r["audio"])+"\n")
        f.write("- 诊断项 "+str(r["diag"])+" | 堆检查坏 "+str(r["heap_bad"])+" | trace真实报错 "+str(r.get("trace_err",0))+" | 陈旧"+str(r.get("stale_days"))+"天\n")
        f.write("- 开机时间 "+str(r.get("boot_at"))+" | 在线时长 "+fmt_uptime(r.get("uptime_sec"))+"\n\n")
print("\n报告已写: D:/project/WorkBuddy/WinSuiteX/网吧服务器配置_审核报告.md")
