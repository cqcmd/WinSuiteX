#!/usr/bin/env python3
# -*- coding: utf-8 -*-
"""github.com:443 不通时的应急推送：经 api.github.com 走 Git Data API
逐个重建 blob/tree/commit 并 fast-forward 更新 refs/heads/main（等价 git push）。

用法：
  python tools/gh_api_push.py <远端当前HEAD父提交> <待推提交sha>...

远端当前 HEAD 用 `git ls-remote origin main`（不通时改查 api /repos/.../git/ref/heads/main）
或上一次推送回显获取。要求待推提交链从该提交线性向后，且工作区干净。

注意：
- GitHub 重建的提交对象与本地 SHA 一致（author/message/date 全量透传），
  推完 git update-ref refs/remotes/origin/main <新sha> 即完成同步。
- 本地新树对象 GitHub 不认识，base_tree 必须取**父提交**的树（show -s --format=%T）。
- 2026-10-27 首战：GPU 无驱动识别两个提交经此通道推送成功。
"""
import base64
import json
import os
import subprocess
import sys

REPO = "cqcmd/WinSuiteX"


def load_token() -> str:
    env = os.environ.get("GITHUB_PERSONAL_ACCESS_TOKEN")
    if env:
        return env
    mcp = json.load(open(os.path.expanduser("~/.workbuddy/mcp.json"), encoding="utf-8"))
    for name, cfg in mcp.get("mcpServers", {}).items():
        if "github" in name.lower():
            t = cfg.get("env", {}).get("GITHUB_PERSONAL_ACCESS_TOKEN", "")
            if t:
                return t
    raise SystemExit("找不到 GitHub token（环境变量或 ~/.workbuddy/mcp.json）")


TOKEN = load_token()
API = f"https://api.github.com/repos/{REPO}"
HDR = ["-H", f"Authorization: Bearer {TOKEN}", "-H", "Accept: application/vnd.github+json"]


def api(method, path, payload=None):
    args = ["curl", "-sS", "--ssl-no-revoke", "-X", method, *HDR]
    pf = None
    if payload is not None:
        pf = f"_api_payload.json"
        with open(pf, "w", encoding="utf-8") as f:
            json.dump(payload, f, ensure_ascii=False)
        args += ["-d", f"@{pf}"]
    args.append(f"{API}{path}")
    out = subprocess.run(args, capture_output=True, text=True, encoding="utf-8")
    if pf:
        os.remove(pf)
    try:
        return json.loads(out.stdout)
    except json.JSONDecodeError:
        raise SystemExit(f"[api] {method} {path} 非 JSON 响应: {out.stdout[:300]} {out.stderr[:200]}")


def git(*args):
    return subprocess.run(["git", *args], capture_output=True, check=True, text=True,
                          encoding="utf-8").stdout


def main(parent, commits):
    for sha in commits:
        meta = git("log", "-1", "--format=%B%x01%an%x01%ae%x01%aI", sha).rstrip("\n").split("\x01")
        message, an, ae, ad = meta[0], meta[1], meta[2], meta[3]
        # base_tree 必须是 GitHub 已知的对象 = **父提交**的树（本地新树 GitHub 不认识）
        base_tree = git("show", "-s", "--format=%T", f"{sha}^").strip()
        # 变更清单：非 tty 下 diff-tree 直出 "STATUS\tpath"（首行是 commit id）
        changes = []
        for line in git("diff-tree", "-r", "--no-renames", "--name-status", sha).strip().splitlines():
            if "\t" not in line or line.startswith(sha):
                continue
            status, path = line.split("\t", 1)
            changes.append((status, path))
        entries = []
        for status, path in changes:
            if status == "D":
                continue  # 从 tree 省略 = 删除
            mode_line = git("ls-tree", sha, "--", path).strip()
            if not mode_line:
                raise SystemExit(f"[git] {sha} 里找不到 {path}")
            mode = mode_line.split()[0]
            content = subprocess.run(["git", "show", f"{sha}:{path}"], capture_output=True,
                                     check=True).stdout
            blob = api("POST", "/git/blobs", {
                "content": base64.b64encode(content).decode(), "encoding": "base64"})
            if "sha" not in blob:
                raise SystemExit(f"[api] blob 失败 {path}: {blob}")
            entries.append({"path": path, "mode": mode, "type": "blob", "sha": blob["sha"]})
        tree = api("POST", "/git/trees", {"base_tree": base_tree, "tree": entries})
        if "sha" not in tree:
            raise SystemExit(f"[api] tree 失败: {tree}")
        commit = api("POST", "/git/commits", {
            "message": message, "tree": tree["sha"], "parents": [parent],
            "author": {"name": an, "email": ae, "date": ad}})
        if "sha" not in commit:
            raise SystemExit(f"[api] commit 失败: {commit}")
        ref = api("PATCH", "/git/refs/heads/main", {"sha": commit["sha"], "force": False})
        if ref.get("object", {}).get("sha") != commit["sha"]:
            raise SystemExit(f"[api] ref 更新失败（可能非 fast-forward）: {ref}")
        print(f"[api-push] {sha[:7]} → {commit['sha'][:7]}  {message.splitlines()[0][:40]}")
        parent = commit["sha"]
    print(f"REMOTE_MAIN={parent}")


if __name__ == "__main__":
    if len(sys.argv) < 3:
        raise SystemExit(__doc__)
    main(sys.argv[1], sys.argv[2:])
