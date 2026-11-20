"""Publishes a Kite release that was already built and prepared: py release/publish.py v0.1.0

Expects, done beforehand:
- main at the release commit and the tag (e.g. v0.1.0) created locally,
- release/<tag>/ holding the installer, its .sig, latest.json and notes.md.

Creates the public GitHub repo if it doesn't exist yet, pushes main and the tag, creates the
release with the Git Credential Manager token (never printed or saved), uploads the files, and
checks that the updater sees the new version.
"""
import json
import os
import subprocess
import sys
import urllib.error
import urllib.request
from pathlib import Path

OWNER, NAME = "rockslol", "kite"
REPO = f"{OWNER}/{NAME}"
ROOT = Path(__file__).resolve().parent.parent
TAG = sys.argv[1]
VERSION = TAG.lstrip("v")
DIR = ROOT / "release" / TAG
ENV = {**os.environ, "GIT_TERMINAL_PROMPT": "0", "GCM_INTERACTIVE": "never"}
DESCRIPTION = "A small, open-source music player for YouTube Music, YouTube, SoundCloud and local files."


def git(*args):
    r = subprocess.run(["git", "-C", str(ROOT), *args], capture_output=True, text=True, env=ENV, timeout=300)
    if r.returncode:
        raise RuntimeError(f"git {' '.join(args)}: {r.stderr.strip() or r.stdout.strip()}")
    return r.stdout.strip()


def api(url, token, data=None, ctype="application/json", method=None):
    req = urllib.request.Request(url, data=data, method=method or ("POST" if data is not None else "GET"), headers={
        "Authorization": f"Bearer {token}", "Accept": "application/vnd.github+json", "User-Agent": "kite-release",
        **({"Content-Type": ctype} if data is not None else {})})
    with urllib.request.urlopen(req, timeout=600) as r:
        return json.loads(r.read() or b"null")


def main():
    files = [f"Kite_{VERSION}_x64-setup.exe", f"Kite_{VERSION}_x64-setup.exe.sig", "latest.json"]
    missing = [f for f in files + ["notes.md"] if not (DIR / f).exists()]
    if missing:
        raise RuntimeError(f"missing in {DIR}: {', '.join(missing)}")
    if git("rev-parse", "main") != git("rev-parse", f"{TAG}^{{commit}}"):
        raise RuntimeError(f"main isn't at {TAG}")
    if json.loads((DIR / "latest.json").read_text(encoding="utf-8"))["version"] != VERSION:
        raise RuntimeError("latest.json has a different version")

    cred = subprocess.run(["git", "credential", "fill"], input="protocol=https\nhost=github.com\n\n",
                          capture_output=True, text=True, env=ENV, timeout=60).stdout
    token = dict(line.split("=", 1) for line in cred.splitlines() if "=" in line).get("password")
    if not token:
        raise RuntimeError("no GitHub login in Git Credential Manager")

    try:
        api(f"https://api.github.com/repos/{REPO}", token)
        print(f"repo {REPO} exists")
    except urllib.error.HTTPError as e:
        if e.code != 404:
            raise
        api("https://api.github.com/user/repos", token, json.dumps({
            "name": NAME, "description": DESCRIPTION, "private": False, "has_wiki": False}).encode())
        print(f"created public repo {REPO}")

    remotes = git("remote").split()
    url = f"https://github.com/{REPO}.git"
    if "origin" not in remotes:
        git("remote", "add", "origin", url)
    print("pushing main and the tag")
    git("push", "-u", "origin", "main")
    git("push", "origin", TAG)

    rel = api(f"https://api.github.com/repos/{REPO}/releases", token, json.dumps({
        "tag_name": TAG, "name": f"Kite {VERSION}", "body": (DIR / "notes.md").read_text(encoding="utf-8"),
        "draft": False, "prerelease": False, "make_latest": "true"}).encode())
    print(f"release created: {rel['html_url']}")
    upload = rel["upload_url"].split("{")[0]
    for name in files:
        a = api(f"{upload}?name={name}", token, (DIR / name).read_bytes(), "application/octet-stream")
        print(f"uploaded {a['name']} ({a['size']} bytes)")

    with urllib.request.urlopen(f"https://github.com/{REPO}/releases/latest/download/latest.json", timeout=60) as r:
        seen = json.loads(r.read())["version"]
    print(f"the updater now sees {seen}")
    print(f"done: https://github.com/{REPO}")


if __name__ == "__main__":
    try:
        main()
    except Exception as e:
        print(f"FAILED: {e}")
        sys.exit(1)
