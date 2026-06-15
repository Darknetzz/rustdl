#!/usr/bin/env python3
"""GitHub push webhook listener for rolling dev releases (no GitHub Actions).

Run on a trusted host that can build rustdl and run ``gh`` (authenticated).
Point a GitHub repo webhook (Push events, JSON) at this server.

Environment:
  RUSTDL_WEBHOOK_SECRET   Required. Same secret as configured on GitHub.
  RUSTDL_REPO_ROOT        Git checkout (default: parent of scripts/).
  RUSTDL_GITHUB_REMOTE    Remote to fetch (default: github).
  RUSTDL_WEBHOOK_PORT     Listen port (default: 8766).
  RUSTDL_WEBHOOK_PATH     URL path (default: /rustdl-dev-release).

Example (repo root, after ``gh auth login``):
  export RUSTDL_WEBHOOK_SECRET='…'
  python scripts/dev_release_webhook.py

GitHub → Settings → Webhooks → Add:
  Payload URL: https://your-host:8766/rustdl-dev-release
  Content type: application/json
  Secret: (same as RUSTDL_WEBHOOK_SECRET)
  Events: Just the push event
"""

from __future__ import annotations

import hashlib
import hmac
import json
import os
import subprocess
import sys
from http.server import BaseHTTPRequestHandler, HTTPServer
from pathlib import Path

SCRIPTS = Path(__file__).resolve().parent
REPO_ROOT = Path(os.environ.get("RUSTDL_REPO_ROOT", SCRIPTS.parent)).resolve()
SECRET = os.environ.get("RUSTDL_WEBHOOK_SECRET", "")
GITHUB_REMOTE = os.environ.get("RUSTDL_GITHUB_REMOTE", "github")
PORT = int(os.environ.get("RUSTDL_WEBHOOK_PORT", "8766"))
PATH = os.environ.get("RUSTDL_WEBHOOK_PATH", "/rustdl-dev-release")


def log(msg: str) -> None:
    print(msg, flush=True)


def verify_signature(body: bytes, header: str | None) -> bool:
    if not SECRET:
        log("RUSTDL_WEBHOOK_SECRET is not set")
        return False
    if not header or not header.startswith("sha256="):
        return False
    expected = hmac.new(SECRET.encode(), body, hashlib.sha256).hexdigest()
    got = header.removeprefix("sha256=")
    return hmac.compare_digest(expected, got)


def publish_dev(after_sha: str) -> None:
    publish_sh = SCRIPTS / "publish_dev_release.sh"
    publish_ps1 = SCRIPTS / "publish_dev_release.ps1"
    if sys.platform == "win32":
        script = publish_ps1
        cmd = [
            "powershell",
            "-NoProfile",
            "-ExecutionPolicy",
            "Bypass",
            "-File",
            str(script),
            "-Commit",
            after_sha,
        ]
    else:
        script = publish_sh
        cmd = [str(script), "--commit", after_sha]

    if not script.is_file():
        raise FileNotFoundError(script)

    log(f"Fetching {GITHUB_REMOTE} dev …")
    subprocess.run(
        ["git", "fetch", GITHUB_REMOTE, "dev"],
        cwd=REPO_ROOT,
        check=True,
    )
    subprocess.run(
        ["git", "checkout", "--detach", after_sha],
        cwd=REPO_ROOT,
        check=True,
    )

    log(f"Publishing rolling dev release for {after_sha[:7]} …")
    subprocess.run(cmd, cwd=REPO_ROOT, check=True)

    stable_ps1 = SCRIPTS / "publish_stable_release.ps1"
    stable_sh = SCRIPTS / "publish_stable_release.sh"
    if sys.platform == "win32":
        stable_script = stable_ps1
        stable_cmd = [
            "powershell",
            "-NoProfile",
            "-ExecutionPolicy",
            "Bypass",
            "-File",
            str(stable_script),
            "-Commit",
            after_sha,
            "-SkipBuild",
        ]
    else:
        stable_script = stable_sh
        stable_cmd = [str(stable_script), "--commit", after_sha, "--skip-build"]

    if stable_script.is_file():
        log(f"Publishing stable release (if new) for {after_sha[:7]} …")
        subprocess.run(stable_cmd, cwd=REPO_ROOT, check=True)

    subprocess.run(
        ["git", "checkout", "dev"],
        cwd=REPO_ROOT,
        check=False,
    )


class Handler(BaseHTTPRequestHandler):
    def log_message(self, fmt: str, *args) -> None:
        log(f"{self.address_string()} - {fmt % args}")

    def do_POST(self) -> None:
        if self.path.split("?", 1)[0] != PATH:
            self.send_error(404)
            return

        length = int(self.headers.get("Content-Length", "0"))
        body = self.rfile.read(length)
        if not verify_signature(body, self.headers.get("X-Hub-Signature-256")):
            self.send_error(401, "Invalid webhook signature")
            return

        try:
            payload = json.loads(body.decode("utf-8"))
        except json.JSONDecodeError:
            self.send_error(400, "Invalid JSON")
            return

        event = self.headers.get("X-GitHub-Event", "")
        if event == "ping":
            self.send_response(200)
            self.end_headers()
            self.wfile.write(b"pong")
            return

        if event != "push":
            self.send_response(204)
            self.end_headers()
            return

        ref = payload.get("ref", "")
        if ref != "refs/heads/dev":
            log(f"Ignoring push to {ref}")
            self.send_response(204)
            self.end_headers()
            return

        after = payload.get("after")
        if not after or after == ("0" * 40):
            self.send_response(204)
            self.end_headers()
            return

        try:
            publish_dev(after)
        except subprocess.CalledProcessError as exc:
            log(f"Publish failed: {exc}")
            self.send_error(500, "Publish failed")
            return
        except OSError as exc:
            log(f"Publish error: {exc}")
            self.send_error(500, str(exc))
            return

        self.send_response(200)
        self.end_headers()
        self.wfile.write(b"ok")

    def do_GET(self) -> None:
        if self.path.split("?", 1)[0] == PATH:
            self.send_response(200)
            self.end_headers()
            self.wfile.write(b"rustdl dev release webhook\n")
            return
        self.send_error(404)


def main() -> None:
    if not SECRET:
        sys.exit("Set RUSTDL_WEBHOOK_SECRET before starting the webhook listener.")

    if not (REPO_ROOT / ".git").exists():
        sys.exit(f"Not a git repo: {REPO_ROOT}")

    server = HTTPServer(("0.0.0.0", PORT), Handler)
    log(f"Listening on http://0.0.0.0:{PORT}{PATH}")
    log(f"Repo: {REPO_ROOT}")
    try:
        server.serve_forever()
    except KeyboardInterrupt:
        log("Stopped.")


if __name__ == "__main__":
    main()
