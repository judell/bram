#!/usr/bin/env python3
"""Refresh app/right/resources/xmlui-org-repos.json from the GitHub API.

Usage:
  python3 scripts/update_xmlui_org_repos.py

Optional auth:
  export GITHUB_TOKEN=...
  # or
  export GH_PAT=...
"""

from __future__ import annotations

import argparse
import json
import os
import sys
import time
import urllib.error
import urllib.parse
import urllib.request
from datetime import date
from pathlib import Path


DEFAULT_ORG = "xmlui-org"
DEFAULT_OUTPUT = Path("app/right/resources/xmlui-org-repos.json")
API_ROOT = "https://api.github.com"


def github_headers() -> dict[str, str]:
    headers = {
        "Accept": "application/vnd.github+json",
        "User-Agent": "xmlui-claude-code-desktop-repo-updater",
        "X-GitHub-Api-Version": "2022-11-28",
    }
    token = os.environ.get("GITHUB_TOKEN") or os.environ.get("GH_PAT")
    if token:
        headers["Authorization"] = f"Bearer {token}"
    return headers


def fetch_json(url: str) -> object:
    request = urllib.request.Request(url, headers=github_headers())
    try:
        with urllib.request.urlopen(request, timeout=30) as response:
            return json.load(response)
    except urllib.error.HTTPError as exc:
        detail = exc.read().decode("utf-8", errors="replace")
        raise RuntimeError(f"GitHub API error {exc.code} for {url}\n{detail}") from exc
    except urllib.error.URLError as exc:
        raise RuntimeError(f"Network error for {url}: {exc}") from exc


def fetch_all_repos(org: str) -> list[dict[str, object]]:
    repos: list[dict[str, object]] = []
    page = 1
    while True:
        query = urllib.parse.urlencode(
            {
                "per_page": 100,
                "page": page,
                "type": "public",
                "sort": "full_name",
            }
        )
        url = f"{API_ROOT}/orgs/{org}/repos?{query}"
        batch = fetch_json(url)
        if not isinstance(batch, list):
            raise RuntimeError(f"Unexpected repos payload for {url}: {type(batch).__name__}")
        if not batch:
            break
        repos.extend(batch)
        page += 1
    return repos


def fetch_latest_commit(org: str, repo_name: str, default_branch: str) -> dict[str, object]:
    query = urllib.parse.urlencode({"sha": default_branch, "per_page": 1})
    url = f"{API_ROOT}/repos/{org}/{repo_name}/commits?{query}"
    payload = fetch_json(url)
    if not isinstance(payload, list) or not payload:
        raise RuntimeError(f"No commits returned for {org}/{repo_name}")
    commit = payload[0]
    if not isinstance(commit, dict):
        raise RuntimeError(f"Unexpected commit payload for {org}/{repo_name}")
    return commit


def build_row(org: str, repo: dict[str, object]) -> dict[str, str]:
    repo_name = str(repo["name"])
    default_branch = str(repo.get("default_branch") or "main")
    commit = fetch_latest_commit(org, repo_name, default_branch)

    commit_block = commit.get("commit") or {}
    if not isinstance(commit_block, dict):
        commit_block = {}
    author_block = commit_block.get("author") or {}
    if not isinstance(author_block, dict):
        author_block = {}

    html_url = str(commit.get("html_url") or f"https://github.com/{org}/{repo_name}")
    author_name = str(author_block.get("name") or "")
    if commit.get("author") and isinstance(commit["author"], dict):
        author_name = str(commit["author"].get("login") or author_name)

    committed_at = str(author_block.get("date") or "")
    committed_date = committed_at[:10] if len(committed_at) >= 10 else ""

    message = str(commit_block.get("message") or "").strip()
    title = message.splitlines()[0] if message else ""

    return {
        "id": repo_name,
        "repo": repo_name,
        "repoUrl": str(repo.get("html_url") or f"https://github.com/{org}/{repo_name}"),
        "date": committed_date,
        "title": title,
        "author": author_name,
        "commitUrl": html_url,
    }


def write_snapshot(output_path: Path, rows: list[dict[str, str]]) -> None:
    payload = {
        "fetchedAt": date.today().isoformat(),
        "rows": rows,
    }
    output_path.parent.mkdir(parents=True, exist_ok=True)
    output_path.write_text(json.dumps(payload, indent=2) + "\n", encoding="utf-8")


def main() -> int:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--org", default=DEFAULT_ORG, help=f"GitHub org to query (default: {DEFAULT_ORG})")
    parser.add_argument(
        "--output",
        default=str(DEFAULT_OUTPUT),
        help=f"Output JSON path (default: {DEFAULT_OUTPUT})",
    )
    parser.add_argument(
        "--include-archived",
        action="store_true",
        help="Include archived repositories in the output.",
    )
    args = parser.parse_args()

    output_path = Path(args.output)
    repos = fetch_all_repos(args.org)
    repos = [repo for repo in repos if isinstance(repo, dict)]
    if not args.include_archived:
        repos = [repo for repo in repos if not repo.get("archived")]

    repos.sort(key=lambda repo: str(repo.get("name", "")).lower())

    rows: list[dict[str, str]] = []
    failures: list[str] = []
    for repo in repos:
        repo_name = str(repo.get("name") or "")
        if not repo_name:
            continue
        try:
            rows.append(build_row(args.org, repo))
        except Exception as exc:  # noqa: BLE001
            failures.append(f"{repo_name}: {exc}")
        time.sleep(0.05)

    write_snapshot(output_path, rows)

    print(f"Wrote {len(rows)} repos to {output_path}")
    if failures:
        print("\nSome repos failed:\n", file=sys.stderr)
        for failure in failures:
            print(f"  - {failure}", file=sys.stderr)
        return 1
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
