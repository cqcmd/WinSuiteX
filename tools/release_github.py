#!/usr/bin/env python3
# -*- coding: utf-8 -*-
"""
为 WinSuiteX 创建 GitHub Release 并上传分发包。

用法：
  GITHUB_TOKEN=ghp_xxx python tools/release_github.py \
      --tag v0.5.5.20261023 \
      --name "WinSuiteX GUI v0.5.5 (2026-10-23)" \
      --notes releases/WinSuiteX-v0.5.5-release-notes.md \
      --asset releases/WinSuiteX-GUI-v0.5.5.20261023.zip

token 从环境变量 GITHUB_TOKEN 读取（不落盘到仓库）。
"""
import os
import sys
import json
import urllib.request
import urllib.error
import urllib.parse

API = "https://api.github.com"
UPLOADS = "https://uploads.github.com"
REPO = "cqcmd/WinSuiteX"


def _req(method, url, token, data=None, headers=None, binary=False):
    h = {
        "Authorization": f"Bearer {token}",
        "Accept": "application/vnd.github+json",
        "User-Agent": "WinSuiteX-release-script",
    }
    if headers:
        h.update(headers)
    if isinstance(data, str):
        data = data.encode("utf-8")
    req = urllib.request.Request(url, data=data, headers=h, method=method)
    try:
        with urllib.request.urlopen(req, timeout=120) as r:
            body = r.read()
            return r.status, body
    except urllib.error.HTTPError as e:
        print(f"[HTTPError] {e.code} {e.reason}", file=sys.stderr)
        try:
            print(e.read().decode("utf-8", "replace"), file=sys.stderr)
        except Exception:
            pass
        raise


def create_release(token, tag, name, notes_md):
    with open(notes_md, "r", encoding="utf-8") as f:
        body = f.read()
    payload = {
        "tag_name": tag,
        "name": name,
        "body": body,
        "draft": False,
        "prerelease": False,
    }
    status, resp = _req(
        "POST",
        f"{API}/repos/{REPO}/releases",
        token,
        data=json.dumps(payload),
        headers={"Content-Type": "application/json"},
    )
    if status != 201:
        raise SystemExit(f"创建 Release 失败：HTTP {status}")
    rel = json.loads(resp.decode("utf-8"))
    print(f"[ok] release created: {rel['html_url']}")
    return rel["id"], rel["upload_url"].split("{")[0]


def upload_asset(token, upload_base, asset_path):
    fn = os.path.basename(asset_path)
    with open(asset_path, "rb") as f:
        data = f.read()
    url = f"{upload_base}?name={urllib.parse.quote(fn)}"
    status, resp = _req(
        "POST", url, token, data=data,
        headers={"Content-Type": "application/zip"},
    )
    if status != 201:
        raise SystemExit(f"上传资产失败：HTTP {status}")
    a = json.loads(resp.decode("utf-8"))
    print(f"[ok] asset uploaded: {a['browser_download_url']}")
    return a["browser_download_url"]


def main():
    import argparse
    import urllib.parse
    ap = argparse.ArgumentParser()
    ap.add_argument("--tag", required=True)
    ap.add_argument("--name", required=True)
    ap.add_argument("--notes", required=True)
    ap.add_argument("--asset", required=True)
    args = ap.parse_args()

    token = os.environ.get("GITHUB_TOKEN")
    if not token:
        raise SystemExit("缺少环境变量 GITHUB_TOKEN")

    rid, upload_base = create_release(token, args.tag, args.name, args.notes)
    upload_asset(token, upload_base, args.asset)
    print("RELEASE_DONE")


if __name__ == "__main__":
    main()
