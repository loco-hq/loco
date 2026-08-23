#!/usr/bin/env python3
"""Mint a 1-hour GitHub App installation token.

Credentials: ~/.config/loco-hq/apps/loco-{vendor}.{json,pem}

  export GH_TOKEN=$(python3 scripts/agent-github/token.py grok)
  eval "$(python3 scripts/agent-github/token.py env claude)"
"""

from __future__ import annotations

import base64
import json
import os
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


def main() -> None:
    args = sys.argv[1:]
    if not args or args[0] in {"-h", "--help"}:
        print(__doc__.strip(), file=sys.stderr)
        raise SystemExit(2)
    if args[0] == "env":
        if len(args) != 2:
            die("usage: token.py env grok|claude")
        meta, _ = load(args[1])
        print(f"export GH_TOKEN={token_for(args[1])}")
        print(f"export GIT_AUTHOR_NAME='{meta['bot_login']}'")
        print(f"export GIT_AUTHOR_EMAIL='{meta['bot_email']}'")
        print(f"export GIT_COMMITTER_NAME='{meta['bot_login']}'")
        print(f"export GIT_COMMITTER_EMAIL='{meta['bot_email']}'")
        return
    if len(args) != 1:
        die("usage: token.py grok|claude")
    sys.stdout.write(token_for(args[0]) + "\n")


if __name__ == "__main__":
    main()
