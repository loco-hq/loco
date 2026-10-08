#!/usr/bin/env python3
"""Mint a 1-hour GitHub App installation token.

Credentials: ~/.config/loco-hq/apps/loco-{vendor}.{json,pem}

  export GH_TOKEN=$(python3 scripts/agent-github/token.py grok)
  eval "$(python3 scripts/agent-github/token.py env claude)"
  python3 scripts/agent-github/token.py git-credential claude get   # git calls this

`env` exports GH_TOKEN, the bot as git author and committer, and git config
through GIT_CONFIG_COUNT / GIT_CONFIG_KEY_n / GIT_CONFIG_VALUE_n, so that in
that shell, and nowhere else, `git push` to github.com goes over HTTPS as the
App: pushInsteadOf rewrites git@github.com: and ssh://git@github.com/ to
https://github.com/ for pushes only (fetches stay on SSH), inherited github.com
credential helpers such as osxkeychain are cleared, and `git-credential` is the
one helper left. It answers `get` for github.com with a cached token, re-minted
near expiry, so a push still works after GH_TOKEN's hour. Entries are appended
after any GIT_CONFIG_COUNT the shell already has, and GIT_TERMINAL_PROMPT=0
makes a failing helper fail the push instead of prompting. Nothing is written
to a git config file or the keychain.
"""

from __future__ import annotations

import base64
import json
import os
import shlex
import subprocess
import sys
import time
import urllib.error
import urllib.request
from datetime import datetime, timezone
from pathlib import Path

CONFIG = Path.home() / ".config" / "loco-hq" / "apps"
VENDORS = {"grok": "loco-grok", "claude": "loco-claude"}


def die(msg: str) -> None:
    print(msg, file=sys.stderr)
    raise SystemExit(1)


def b64url(data: bytes) -> str:
    return base64.urlsafe_b64encode(data).rstrip(b"=").decode()


def app_jwt(app_id: int, pem: Path) -> str:
    now = int(time.time())
    header = b64url(b'{"alg":"RS256","typ":"JWT"}')
    payload = b64url(json.dumps({"iat": now - 60, "exp": now + 540, "iss": app_id}, separators=(",", ":")).encode())
    signing_input = f"{header}.{payload}"
    proc = subprocess.run(
        ["openssl", "dgst", "-sha256", "-sign", str(pem)],
        input=signing_input.encode(),
        capture_output=True,
        check=False,
    )
    if proc.returncode != 0:
        die(proc.stderr.decode().strip() or "openssl sign failed")
    return f"{signing_input}.{b64url(proc.stdout)}"


def api(method: str, url: str, token: str, body: dict | None = None) -> dict:
    data = None if body is None else json.dumps(body).encode()
    headers = {
        "Accept": "application/vnd.github+json",
        "Authorization": f"Bearer {token}",
        "User-Agent": "loco-hq-token",
        "X-GitHub-Api-Version": "2022-11-28",
    }
    if data is not None:
        headers["Content-Type"] = "application/json"
    req = urllib.request.Request(url, data=data, headers=headers, method=method)
    try:
        with urllib.request.urlopen(req) as resp:
            raw = resp.read()
            return json.loads(raw) if raw else {}
    except urllib.error.HTTPError as e:
        die(f"{method} {url} -> {e.code}: {e.read().decode()}")


def load(vendor: str) -> tuple[dict, Path]:
    slug = VENDORS.get(vendor)
    if not slug:
        die(f"unknown vendor {vendor}; choose grok or claude")
    meta_path = CONFIG / f"{slug}.json"
    pem_path = CONFIG / f"{slug}.pem"
    if not meta_path.is_file() or not pem_path.is_file():
        die(f"missing {meta_path} or {pem_path}")
    return json.loads(meta_path.read_text()), pem_path


def token_for(vendor: str) -> str:
    meta, pem = load(vendor)
    cache = CONFIG / f"{VENDORS[vendor]}.token"
    if cache.is_file():
        cached = json.loads(cache.read_text())
        exp = datetime.strptime(cached["expires_at"], "%Y-%m-%dT%H:%M:%SZ").replace(tzinfo=timezone.utc)
        if exp.timestamp() - time.time() > 300:
            return cached["token"]
    jwt = app_jwt(meta["app_id"], pem)
    body = api(
        "POST",
        f"https://api.github.com/app/installations/{meta['installation_id']}/access_tokens",
        jwt,
    )
    cache.write_text(json.dumps({"token": body["token"], "expires_at": body["expires_at"]}) + "\n")
    os.chmod(cache, 0o600)
    return body["token"]


def git_config_env(vendor: str) -> list[tuple[str, str]]:
    helper = f"!python3 {shlex.quote(str(Path(__file__).resolve()))} git-credential {vendor}"
    return [
        ("url.https://github.com/.pushInsteadOf", "git@github.com:"),
        ("url.https://github.com/.pushInsteadOf", "ssh://git@github.com/"),
        ("credential.https://github.com.helper", ""),
        ("credential.https://github.com.helper", helper),
    ]


def git_credential(vendor: str, op: str) -> None:
    """git credential helper protocol: answer `get` for github.com, ignore the rest."""
    attrs = {}
    for line in sys.stdin:
        line = line.rstrip("\n")
        if not line:
            break
        key, _, value = line.partition("=")
        attrs[key] = value
    if op != "get" or attrs.get("host") != "github.com" or attrs.get("protocol", "https") != "https":
        return
    sys.stdout.write(f"username=x-access-token\npassword={token_for(vendor)}\n")


def main() -> None:
    args = sys.argv[1:]
    if not args or args[0] in {"-h", "--help"}:
        print(__doc__.strip(), file=sys.stderr)
        raise SystemExit(2)
    if args[0] == "env":
        if len(args) != 2:
            die("usage: token.py env grok|claude")
        meta, _ = load(args[1])
        exports = [
            ("GH_TOKEN", token_for(args[1])),
            ("GIT_AUTHOR_NAME", meta["bot_login"]),
            ("GIT_AUTHOR_EMAIL", meta["bot_email"]),
            ("GIT_COMMITTER_NAME", meta["bot_login"]),
            ("GIT_COMMITTER_EMAIL", meta["bot_email"]),
            ("GIT_TERMINAL_PROMPT", "0"),
        ]
        config = git_config_env(args[1])
        base = os.environ.get("GIT_CONFIG_COUNT", "0")
        if not base.isdigit():
            die(f"GIT_CONFIG_COUNT is {base!r}, not a count")
        base = int(base)
        exports.append(("GIT_CONFIG_COUNT", str(base + len(config))))
        for i, (key, value) in enumerate(config, start=base):
            exports.append((f"GIT_CONFIG_KEY_{i}", key))
            exports.append((f"GIT_CONFIG_VALUE_{i}", value))
        for name, value in exports:
            print(f"export {name}={shlex.quote(value)}")
        return
    if args[0] == "git-credential":
        if len(args) != 3:
            die("usage: token.py git-credential grok|claude get|store|erase")
        load(args[1])
        git_credential(args[1], args[2])
        return
    if len(args) != 1:
        die("usage: token.py grok|claude")
    sys.stdout.write(token_for(args[0]) + "\n")


if __name__ == "__main__":
    main()
