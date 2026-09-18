#!/usr/bin/env python3
"""Scan Drives INF files for VID+PID -> model name mappings.

Parses [Manufacturer] device lines like:
    %DeviceDesc% = Install, USB\VID_258A&PID_1007
resolves %DeviceDesc% via [Strings] (handles \\ continuations, quotes, comments),
and emits (vid, pid, model, class, file) where model is a real product name.

Used to build winsuitex-platform peripheral_db PID_MODEL seed table.
"""
import json
import os
import re

DRIVES = r"D:\jinglingResource\Drives"

hwid_re = re.compile(r"VID_([0-9A-Fa-f]{4})&PID_([0-9A-Fa-f]{4})")
# device line: optional %Token% or "Quoted" = InstallSection, HWID
devline_re = re.compile(r'^\s*("?)([^=,\r\n]+?)\1\s*=\s*[^,]+,\s*(\S+)\s*$')

# Generic OS / vendor placeholder names — NOT a real product model, drop them.
GENERIC = {
    "USB Audio Class 1.0 and 2.0 Device Driver",
    "C-Media Device Extension",
    "C-Media Audio Effects",
    "Fortemedia SAMSoft Device Extension",
    "USB Gaming Headset",
    "USB Advanced Audio Device",
    "SteelSeries DTS Headphone:X Device Extension",
    "USB Input Device",
}


def clean_value(val):
    """Strip quotes, trailing ;comment, and surrounding whitespace."""
    val = val.strip()
    if val.startswith('"') and '"' in val[1:]:
        val = val[1:val.rindex('"')]
    # cut at ';' comment
    if ";" in val:
        val = val[: val.index(";")]
    return val.strip()


def parse_strings(lines):
    """Return dict token->value from [Strings] sections (handles \\ continuations)."""
    strings = {}
    in_strings = False
    buf_key = None
    buf_val = ""
    for raw in lines:
        line = raw.rstrip("\n")
        stripped = line.strip()
        if stripped.startswith("["):
            # flush any pending continuation
            if buf_key is not None:
                strings[buf_key] = clean_value(buf_val)
                buf_key, buf_val = None, ""
            in_strings = stripped.lower().startswith("[strings")
            continue
        if not in_strings:
            continue
        if not stripped or stripped.startswith(";"):
            continue
        if buf_key is not None and line.rstrip().endswith("\\"):
            buf_val += stripped.rstrip("\\") + " "
            continue
        if "=" not in stripped:
            continue
        # flush previous
        if buf_key is not None:
            strings[buf_key] = clean_value(buf_val)
        key, _, val = stripped.partition("=")
        key = key.strip()
        val = clean_value(val)
        if line.rstrip().endswith("\\"):
            buf_key, buf_val = key, val + " "
        else:
            strings[key] = val
            buf_key, buf_val = None, ""
    if buf_key is not None:
        strings[buf_key] = clean_value(buf_val)
    return strings


def resolve_token(tok, strings):
    tok = tok.strip()
    if tok.startswith("%") and tok.endswith("%"):
        inner = tok[1:-1]
        return strings.get(inner, tok)
    return clean_value(tok)


def main():
    rows = []
    for root, _dirs, files in os.walk(DRIVES):
        for fn in files:
            if not fn.lower().endswith(".inf"):
                continue
            path = os.path.join(root, fn)
            try:
                with open(path, "r", encoding="utf-8", errors="ignore") as f:
                    lines = f.readlines()
            except Exception:
                continue
            cls = ""
            for ln in lines:
                m = re.search(r'^\s*Class\s*=\s*"?(.*?)"?\s*$', ln, re.I)
                if m:
                    cls = m.group(1).strip()
                    break
            strings = parse_strings(lines)
            for ln in lines:
                m = devline_re.match(ln)
                if not m:
                    continue
                tok = m.group(2)
                hwid = m.group(3)
                hm = hwid_re.search(hwid)
                if not hm:
                    continue
                vid = hm.group(1).upper()
                pid = hm.group(2).upper()
                model = resolve_token(tok, strings)
                if not model or model in GENERIC:
                    continue
                rows.append((vid, pid, model, cls, os.path.relpath(path, DRIVES)))

    # dedupe by (vid,pid,model)
    seen = set()
    out = []
    for r in rows:
        k = (r[0], r[1], r[2])
        if k in seen:
            continue
        seen.add(k)
        out.append(r)

    out.sort(key=lambda x: (x[0], x[1], x[2]))
    out_json = [
        {"vid": v, "pid": p, "model": m, "class": c, "file": f}
        for (v, p, m, c, f) in out
    ]
    with open(os.path.join(os.path.dirname(__file__), "inf_vidpid.json"), "w", encoding="utf-8") as jf:
        json.dump(out_json, jf, ensure_ascii=False, indent=2)
    print(f"TOTAL curated (vid,pid,model): {len(out)}  -> tools/inf_vidpid.json")
    for v, p, m, c, f in out:
        print(f"{v}:{p}  | {m!r}  | class={c}  | {f}")


if __name__ == "__main__":
    main()
