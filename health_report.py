#!/usr/bin/env python3
# -*- coding: utf-8 -*-
"""网吧/电竞酒店服务器硬件健康排查（磁盘为主）。

对 dist/网吧服务器配置 下所有 inventory.json 逐盘评估 SMART/通电/健康信号，
标出「该更换 / 该关注 / 无法评估」三类，输出控制台摘要 + markdown 报告。

判定口径（针对网吧无盘服务器 7x24 高负载场景收紧）：
  🔴 立即处理：health_status 非 Healthy / reallocated_sectors>0 / pending_sectors>0 / media_errors>0
  🟠 关注预警：POH>=50000(≈5.7年,高龄) / POH>=35000 且 HDD(≈4年,偏老) / percentage_used>=80(SSD磨损) / 温度>=55℃
  ⚪ 无法评估：smart.source 不可用或关键字段全 None（多因 VMD 未透传 / 存储控制器不返回）
"""
import json, os, datetime, glob

BASE = r"D:\project\WorkBuddy\WinSuiteX\dist\网吧服务器配置"
TODAY = datetime.date(2026, 9, 22)

AGE_WARN_HDD = 35000      # HDD 偏老关注阈值（约 4 年）
AGE_CRIT = 50000          # 高龄阈值（约 5.7 年）
SSD_WEAR_WARN = 80        # NVMe/SSD 磨损预留（percentage_used 百分比）
TEMP_WARN = 55            # 温度关注阈值 ℃

SEV_ORDER = {"crit": 0, "warn": 1, "ok": 2, "na": 3}

def poh_years(h):
    return (h / 8760.0) if h else None

def assess_disk(dk):
    """返回 (severity, reasons[], poh, wear, temp, status)"""
    sm = dk.get("smart") or {}
    reasons = []
    status = sm.get("health_status")
    realloc = sm.get("reallocated_sectors")
    pend = sm.get("pending_sectors")
    media = sm.get("media_errors")
    poh = sm.get("power_on_hours")
    wear = sm.get("percentage_used")
    temp = sm.get("temperature_c")
    media_type = dk.get("media")
    iface = dk.get("iface")
    src = sm.get("source")
    unavail = sm.get("unavailable_reason")

    # 无法评估：源不可用或关键健康字段全空
    if unavail or (status is None and realloc is None and pend is None and poh is None and wear is None):
        return "na", ["SMART 不可用" + (": " + unavail if unavail else "")], poh, wear, temp, status

    if status and status != "Healthy":
        reasons.append("健康态=%s" % status)
    severe = False
    if isinstance(realloc, int) and realloc > 0:
        reasons.append("重映射扇区=%d" % realloc)
        if realloc >= 50:               # 重映射扇区大量增长 = 坏道扩散，紧急
            severe = True
    if isinstance(pend, int) and pend > 0:
        reasons.append("待映射扇区=%d" % pend); severe = True
    if isinstance(media, int) and media > 0:
        reasons.append("介质错误=%d" % media); severe = True

    if isinstance(poh, int):
        if poh >= AGE_CRIT:
            reasons.append("通电%dh(约%.1f年,高龄)" % (poh, poh_years(poh)))
        elif poh >= AGE_WARN_HDD and media_type == "HDD":
            reasons.append("通电%dh(约%.1f年,偏老)" % (poh, poh_years(poh)))
    if isinstance(wear, int) and wear >= SSD_WEAR_WARN:
        reasons.append("SSD磨损=%d%%" % wear)
    if isinstance(temp, int) and temp >= TEMP_WARN:
        reasons.append("温度%d℃" % temp)

    # 严重度：crit=紧急(坏道扩散/待映射/介质错误)；warn=SMART告警或高龄或高温，计划更换/观察
    sev = "crit" if severe else ("warn" if reasons else "ok")
    return sev, reasons, poh, wear, temp, status

def main():
    files = sorted(glob.glob(os.path.join(BASE, "**", "inventory.json"), recursive=True))
    rows = []          # 每条磁盘一行
    site_stat = {}     # 站点 -> {crit,warn,na,ok,disks}
    total = {"crit": 0, "warn": 0, "na": 0, "ok": 0}

    for f in files:
        rel = os.path.relpath(f, BASE)
        parts = rel.split(os.sep)
        site = parts[0]; sub = parts[1] if len(parts) > 1 else ""
        try:
            d = json.load(open(f, encoding="utf-8"))
        except Exception as e:
            rows.append((site, sub, "-", "-", "解析失败: %s" % str(e)[:60], "crit", None, None, None, None)); continue
        disks = d.get("info", {}).get("disks", []) or []
        sst = site_stat.setdefault(site, {"crit": 0, "warn": 0, "na": 0, "ok": 0, "disks": 0})
        sst["disks"] += len(disks)
        for dk in disks:
            model = dk.get("model") or "?"
            iface = dk.get("iface") or "?"
            media = dk.get("media") or "?"
            sev, reasons, poh, wear, temp, status = assess_disk(dk)
            total[sev] += 1
            sst[sev] += 1
            tag = {"crit": "🔴", "warn": "🟠", "na": "⚪", "ok": "✅"}[sev]
            detail = ("；".join(reasons)) if reasons else ("健康(%s)" % (status or "?"))
            rows.append((site, sub, "%s %s %s" % (media, iface, model),
                         "POH=%s" % (poh if poh is not None else "?"),
                         detail, sev, poh, wear, temp, status))

    # 排序：严重优先，其次通电时长高
    def sortkey(r):
        return (SEV_ORDER[r[5]], -(r[6] or 0))
    rows.sort(key=sortkey)

    # ---- 控制台摘要 ----
    print("共解析 inventory.json:", len(files), "个，磁盘", sum(s["disks"] for s in site_stat.values()), "块")
    print("🔴 紧急更换 %d 块 | 🟠 计划更换/观察 %d 块 | ⚪ 无法评估 %d 块 | ✅ 健康 %d 块\n"
          % (total["crit"], total["warn"], total["na"], total["ok"]))
    print("=== 需处理/关注的磁盘（按严重度+通电时长）===")
    for r in rows:
        if r[5] in ("crit", "warn"):
            print("%s %-22s %-6s %s | %s" % ({"crit":"🔴","warn":"🟠"}[r[5]],
                  (r[0] + ("/"+r[1] if r[1] else ""))[:22], r[2][:30], r[3], r[4]))

    # ---- markdown 报告 ----
    out = os.path.join(BASE, "..", "网吧服务器配置_硬件健康报告.md")
    with open(out, "w", encoding="utf-8") as fo:
        fo.write("# 网吧服务器配置 硬件健康排查报告\n\n")
        fo.write("生成 %s。数据来源：%d 个 inventory.json，共 %d 块磁盘。\n"
                 % (TODAY.isoformat(), len(files), sum(s["disks"] for s in site_stat.values())))
        fo.write("判定口径：🔴 坏道/待映射/介质错误/健康态非Healthy；🟠 通电≥%.0f年(HDD偏老)/≥%.1f年(高龄)/SSD磨损≥%d%%/温度≥%d℃；⚪ SMART不可用(多因VMD未透传)。\n\n"
                 % (AGE_WARN_HDD/8760, AGE_CRIT/8760, SSD_WEAR_WARN, TEMP_WARN))
        fo.write("## 总览\n")
        fo.write("- 🔴 紧急更换：**%d** 块（坏道扩散/待映射扇区/介质错误，立即备份并换盘）\n- 🟠 计划更换/观察：**%d** 块（SMART 告警或高龄/高温，纳入备件计划）\n- ⚪ 无法评估：**%d** 块（数据缺口，非故障）\n- ✅ 健康：**%d** 块\n\n"
                 % (total["crit"], total["warn"], total["na"], total["ok"]))
        if total["crit"]:
            fo.write("## 🔴 紧急更换（坏道扩散/待映射扇区，立即备份并换盘）\n")
            for r in rows:
                if r[5] == "crit":
                    fo.write("- **%s%s** %s — %s %s\n" % (r[0], ("/"+r[1] if r[1] else ""), r[2], r[3], r[4]))
        if total["warn"]:
            fo.write("\n## 🟠 计划更换 / 观察（SMART 告警或高龄/高温）\n")
            for r in rows:
                if r[5] == "warn":
                    fo.write("- %s%s %s — %s %s\n" % (r[0], ("/"+r[1] if r[1] else ""), r[2], r[3], r[4]))
        if total["na"]:
            fo.write("\n## ⚪ 无法评估（需现场补采 SMART）\n")
            for r in rows:
                if r[5] == "na":
                    fo.write("- %s%s %s — %s %s\n" % (r[0], ("/"+r[1] if r[1] else ""), r[2], r[3], r[4]))
        fo.write("\n## 逐站统计\n")
        fo.write("| 站点 | 磁盘 | 🔴 | 🟠 | ⚪ | ✅ |\n")
        fo.write("|------|------|----|----|----|----|\n")
        for site in sorted(site_stat):
            s = site_stat[site]
            fo.write("| %s | %d | %d | %d | %d | %d |\n" % (site, s["disks"], s["crit"], s["warn"], s["na"], s["ok"]))
    print("\n报告已写:", out)

if __name__ == "__main__":
    main()
