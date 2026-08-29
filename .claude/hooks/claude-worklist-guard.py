#!/usr/bin/env python3
"""PreToolUse hook: enforce the worklist flow on this project.

Three write surfaces, registered via CLAUDE_GUARD_MATCHERS in
src-tauri/src/lib.rs — the registration is what makes each branch below
reachable, and a branch whose matcher is missing is dead code that still
reads as coverage (judell/bram#261):

- `Write` / `Edit` — the canonical Claude edit tools.
- `Bash` — the agent can `sed -i` / `tee` / `python -c` / heredoc around
  the Write/Edit gate, so mutating commands need coverage too. Also
  where the #176 `gh --body @` check and the cross-boundary signature
  check live.
- `mcp__*` — a configured filesystem MCP server exposes write_file /
  edit_file / move_file, which would otherwise bypass Write/Edit
  entirely.

Two responsibilities:

1. Writes/Edits to resources/worklist.json — validate that any removed item
   was authorized. status="applied" items can be pruned freely (commit-then-
   prune); status="proposed" items can only be pruned when the last user
   message was `drop: {"ids":[...]}` listing that id.

2. Writes/Edits to any OTHER file in the project — require the target file
   to be covered by a proposed/applied item in resources/worklist.json, OR
   a fresh direct-edit bypass record in resources/.worklist-authorization.json,
   OR the single explicit opt-out phrase "just do it" in the last user
   message. Mirrors the coverage check the codex-side hook
   (app/provider-hooks/codex-worklist-guard.py) does for apply_patch.

If the project lacks resources/.worklist-authorization.json (the managed-repo
marker Bram Setup writes), the hook exits 0 (allow) — Claude
sessions in unmanaged repos run as if no hook were installed.
"""

import hashlib
import json
import os
import re
import subprocess
import sys

NL = chr(10)
import time
import urllib.request
from datetime import datetime, timezone


WORKLIST_REL = "resources/worklist.json"
WORKLIST_DRAFTS_PREFIX = "resources/worklist-drafts/"
AUTH_REL = "resources/.worklist-authorization.json"
BYPASS_TTL_SECONDS = 60 * 60  # direct-edit auth records are fresh for 1h


_LIFECYCLE_PATHS_EXACT = {
    "resources/worklist.json",
    "resources/.worklist-authorization.json",
    "resources/.inflight-claim.json",
    "resources/.pty-intent.jsonl",
    "resources/.worklist-intent.json",
    "resources/.worklist-result.json",
    "resources/.bram-port",
    "resources/.bram-port.json",
}
_LIFECYCLE_PATHS_PREFIXES = (
    "resources/worklist-drafts/",
    "resources/worklist-citations/",
    "resources/feedback-drafts/",
    "resources/feedback-history/",
    "resources/bram-traces/",
)


def is_lifecycle_path(rel):
    """True iff rel names a pure-coordination path whose writes are
    implicitly authorized by the worklist lifecycle."""
    if not isinstance(rel, str) or not rel:
        return False
    if rel in _LIFECYCLE_PATHS_EXACT:
        return True
    return any(rel.startswith(p) for p in _LIFECYCLE_PATHS_PREFIXES)


def emit_allow_for_lifecycle(target_rel, reason="bram-lifecycle-channel"):
    """Emit Claude's PreToolUse 'allow' decision so the user is not
    prompted for permission on lifecycle bookkeeping writes.
    Reference: https://docs.claude.com/en/docs/claude-code/hooks#pretooluse-decision-control
    """
    _trace_hook(
        "PreToolUse",
        os.environ.get("__BRAM_TRACE_TOOL", "Write"),
        target_rel,
        "allow",
        reason,
    )
    output = {
        "hookSpecificOutput": {
            "hookEventName": "PreToolUse",
            "permissionDecision": "allow",
            "permissionDecisionReason": (
                f"Bram lifecycle channel ({target_rel}): implicitly authorized "
                f"by the worklist flow, no per-write confirmation needed."
            ),
        }
    }
    print(json.dumps(output))
    sys.exit(0)


# Issue #176: detect the `gh ... --body @<...>` antipattern. `gh`'s --body
# takes a literal string, but `@-` and `@path` look like stdin/file refs
# (curl convention) and silently stash the literal text — comments render
# as a placeholder smiley. Deny before the malformed command runs.
_GH_BODY_AT_RX = re.compile(
    r"(?:^|\s|;|&&|\|\||\|)gh\s.*?--body\s+@\S",
    re.MULTILINE,
)


def is_gh_body_at_antipattern(command):
    if not isinstance(command, str):
        return False
    return bool(_GH_BODY_AT_RX.search(command))


# Named separately from _BASH_WRITE_PATTERNS so forge_issue_only_write() can
# skip this one entry by identity when checking whether an issue-only forge
# command ALSO matches some other write pattern (e.g. a chained `git commit`).
_GH_ISSUE_WRITE_RX = re.compile(
    r"(^|[\s;&|`(])gh\s+issue\s+(close|reopen|edit|comment|create|delete|transfer|pin|unpin|lock|unlock)\b"
)

# judell/bram#283: post-commit push grace. `worklist-commit` prunes items on
# success, so "commit, then push" self-defeats -- the emptied board denies
# the follow-up `git push` at the Bash coverage gate below. This classifier
# recognizes push-shaped commands (`git push`, `git -C <path> push`, and
# either form chained after `&&`/`;`) so the grace predicate can allow them
# in a short post-commit window. `\b` after `push` keeps `pushd` out.
_PUSH_CMD_RX = re.compile(r"(^|[\s;&|`(])git\s+(-C\s+\S+\s+)?push\b")

# The redirect entry needs extra logic (see _pattern_is_write /
# bash-write-nonrepo-target below), so it's named rather than inlined below.
# #299: this regex is now only the identity sentinel for the redirect entry in
# _BASH_WRITE_PATTERNS -- the actual scanning is done by the quote-aware
# _redirect_targets() below, never by this pattern's .search().
_REDIRECT_WRITE_RX = re.compile(r"(^|[\s;&|`(])>+\s*[^\s>&]")  # > file or >> file

# #299: named so _pattern_is_write can dispatch it to the content test in
# python_dash_c_writes() instead of treating the mere presence of `python -c`
# as a write.
_PYTHON_DASH_C_RX = re.compile(r"(^|[\s;&|`(])python[0-9.]*\s+-c\b")

# Bash commands we deny without worklist coverage. This intentionally matches
# the Codex guard's H3-level classifier: broad enough to stop common write and
# external side-effect bypasses, but narrow enough to keep read-only shell
# investigation usable.
_BASH_WRITE_PATTERNS = [
    _REDIRECT_WRITE_RX,
    re.compile(r"(^|[\s;&|`(])tee\b"),                # tee
    re.compile(r"(^|[\s;&|`(])sed\s+[^|;&]*-i\b"),    # sed -i
    re.compile(r"(^|[\s;&|`(])perl\s+[^|;&]*-i\b"),   # perl -i
    re.compile(r"(^|[\s;&|`(])(rm|mv|cp|truncate|install)\b"),
    re.compile(r"(^|[\s;&|`(])git\s+(add|commit|push|rm|mv|reset|checkout|restore|stash|am|apply|cherry-pick|rebase|revert|tag|branch)\b"),
    _GH_ISSUE_WRITE_RX,
    re.compile(r"open\s*\(\s*['\"][^'\"]+['\"]\s*,\s*['\"][wax]"),
    _PYTHON_DASH_C_RX,  # #299: dispatched to python_dash_c_writes()
    re.compile(r"(^|[\s;&|`(])node\s+-e\b"),
    re.compile(r"(^|[\s;&|`(])bash\s+-c\b"),
    re.compile(r"(^|[\s;&|`(])sh\s+-c\b"),
]

# Redirect targets that are not a repository mutation: no-op sinks and temp
# scratch space, not the repo (guard-write-detection-false-positives). Other
# /dev/* targets (stdout, fd/*) are genuine writes and stay covered -- only
# these two exact sinks and the two temp prefixes are excluded.
_NONREPO_REDIRECT_EXACT = {"/dev/null", "/dev/zero", "/tmp", "/private/tmp"}
_NONREPO_REDIRECT_PREFIXES = ("/tmp/", "/private/tmp/")

# #299: `(^|[\s;&|`(])` -- the command-position boundary the old
# _REDIRECT_TARGET_RX opened with, as a character test the scanner can use.
_BOUNDARY_CHARS = " \t\n\r\x0b\x0c;&|`("


def _at_boundary(command, i):
    return i == 0 or command[i - 1] in _BOUNDARY_CHARS


def _strip_matching_quotes(s):
    if len(s) >= 2 and s[0] == s[-1] and s[0] in ("'", '"'):
        return s[1:-1]
    return s


def _redirect_targets(command):
    """Every `> x` / `>> x` redirect target in command, quotes stripped.

    #299 (case 3): the former regex (`_REDIRECT_TARGET_RX`) was quote-blind, so
    `awk '$1 >= "2026-08-28"'` and `--jq '.[]|select(.number > 200)'` read as
    shell redirects and were denied `bash-write-no-coverage`. This scanner
    tracks single- and double-quote state (and backslash escapes) and only
    reports redirects that are actually unquoted. A `>` inside quotes is a
    comparison operator, JSON, or jq syntax -- never a redirect. A quoted
    target (`> "my file.txt"`) is consumed to the matching quote so quote state
    stays coherent for the remainder of the command. Ported from the
    differential-verified Rust shadow (`redirect_targets`, guard_policy.rs)."""
    if not isinstance(command, str):
        return []
    n = len(command)
    out = []
    i = 0
    in_single = False
    in_double = False
    while i < n:
        ch = command[i]
        if in_single:
            if ch == "'":
                in_single = False
            i += 1
            continue
        if in_double:
            if ch == "\\":
                i += 2
                continue
            if ch == '"':
                in_double = False
            i += 1
            continue
        if ch == "'":
            in_single = True
            i += 1
            continue
        if ch == '"':
            in_double = True
            i += 1
            continue
        if ch == "\\":
            i += 2
            continue
        if ch == ">":
            boundary_ok = _at_boundary(command, i)
            j = i
            while j < n and command[j] == ">":
                j += 1
            if boundary_ok:
                k = j
                while k < n and command[k].isspace():
                    k += 1
                if k < n and command[k] != ">" and command[k] != "&":
                    if command[k] in ("'", '"'):
                        q = command[k]
                        e = k + 1
                        buf = []
                        while e < n and command[e] != q:
                            buf.append(command[e])
                            e += 1
                        out.append("".join(buf))
                        i = e + 1 if e < n else e
                        continue
                    e = k
                    while e < n and not command[e].isspace() and command[e] not in (">", "&"):
                        e += 1
                    out.append(_strip_matching_quotes(command[k:e]))
                    i = e
                    continue
            i = j
            continue
        i += 1
    return out


def has_redirect(command):
    """#299: quote-aware replacement for `_REDIRECT_WRITE_RX.search(command)`
    at the two sites that asked "is there a redirect at all?"."""
    return bool(_redirect_targets(command))


def _is_nonrepo_redirect_target(target):
    return (
        target in _NONREPO_REDIRECT_EXACT
        or target.startswith(_NONREPO_REDIRECT_PREFIXES)
    )


def redirect_is_write(command):
    """True iff command has a redirect targeting somewhere other than a
    no-op sink or temp scratch space. A command whose only redirect targets
    are excluded (`cmd >/dev/null 2>&1`, `cat > /tmp/body.md`) is not a
    write via this pattern -- other _BASH_WRITE_PATTERNS entries still
    apply, so a compound command with a real write is still caught."""
    targets = _redirect_targets(command)
    return any(not _is_nonrepo_redirect_target(t) for t in targets)


# #299 (case 2): write indicators inside an inline `python -c` script.
# Deliberately conservative: `.write` catches `sys.stdout.write` too, and any
# `open(` counts, so the divergence from the old presence-only match only
# covers scripts that plainly touch nothing. Copied from PY_WRITE_HINTS in the
# Rust shadow (guard_policy.rs); keep in step.
_PY_WRITE_HINTS = (
    "open(",
    ".write",
    "os.remove",
    "os.unlink",
    "os.rename",
    "os.replace",
    "os.rmdir",
    "os.mkdir",
    "os.makedirs",
    "os.system",
    "os.chmod",
    "os.chown",
    "os.truncate",
    "os.link",
    "os.symlink",
    "shutil",
    "subprocess",
    "json.dump(",
    "pickle.dump",
    "csv.writer",
    ".unlink(",
    ".mkdir(",
    ".touch(",
    ".rmdir(",
    ".rename(",
    "urllib",
    "requests.",
    "socket.",
)


def _shell_arg_after(command, i):
    """#299: the argument following `-c` at index i, honouring shell quoting.
    None means "could not read it" -- the conservative branch."""
    n = len(command)
    j = i
    while j < n and command[j].isspace():
        j += 1
    if j >= n:
        return None
    if command[j] in ("'", '"'):
        q = command[j]
        e = j + 1
        buf = []
        while e < n:
            if q == '"' and command[e] == "\\" and e + 1 < n:
                buf.append(command[e + 1])
                e += 2
                continue
            if command[e] == q:
                return "".join(buf)
            buf.append(command[e])
            e += 1
        return None  # unterminated quote: unreadable
    e = j
    while e < n and not command[e].isspace():
        e += 1
    return command[j:e] or None


def python_dash_c_writes(command):
    """#299 (case 2): the bare `python -c` pattern treated mere presence as a
    write, so `python3 -c "print(1)"` was denied `bash-write-no-coverage`.
    Read the inline script instead: if it can be extracted and carries no
    write indicator, the command is not a write via this pattern. When the
    script cannot be extracted (unquoted, unterminated) or is built by shell
    expansion (`$`, backtick -- the text read is not the text run), stay
    conservative and keep the old classification."""
    if not isinstance(command, str):
        return False
    positions = [m.end() for m in _PYTHON_DASH_C_RX.finditer(command)]
    if not positions:
        return False
    for p in positions:
        script = _shell_arg_after(command, p)
        if script is None:
            return True
        if "$" in script or "`" in script:
            return True
        if any(hint in script for hint in _PY_WRITE_HINTS):
            return True
    return False


# #299 (case 1): `(gh|glab) (issue|pr|mr) (list|view|status|diff|checks)` --
# pure forge reads. `gh issue list --state all --json ...` was denied while
# `gh issue view N --json ...` passed; both are reads. Neither verb is in
# _GH_ISSUE_WRITE_RX or _FORGE_WRITE_RX, so the observed denial came from the
# quote-blind redirect scan firing on a `--jq '... > ...'` filter (case 3).
# This predicate makes the read classification explicit and testable so a
# future widening of the forge verb lists cannot silently recapture it.
_FORGE_READ_RX = re.compile(
    r"(^|[\s;&|`(])(gh|glab)\s+(issue|pr|mr)\s+(list|view|status|diff|checks)\b"
)


def forge_read_command(command):
    if not isinstance(command, str):
        return False
    return bool(_FORGE_READ_RX.search(command))


def _pattern_is_write(rx, command):
    """Evaluate one _BASH_WRITE_PATTERNS entry against command. Every entry
    is a plain regex search except the redirect entry, which additionally
    requires at least one non-excluded target, and the `python -c` entry,
    which reads the inline script (#299)."""
    if rx is _REDIRECT_WRITE_RX:
        return redirect_is_write(command)
    if rx is _PYTHON_DASH_C_RX:  # #299
        return python_dash_c_writes(command)
    return bool(rx.search(command))


def bash_writes(command):
    if not isinstance(command, str):
        return False
    return any(_pattern_is_write(rx, command) for rx in _BASH_WRITE_PATTERNS)


# conventions.md, "Issue-only forge work with no repo diff": create, edit,
# comment on, close, or reopen a forge issue with no tracked-file change and
# no commit is exempt from worklist coverage entirely. `delete` / `transfer`
# / `pin` / `unpin` / `lock` / `unlock` are deliberately NOT exempt -- they
# are not the verbs conventions.md names, so they keep requiring coverage.
_FORGE_ISSUE_ONLY_RX = re.compile(
    r"(^|[\s;&|`(])gh\s+issue\s+(comment|create|edit|close|reopen)\b"
)


def forge_issue_only_write(command):
    """True iff the command is issue-only forge work that conventions.md
    exempts from worklist coverage ("Issue-only forge work with no repo
    diff"). A compound command that ALSO matches any other write pattern is
    not exempt, so `gh issue comment ... && git commit ...` still faces the
    gate."""
    if not isinstance(command, str) or not _FORGE_ISSUE_ONLY_RX.search(command):
        return False
    # Any redirect in the same command disqualifies the exemption, regardless
    # of target. Before the non-repo-target narrowing this fell out of
    # _BASH_WRITE_PATTERNS by accident: a heredoc body write counted as a
    # write, so `cat > /tmp/b.md <<EOF ... && gh issue comment --body-file`
    # was refused and the agent had to split the two commands. That accident
    # was load-bearing -- splitting them is what lets PreToolUse read the body
    # and actually verify the signature (conventions.md, "Write the body file
    # in its own step"); combined, the check fails open against the very
    # artifact it exists to verify. Narrowing the redirect pattern would have
    # silently restored the combined form, so the refusal is now deliberate
    # rather than incidental.
    if has_redirect(command):  # #299: quote-aware
        return False
    for rx in _BASH_WRITE_PATTERNS:
        if rx is _GH_ISSUE_WRITE_RX:
            continue
        if _pattern_is_write(rx, command):
            return False
    return True


# Cross-boundary signature check. conventions.md ("Name the boundary and
# sign your side") requires every artifact crossing a project boundary to
# open with `<owner>'s <Agent> speaking from the <Project> project:`.
# Measured drift on both sides is what motivates enforcing it here rather
# than trusting prose alone.
#
# A parallel implementation lives in the Codex guard; the two scripts
# install to different roots as standalone files, so this logic is
# deliberately duplicated rather than shared (same reasoning as
# _BASH_WRITE_PATTERNS). Keep them in step.
#
# Three gates, all of which must pass before denying. Any shape we cannot
# read with confidence FAILS OPEN — a guard that blocks real work over a
# formatting rule is worse than the drift it prevents.
_FORGE_WRITE_RX = re.compile(
    r"(?:^|[\s;&|`(])(?:gh|glab)\s+(?:issue|pr|mr)\s+"
    r"(?:create|comment|note|edit|update|review)\b"
)
# Body sources. The alternation deliberately requires `=` or whitespace
# after the flag so `--body-file` is not swallowed by the `--body` arm.
_BODY_FILE_RX = re.compile(r"(?:--body-file|-F)(?:=|\s+)(\S+)")
_BODY_LITERAL_RX = re.compile(
    r"(?:--body|-b|--message|-m|--description)(?:=|\s+)"
    r"(\"(?:[^\"\\]|\\.)*\"|'[^']*'|\S+)"
)
# `<owner>'s <Agent> speaking from the <Project> project:` — the shape, not
# the instance. The script ships to every managed project, so no owner or
# project name is hardcoded; conventions.md supplies the instances.
_SIGNATURE_RX = re.compile(
    r"speaking\s+from\s+the\s+.{1,60}?\bproject\b",
    re.IGNORECASE,
)


def _unquote_shell(value):
    if len(value) >= 2 and value[0] == value[-1] and value[0] in ("'", '"'):
        inner = value[1:-1]
        return inner.replace('\\"', '"').replace("\\n", NL)
    return value


def crossboundary_body(command, cwd):
    """Return (body_text, reason). body_text is None when the body cannot be
    read, with reason naming why so the fail-open is greppable."""
    m = _BODY_FILE_RX.search(command)
    if m:
        path = m.group(1)
        if path == "-":
            return None, "body-file-stdin"
        try:
            candidate = path if os.path.isabs(path) else os.path.join(cwd, path)
            with open(candidate, encoding="utf-8", errors="replace") as f:
                return f.read(), "body-file"
        except Exception:
            return None, "body-file-unreadable"
    m = _BODY_LITERAL_RX.search(command)
    if m:
        return _unquote_shell(m.group(1)), "body-literal"
    return None, "no-body-flag"


def body_is_signed(body):
    """True iff the first non-empty line carries the canonical shape."""
    if not isinstance(body, str):
        return False
    for line in body.splitlines():
        stripped = line.strip().lstrip("_*>#- ").strip()
        if not stripped:
            continue
        return bool(_SIGNATURE_RX.search(stripped))
    return False


def crossboundary_signature_verdict(command, cwd):
    """('skip'|'signed'|'unparsed'|'unsigned', detail).

    'skip' means the command is not a forge write with a body at all.

    The signature is required on EVERY agent-authored forge artifact, not
    only ones that cross a project boundary. The risk it exists to prevent
    is not "which project is this from" but "who is speaking": on a shared
    account an agent comment is indistinguishable from a human one, and
    that is true in every repo. The cross-boundary case was a subset of the
    problem mistaken for the whole of it (judell/bram#253, where unsigned
    agent comments sat beside Jon's own replies while a cross-boundary
    issue in the same period was correctly signed).

    So there is no origin comparison here any more, and the hook no longer
    parses .git/config. That the correction REMOVES a gate is the tell that
    the scoping was the accidental part.
    """
    if not isinstance(command, str) or not command:
        return "skip", "no-command"
    if not _FORGE_WRITE_RX.search(command):
        return "skip", "not-forge-write"
    body, reason = crossboundary_body(command, cwd)
    if body is None:
        return "unparsed", reason
    if body_is_signed(body):
        return "signed", reason
    return "unsigned", reason


# issue-277 B: a full 40-hex SHA written into a forge artifact is
# uncheckable by eye (right length, right alphabet, the short prefix is
# usually correct) and 404s silently when fabricated or orphaned by a
# rebase. Verify each one locally before the write leaves the machine.
# Short SHAs are deliberately out of scope — too many false positives
# against ordinary hex in prose. The \b boundaries mean a longer hex run
# (e.g. a sha256) never yields a 40-char window.
_FULL_SHA_RX = re.compile(r"\b[0-9a-f]{40}\b")


def forge_sha_verdict(command, cwd):
    """('skip'|'ok'|'unparsed'|'bad', detail) for full SHAs in a forge write.

    Fails open on the signature check's terms: a body that cannot be read
    is allowed through ('unparsed'), and so is any git error — the check
    only ever denies on a positive local determination that a named
    40-hex string is not a commit in this repo. That also catches the
    rebase-orphan case: a rewritten commit's old SHA stops resolving.
    """
    if not isinstance(command, str) or not command:
        return "skip", "no-command"
    if not _FORGE_WRITE_RX.search(command):
        return "skip", "not-forge-write"
    body, reason = crossboundary_body(command, cwd)
    if body is None:
        return "unparsed", reason
    shas = sorted(set(_FULL_SHA_RX.findall(body)))
    if not shas:
        return "ok", "no-full-shas"
    for sha in shas:
        try:
            probe = subprocess.run(
                ["git", "cat-file", "-e", sha + "^{commit}"],
                cwd=cwd,
                stdout=subprocess.DEVNULL,
                stderr=subprocess.DEVNULL,
                timeout=5,
            )
        except Exception:
            return "unparsed", "git-unavailable"
        if probe.returncode != 0:
            return "bad", sha
    return "ok", "verified:" + str(len(shas))


# Worklist lifecycle routes are single-claimant: one inflight sentinel, one
# whole-record auth consume. A subagent calling one directly races the
# orchestrator's own claim/authorization state (subagent-delegation-
# serialize-the-gates). See "Delegating worklist items to subagents" in
# conventions.md.
_WORKLIST_LIFECYCLE_ROUTES = (
    "/__worklist/resolve", "/__worklist/mutate", "/__worklist/commit",
)


def subagent_lifecycle_verdict(command, agent_id):
    """('skip'|'deny'|'allow', detail) for a Bash command that may be a
    worklist lifecycle call made from a delegated subagent.

    'skip' means the command doesn't target a lifecycle route at all, so
    the caller should fall through to the other Bash checks. 'deny' means
    the command targets a lifecycle route AND the call originated in a
    subagent. 'allow' is everything else -- crucially including the
    orchestrator's own lifecycle calls, which are the normal case and must
    never be blocked.

    Keyed on the payload's `agent_id`, which is populated only for
    subagent-originated tool calls (documented as a subagent-specific field
    at https://code.claude.com/docs/en/hooks.md, and observed present in a
    real payload -- see the decision=observe trace from ee33cd5).

    This previously keyed on a `/subagents/` segment in `transcript_path`,
    which never fired: that field is present but carries the session path,
    never a subagent path. The check was inert from 6e07f6b until the
    deliberate violation in xmlui-org/xmlui-mcp#33 exposed it. Keying on
    identity rather than on where transcripts happen to be stored is also
    the more durable choice -- storage layouts move (the subagents/workflows
    discovery miss is precedent), whereas agent_id is about the caller.

    Fail open when agent_id is absent: a guard that blocked the
    orchestrator's lifecycle calls would break every gate in the system.
    """
    if not isinstance(command, str) or not any(
        route in command for route in _WORKLIST_LIFECYCLE_ROUTES
    ):
        return "skip", None
    if isinstance(agent_id, str) and agent_id.strip():
        return "deny", None
    return "allow", "no-agent-id"


# judell/bram#287: the lifecycle-route check above covers the Bash branch's
# curl calls, but the Write/Edit branch's own worklist surfaces --
# resources/worklist.json and resources/worklist-drafts/*.md -- have no
# comparable check. Both are implicitly-authorized "lifecycle channel" paths
# (see is_lifecycle_path / emit_allow_for_lifecycle), which is exactly what
# let a delegated subagent write them directly with no denial: a subagent
# widened its own item's `files` via a direct worklist.json edit, unnoticed
# until the item was later found committing more than it should have.
_WORKLIST_WRITE_PATHS_PREFIXES = (WORKLIST_DRAFTS_PREFIX,)


def subagent_worklist_write_verdict(rel, agent_id):
    """('skip'|'deny', detail) for a Write/Edit (or Bash-redirect) targeting
    resources/worklist.json or resources/worklist-drafts/*, keyed on whether
    the call carries a subagent's `agent_id`.

    'skip' means either rel isn't a worklist proposal/claim path at all, or
    agent_id is absent/whitespace-only -- the orchestrator's own path, which
    must never be blocked (fail-open, mirroring subagent_lifecycle_verdict
    above). 'deny' means both conditions hold: a subagent-originated call
    directly editing the worklist's proposal metadata or claim/draft prose.

    Worklist proposals and claim edits are supposed to happen only in the
    orchestrator's own turn -- conventions.md's "Delegating worklist items to
    subagents" says subagents receive file paths and instructions only, never
    lifecycle authority. This check is this file's second attempt at
    enforcing that boundary for the Write/Edit surface (the first,
    is_lifecycle_path's blanket allow, has no agent_id test at all) and, like
    subagent_lifecycle_verdict's original transcript_path check, it has not
    yet been proven live by a deliberate violation -- that fire is planned
    from the xmlui-side session.
    """
    if rel == WORKLIST_REL or (
        isinstance(rel, str) and rel.startswith(_WORKLIST_WRITE_PATHS_PREFIXES)
    ):
        if isinstance(agent_id, str) and agent_id.strip():
            return "deny", None
        return "skip", "no-agent-id"
    return "skip", None


# MCP is the third write surface, at parity with the Codex guard: a user
# with a filesystem MCP server configured can route writes through
# mcp__filesystem__write_file / edit_file / move_file, bypassing the
# Write/Edit gate entirely. This project has a project-scoped filesystem
# server whose allowed roots include the repo (judell/bram#261).
#
# Mutation candidates are recognized by a token in the tool-name suffix, so
# read-only MCP tools (search, docs, list) pass untouched and non-filesystem
# servers are not broken. Mirrors _MCP_WRITE_TOKENS in the Codex guard.
_MCP_WRITE_TOKENS = (
    "write", "edit", "create", "delete", "remove", "rename",
    "move", "copy", "patch", "append", "truncate", "mkdir", "rmdir",
    "modify", "replace", "save", "set_",
)

# Path-shaped keys in MCP tool_input payloads. The standard
# @modelcontextprotocol/server-filesystem uses path/source/destination;
# other servers vary. Mirrors _MCP_PATH_KEYS in the Codex guard.
_MCP_PATH_KEYS = (
    "path", "file_path", "filepath", "filename",
    "source", "src", "destination", "dest", "dst",
    "target", "target_path", "to", "from",
)


def mcp_is_mutation(tool_name):
    """True iff an mcp__-prefixed tool name signals filesystem mutation."""
    name = (tool_name or "").lower()
    return any(tok in name for tok in _MCP_WRITE_TOKENS)


def mcp_paths(tool_input):
    """Extract path-shaped values from an MCP tool_input dict."""
    if not isinstance(tool_input, dict):
        return []
    out = []
    for key in _MCP_PATH_KEYS:
        v = tool_input.get(key)
        if isinstance(v, str) and v:
            out.append(v)
        elif isinstance(v, list):
            for item in v:
                if isinstance(item, str) and item:
                    out.append(item)
    return out


def _post_hook_trace(script, event, tool, target, decision, reason, cwd):
    """Ship the [hook] decision line to the host, which writes it through the
    standard bram-trace path — gated on the LIVE Traces setting, not on
    spawn-time env (hook-trace-follow-settings). BRAM_TRACE/BRAM_TRACE_LOG
    env gating is gone: env is inherited when the agent process spawns, so
    it silently desynced from the Settings toggle for the lifetime of a
    session. Failures are swallowed; tracing must never block a tool call."""
    try:
        cur = os.path.abspath(cwd or os.getcwd())
        port = None
        while True:
            candidate = os.path.join(cur, "resources", ".bram-port")
            if os.path.exists(candidate):
                with open(candidate, encoding="utf-8") as f:
                    port = int(f.read().strip())
                break
            parent = os.path.dirname(cur)
            if parent == cur:
                break
            cur = parent
        if not port:
            return
        body = json.dumps({
            "script": script,
            "event": event,
            "tool": tool,
            "target": str(target)[:300],
            "cwd": cwd or "",
            "decision": decision,
            "reason": reason,
        }).encode()
        req = urllib.request.Request(
            "http://127.0.0.1:%d/__hook-trace" % port,
            data=body,
            headers={"Content-Type": "application/json"},
            method="POST",
        )
        urllib.request.urlopen(req, timeout=0.3).read()
    except Exception:
        pass


def _trace_hook(event, tool, target, decision, reason, cwd=None):
    """Issue #49 [hook] trace + issue #95 phantom-write diagnostic.

    - Always emits one `[worklist-guard]` line to stderr, including cwd,
      so the hook's decision is visible to the agent / user without
      tracing enabled. Refs #95 — phantom worklist writes need this
      signal to distinguish hook-block from cwd-mismatch from
      watcher-revert.
    - Additionally POSTs the line to the host's /__hook-trace route, which
      appends it to resources/bram-traces/bram-trace.log iff the live
      Traces setting is on (hook-trace-follow-settings).
    """
    if cwd is None:
        try:
            cwd = os.getcwd()
        except Exception:
            cwd = ""
    diagnostic = (
        f"[worklist-guard] tool={tool} target={target} cwd={cwd} "
        f"decision={decision} reason={reason}"
    )
    try:
        sys.stderr.write(diagnostic + "\n")
        sys.stderr.flush()
    except Exception:
        pass
    _post_hook_trace("claude-worklist-guard.py", event, tool, target, decision, reason, cwd)


# The single opt-out phrase that authorizes a one-turn direct edit. Matched
# case-insensitively against the last user message. Narrowed from a
# seven-regex list (opt-out-single-phrase-and-audit) to one explicit phrase
# — anything ambiguous ("looks good", "go ahead") is NOT here, matching the
# conventions' "Don't infer commit/drop/advance from feedback" rule. The
# non-verbal route (the Skip worklist button / `skip-worklist:` prefix)
# is unaffected and lives in the host, not here.
_OPT_OUT_PATTERNS = [
    re.compile(r"\bjust do it\b", re.IGNORECASE),
]


def items_by_id(text):
    try:
        return {it["id"]: it for it in json.loads(text).get("items", [])}
    except Exception:
        return {}


def last_user_text(transcript_path):
    if not transcript_path or not os.path.exists(transcript_path):
        return ""
    last = ""
    with open(transcript_path, encoding="utf-8", errors="replace") as f:
        for line in f:
            try:
                m = json.loads(line)
            except Exception:
                continue
            if m.get("type") != "user":
                continue
            c = m.get("message", {}).get("content", "")
            if isinstance(c, list):
                c = "".join(
                    p.get("text", "") for p in c
                    if isinstance(p, dict) and p.get("type") == "text"
                )
            # Only update `last` when c has actual text. tool_result-only user
            # records collapse to an empty string in the list comprehension above;
            # overwriting `last` with that would lose a real `approved:`/`drop:`
            # message typed in an earlier turn whenever any tool call followed it.
            if isinstance(c, str) and c.strip():
                last = c
    return last


def has_opt_out(msg):
    if not isinstance(msg, str):
        return False
    return any(rx.search(msg) for rx in _OPT_OUT_PATTERNS)


def iterate_feedback_text(project_root, last_msg):
    """issue-171: an Iterate turn's real text lives in
    resources/feedback-drafts/<feedbackRef>.md — the transcript carries only
    the structured `iterate:` payload, so an opt-out phrase typed into
    iterate feedback was invisible to the matcher (observed live in #170:
    "just do it" in feedback, denied anyway; only retyping in plain chat
    unblocked). When the last user message is an iterate payload, return the
    concatenated draft texts so has_opt_out sees what the user actually
    typed. Fails open to "" on any parse or read miss — absence of drafts
    must never widen or narrow anything beyond current behavior."""
    if not isinstance(last_msg, str) or not project_root:
        return ""
    stripped = last_msg.strip()
    if not stripped.startswith("iterate:"):
        return ""
    try:
        doc = json.loads(stripped[len("iterate:"):].strip())
        items = doc.get("items", [])
    except Exception:
        return ""
    if not isinstance(items, list):
        return ""
    out = []
    for it in items:
        ref = it.get("feedbackRef") if isinstance(it, dict) else None
        if not isinstance(ref, str) or not ref:
            continue
        # Refs are host-allocated basenames; anything path-shaped is refused
        # rather than resolved.
        if "/" in ref or "\\" in ref or ".." in ref:
            continue
        path = os.path.join(project_root, "resources", "feedback-drafts", ref + ".md")
        try:
            with open(path, encoding="utf-8", errors="replace") as f:
                out.append(f.read())
        except Exception:
            continue
    return "\n".join(out)


def _post_direct_edit_audit_breadcrumb(project_root, last_msg):
    """Best-effort breadcrumb POST to /__audit/direct-edit so a Claude
    prose opt-out lands in the audit ledger the same way a Codex prose
    opt-out or the Skip worklist button already does (opt-out-single-
    phrase-and-audit). Mirrors the fire-and-forget pattern in
    claude-permission-menu-hook.py: short timeout, silent failure — the
    guard must never block or fail a tool call because Bram is down."""
    try:
        with open(
            os.path.join(project_root, "resources", ".bram-port"), encoding="utf-8"
        ) as f:
            port = int(f.read().strip())
        turn_key = hashlib.sha256(last_msg.encode("utf-8")).hexdigest()
        body = json.dumps(
            {
                "provider": "claude",
                "source": "claude-guard-opt-out",
                "turnKey": turn_key,
            }
        ).encode("utf-8")
        req = urllib.request.Request(
            "http://127.0.0.1:%d/__audit/direct-edit" % port,
            data=body,
            headers={"Content-Type": "application/json"},
            method="POST",
        )
        urllib.request.urlopen(req, timeout=0.4).read()
    except Exception:
        pass


def find_project_root(start):
    """Walk up from `start` until we find the AUTH_REL marker. Returns the
    project root path, or None if the marker isn't anywhere above."""
    cur = os.path.abspath(start)
    while True:
        if os.path.exists(os.path.join(cur, AUTH_REL)):
            return cur
        parent = os.path.dirname(cur)
        if parent == cur:
            return None
        cur = parent


def normalize_target(project_root, target):
    """Return project-relative path for target if it's inside project_root,
    else None."""
    if not isinstance(target, str) or not target:
        return None
    abs_target = os.path.abspath(target)
    abs_root = os.path.abspath(project_root)
    if abs_target == abs_root:
        return ""
    prefix = abs_root + os.sep
    if abs_target.startswith(prefix):
        return abs_target[len(prefix):].replace(os.sep, "/")
    return None


def is_worklist_draft(rel):
    return (
        isinstance(rel, str)
        and rel.startswith(WORKLIST_DRAFTS_PREFIX)
        and rel.endswith(".md")
        and "/" not in rel[len(WORKLIST_DRAFTS_PREFIX):]
    )


def worklist_items_with_inline_prose(content):
    """Parse worklist content; return list of ids that carry non-empty
    inline `before` or `after`. Draft-only: prose belongs in
    resources/worklist-drafts/<id>.md, not in the metadata index."""
    try:
        doc = json.loads(content)
    except Exception:
        return []
    items = doc.get("items") if isinstance(doc, dict) else None
    if not isinstance(items, list):
        return []
    bad = []
    for it in items:
        if not isinstance(it, dict):
            continue
        if it.get("status", "proposed") != "proposed":
            continue
        before = it.get("before")
        after = it.get("after")
        has_inline = (
            (isinstance(before, str) and before.strip())
            or (isinstance(after, str) and after.strip())
        )
        if has_inline:
            item_id = it.get("id")
            label = item_id if (isinstance(item_id, str) and item_id.strip()) else "<no-id>"
            bad.append(label)
    return bad


def worklist_covered_files(project_root):
    """Set of project-relative paths covered by proposed/applied items."""
    try:
        with open(os.path.join(project_root, WORKLIST_REL), encoding="utf-8") as f:
            data = json.load(f)
    except Exception:
        return set()
    covered = set()
    for it in data.get("items") or []:
        if not isinstance(it, dict):
            continue
        st = it.get("status", "proposed")
        if st not in ("proposed", "applied"):
            continue
        if isinstance(it.get("file"), str):
            covered.add(it["file"])
        for p in it.get("files", []) or []:
            if isinstance(p, str):
                covered.add(p)
    return covered


def _bypass_lookup_detail(project_root, path_rel):
    """Inspect project_root's AUTH_REL for a fresh direct-edit bypass
    covering path_rel. Returns (matched, detail):

    - (False, "absent")            -- no readable/parseable record there.
    - (False, "wrong-kind")        -- a record exists but isn't direct-edit.
    - (False, "stale")             -- a direct-edit record there expired
                                       (>BYPASS_TTL_SECONDS old).
    - (False, "path-not-covered")  -- a fresh direct-edit record there
                                       doesn't list path_rel or "*".
    - (True, "ok")                 -- matched.

    `detail` exists so a denial can name what it found at each root it
    checked (issue-262), not just that the bypass failed.
    """
    try:
        with open(os.path.join(project_root, AUTH_REL), encoding="utf-8") as f:
            rec = json.load(f)
    except Exception:
        return False, "absent"
    if not isinstance(rec, dict) or rec.get("kind") != "direct-edit":
        return False, "wrong-kind"
    issued = rec.get("issuedAtMs") or rec.get("issued_at_ms") or 0
    if (time.time() * 1000 - issued) > BYPASS_TTL_SECONDS * 1000:
        return False, "stale"
    paths = rec.get("paths") or []
    if path_rel in paths or "*" in paths:
        return True, "ok"
    return False, "path-not-covered"


def fresh_bypass(project_root, path_rel):
    """True iff the auth record carries a recent direct-edit bypass
    covering path_rel."""
    matched, _ = _bypass_lookup_detail(project_root, path_rel)
    return matched


def resolve_session_root(payload):
    """The driving session's own project root -- walk up from the
    payload's `cwd` (present on every PreToolUse payload) to the same
    AUTH_REL marker find_project_root uses for a target path. Returns None
    when cwd is absent/unusable, or when no managed project sits above it
    (an unmanaged cwd simply has no worklist-authorization surface of its
    own to check -- not a failure)."""
    cwd = payload.get("cwd")
    if not isinstance(cwd, str) or not cwd:
        return None
    return find_project_root(cwd)


def cross_root_bypass_verdict(target_root, path_rel, session_root):
    """issue-262: a `skip-worklist:` turn writes its one-turn direct-edit
    record into the ACTIVE (session/cwd) project's AUTH_REL, but Write/Edit
    resolves project_root from the TARGET file's path -- for a cross-project
    edit those are two different roots, so the record is written correctly
    and never found. Consult BOTH roots for the bypass: the target
    project's own record (default, checked first -- unchanged for every
    same-project edit) and the session project's record, only when it
    differs from the target root. A fresh direct-edit record in either
    authorizes. This widening applies ONLY to the direct-edit bypass --
    coverage checks, worklist reads, and everything else stay target-rooted,
    because a direct-edit record is written by an explicit, fresh,
    user-initiated act in the driving session, which is exactly what the
    verbal "just do it" opt-out already honors regardless of project root
    (see has_opt_out / opt_out_clears, which read the transcript, not a
    root -- the asymmetry issue-262 reports).

    Returns (matched, matched_via, target_detail, session_detail):
    - matched_via is "target" | "session" | None.
    - target_detail is always populated (the first, and default, lookup).
    - session_detail is None when the second lookup never ran: no usable
      session_root, or session_root == target_root (degenerate case --
      skip the redundant second lookup; behavior is single-root/unchanged).
      Otherwise it carries the same detail vocabulary as target_detail.
    """
    matched, target_detail = _bypass_lookup_detail(target_root, path_rel)
    if matched:
        return True, "target", target_detail, None
    if not session_root:
        return False, None, target_detail, None
    if os.path.abspath(session_root) == os.path.abspath(target_root):
        return False, None, target_detail, None
    session_matched, session_detail = _bypass_lookup_detail(session_root, path_rel)
    if session_matched:
        return True, "session", target_detail, session_detail
    return False, None, target_detail, session_detail


# judell/bram#283: the approval that produced a commit implies publishing
# exactly that commit, not a standing push permission -- so the window is
# short (a grace), not open-ended.
POST_COMMIT_PUSH_GRACE_MS = 10 * 60 * 1000


def post_commit_push_grace(project_root):
    """True iff resources/.worklist-authorization.json (relative to
    project_root) records a commit-gate authorization ("approved") that was
    consumed within the last POST_COMMIT_PUSH_GRACE_MS. That is the on-disk
    evidence a gate commit just happened -- worklist-commit consumes the
    approved auth record and stamps consumedAtMs as part of committing, then
    prunes the item(s), so a fresh consumedAtMs is the only trace left once
    the board is empty again. Any read/parse failure returns False."""
    try:
        with open(os.path.join(project_root, AUTH_REL), encoding="utf-8") as f:
            rec = json.load(f)
    except Exception:
        return False
    if not isinstance(rec, dict) or rec.get("kind") != "approved":
        return False
    consumed = rec.get("consumedAtMs") or rec.get("consumed_at_ms") or 0
    if not consumed:
        return False
    return (time.time() * 1000 - consumed) <= POST_COMMIT_PUSH_GRACE_MS


_BYPASS_DETAIL_LABELS = {
    "absent": "no resources/.worklist-authorization.json there",
    "wrong-kind": "a record exists there but isn't a direct-edit bypass",
    "stale": "a direct-edit record there has expired (>1h old)",
    "path-not-covered": "a fresh direct-edit record there doesn't cover this path",
}


def deny_coverage(
    target_rel, opt_out_attempted, project_root=None,
    session_root=None, session_bypass_detail=None,
):
    msg = (
        f"Blocked: writing to {target_rel} requires either a proposed/applied "
        f"item in resources/worklist.json covering this path, or an explicit "
        f"opt-out phrase in your last message.\n"
        f"  - Propose the change in resources/worklist.json first (item with "
        f"file=\"{target_rel}\", non-empty before and after, status proposed). "
        f"Wait for the user's approved: payload, then retry.\n"
        f"  - The user can authorize a direct edit by ending their message "
        f"with \"just do it\", or by clicking the Skip worklist button."
    )
    if opt_out_attempted:
        msg += (
            "\n  - (Detected what looked like opt-out language, but it didn't "
            "match the expected phrasing. The verbal opt-out is exactly "
            "\"just do it\"; otherwise use the Skip worklist button.)"
        )
    # issue-280: name the root this denial was judged against. A stray
    # AUTH_REL marker in a subdirectory silently captures everything beneath
    # it, and the only symptom used to be an unexpectedly short relative
    # `target` — three denials and a source-read to diagnose, live 2026-08-24
    # in the xmlui repo. One line makes the wrong root self-announcing (and
    # `cwd` in the trace is a red herring for Edit/Write, whose root comes
    # from the TARGET's directory).
    reason = "no-coverage-no-opt-out"
    if project_root:
        msg += (
            f"\nresolved_project_root={project_root} "
            f"(marker: {AUTH_REL}; if this is not the repo root, a stray "
            f"marker file captured this subtree — remove that {AUTH_REL})"
        )
        reason += f":root={project_root}"
    # issue-262: when the cwd-derived session root differs from the target
    # root, the bypass check above also looked there for a skip-worklist:
    # direct-edit record -- name what it found (or didn't) so a
    # cross-project miss explains itself instead of looking like the host
    # never wrote the record at all.
    if session_root and session_bypass_detail is not None:
        label = _BYPASS_DETAIL_LABELS.get(session_bypass_detail, session_bypass_detail)
        msg += (
            f"\nAlso checked the session project's direct-edit bypass "
            f"(cwd-derived root: {session_root}): {label}."
        )
        reason += f":session_root={session_root}:{session_bypass_detail}"
    _trace_hook(
        "PreToolUse",
        os.environ.get("__BRAM_TRACE_TOOL", "Write"),
        target_rel,
        "deny",
        reason,
    )
    print(msg, file=sys.stderr)
    sys.exit(2)


def opt_out_clears(project_root, payload, tool_name, target_rel):
    """Last-chance authorization shared by EVERY write surface: the documented
    "just do it" phrase in the user's last message.

    Every branch that is about to deny for lack of coverage must call this
    first. It lives in one place deliberately (judell/bram#263): the policy
    used to be inline in the Write/Edit branch only, so the Bash branch
    (#217) and then the MCP branch (#261) were each written without it, and
    `conventions.md`'s promise that the phrase is matched "on every
    PreToolUse" was false on two of the three surfaces for a month.

    Posting the audit breadcrumb is part of this function, not of its
    callers, for the same reason: it is the only record that a prose opt-out
    authorized a change, and a surface that honored the phrase without
    recording it would authorize unaudited writes — worse than
    over-denying.
    """
    last_msg = last_user_text(payload.get("transcript_path", ""))
    if not has_opt_out(last_msg):
        # issue-171: an Iterate turn's transcript message is the structured
        # payload; the user's actual words ride the feedback draft. Match
        # those too before concluding there was no opt-out.
        draft_text = iterate_feedback_text(project_root, last_msg)
        if not (draft_text and has_opt_out(draft_text)):
            return False
        last_msg = draft_text
    _post_direct_edit_audit_breadcrumb(project_root, last_msg)
    # Pass project_root as cwd: it is the project the decision applies to, and
    # it is where _post_hook_trace finds resources/.bram-port. Defaulting to
    # os.getcwd() named the wrong project on the Bash/MCP paths, where the
    # target can live outside the session's directory.
    _trace_hook("PreToolUse", tool_name, target_rel, "allow", "opt-out-phrase",
                project_root)
    return True


def deny_bash_coverage(command, cwd=None, project_root=None):
    preview = (command or "")[:200]
    reason = "bash-write-no-coverage"
    root_line = ""
    if project_root:
        # issue-280: see deny_coverage. For Bash the walk starts at cwd.
        root_line = (
            f"\nresolved_project_root={project_root} "
            f"(marker: {AUTH_REL}; if this is not the repo root, a stray "
            f"marker file captured this subtree — remove that {AUTH_REL})"
        )
        reason += f":root={project_root}"
    push_line = ""
    if _PUSH_CMD_RX.search(command or ""):
        push_line = (
            "\n  - This looks like a push. The user can click Push in the "
            "Commits tab; an agent push is allowed only in the 10-minute "
            "window after a gate commit (see judell/bram#283)."
        )
    _trace_hook(
        "PreToolUse",
        "Bash",
        preview,
        "deny",
        reason,
        cwd,
    )
    print(
        "Bash blocked: this command writes to the filesystem or performs a "
        "sensitive side effect, and resources/worklist.json has no proposed "
        "or applied items covering active work.\n"
        "  - Propose the work in the worklist first, wait for the user's "
        "approved: payload, then retry.\n"
        "  - The user can authorize a direct edit by ending their message "
        "with \"just do it\", or by clicking the Skip worklist button."
        + root_line
        + push_line,
        file=sys.stderr,
    )
    sys.exit(2)


def worklist_version_from_text(text):
    """Return (present, version) for a worklist.json text. `present` is
    True iff the JSON parsed and the top-level `version` field was an
    integer. `version` defaults to 0 when absent or malformed."""
    try:
        doc = json.loads(text)
    except Exception:
        return (False, 0)
    if not isinstance(doc, dict):
        return (False, 0)
    v = doc.get("version")
    if isinstance(v, int):
        return (True, v)
    return (False, 0)


def deny_stale_worklist_version(old_version, new_version, new_present):
    if new_present:
        detail = (
            f"You set version={new_version}, but on-disk version is "
            f"{old_version}. Expected version={old_version + 1}."
        )
    else:
        detail = (
            f"Your write is missing the `version` field. On-disk version "
            f"is {old_version}; the new content must set version="
            f"{old_version + 1}."
        )
    msg = (
        "Blocked: stale base on resources/worklist.json. "
        + detail
        + "\n  - Re-read resources/worklist.json, base your edit on the "
        "current contents, and include the bumped version field on the "
        "write. This guards against concurrent-writer races between "
        "your propose / Edit and other agents or the /__worklist/mutate "
        "route."
    )
    _trace_hook(
        "PreToolUse",
        os.environ.get("__BRAM_TRACE_TOOL", "Write"),
        WORKLIST_REL,
        "deny",
        "stale-worklist-version",
    )
    print(msg, file=sys.stderr)
    sys.exit(2)


def worklist_state_changes(old_items, new_items):
    removed = []
    status_changed = []
    for item_id, old_item in old_items.items():
        if item_id not in new_items:
            removed.append((item_id, old_item.get("status", "proposed")))
            continue
        old_status = old_item.get("status", "proposed")
        new_status = new_items[item_id].get("status", "proposed")
        if old_status != new_status:
            status_changed.append((item_id, old_status, new_status))
    return removed, status_changed


def deny_inline_prose(bad):
    lines = [
        "Blocked: inline `before` / `after` on proposed worklist item(s) — "
        "prose must live in resources/worklist-drafts/<id>.md."
    ]
    for label in bad:
        lines.append(f"  - item {label}: remove inline before/after; write prose to "
                     f"resources/worklist-drafts/{label}.md instead")
    lines.append(
        "Draft file format: `# Before` section then `# After` section, both "
        "in Markdown. The server merge surfaces the draft prose alongside "
        "the metadata in worklist.json."
    )
    _trace_hook(
        "PreToolUse",
        os.environ.get("__BRAM_TRACE_TOOL", "Write"),
        WORKLIST_REL,
        "deny",
        "worklist-inline-prose",
    )
    print("\n".join(lines), file=sys.stderr)
    sys.exit(2)


def deny_mechanical_worklist_change(removed, status_changed):
    lines = [
        "Blocked: mechanical worklist state changes must go through "
        "`POST /__worklist/mutate`, not a direct edit to "
        "`resources/worklist.json`.",
        "  - Direct worklist edits are for proposing items or refining "
        "their prose during iterate.",
        "  - Use mutate for `prune` and `advance` after a verified "
        "`drop:` / `approved:` turn.",
    ]
    if removed:
        detail = ", ".join(f'"{item_id}" (status={status})' for item_id, status in removed)
        lines.append(f"  - Removed item ids: {detail}")
    if status_changed:
        detail = ", ".join(
            f'"{item_id}" ({old_status}->{new_status})'
            for item_id, old_status, new_status in status_changed
        )
        lines.append(f"  - Status changes: {detail}")
    lines.append(
        "  - Example: "
        "curl -4 -sS -X POST -d '{\"op\":\"prune\",\"ids\":[\"item-id\"]}' "
        "http://127.0.0.1:$(cat resources/.bram-port)/__worklist/mutate"
    )
    _trace_hook(
        "PreToolUse",
        os.environ.get("__BRAM_TRACE_TOOL", "Write"),
        WORKLIST_REL,
        "deny",
        "mechanical-worklist-change",
    )
    print("\n".join(lines), file=sys.stderr)
    sys.exit(2)


def self_test():
    write_commands = [
        "echo x > out.txt",
        "printf x >> out.txt",
        "printf x | tee out.txt",
        "sed -i '' s/a/b/ file.txt",
        "perl -i -pe s/a/b/ file.txt",
        "python -c \"open('x', 'w').write('x')\"",
        "node -e \"require('fs').writeFileSync('x','x')\"",
        "git commit -m test",
        "git push",
        "gh issue close 119 --repo judell/bram",
        "rm stale.txt",
        "mv a b",
        "cp a b",
        "truncate -s 0 file.txt",
        "bash -c 'echo x > out.txt'",
        "sh -c 'echo x > out.txt'",
        # guard-write-detection-false-positives: other /dev/* targets and
        # real repo paths are still writes even though /dev/null and /tmp
        # are excluded below.
        "echo hi > /dev/stdout",
        "cat > src-tauri/src/lib.rs",
        # A no-op-sink redirect compounded with a genuine write is still
        # caught -- by the git pattern, not the redirect one.
        "cat > /tmp/b.md && git commit -m x",
        # #299: the quote-aware scanner must not lose real redirects.
        "echo hi > out.txt",
        'echo hi > "my file.txt"',
        # #299 (case 2): an inline script that plainly writes is still a write,
        # and one built by shell expansion stays conservative.
        "python3 -c \"open('x','w').write('x')\"",
        "python3 -c 'import shutil; shutil.rmtree(\"x\")'",
        'python3 -c "$SCRIPT"',
        "python3 -c",
    ]
    read_commands = [
        "ls -la",
        "rg Bash app/provider-hooks/claude-worklist-guard.py",
        "git status --short",
        "gh issue view 119 --repo judell/bram",
        "python --version",
        "curl -I https://example.com",
        # guard-write-detection-false-positives: no-op sinks and temp
        # scratch space are not repository mutations.
        "git ls-files x >/dev/null 2>&1",
        "echo hi > /dev/null",
        "echo hi >> /dev/zero",
        "cat > /tmp/body.md",
        "cat > /private/tmp/x/body.md",
        # #299 (case 3): a `>` inside quotes is a comparison / jq operator,
        # never a shell redirect.
        "gh issue list --state all --json number --jq '.[]|select(.number > 200)'",
        "awk '$1 >= \"2026-08-28T06:16\"' file.txt",
        "jq '.[] | select(.n > 5)' data.json",
        # #299 (case 2): an inline script with no write indicator is a read.
        'python3 -c "print(1)"',
        "python3 -c 'import sys; print(sys.version)'",
        # #299 (case 1): pure forge reads.
        "gh issue view 5 --json title",
        "gh issue list",
    ]
    for command in write_commands:
        assert bash_writes(command), command
    for command in read_commands:
        assert not bash_writes(command), command

    # #299 (case 1): the forge-read predicate, pinned as a regression guard so
    # a future widening of the write verb lists cannot silently recapture it.
    for command in (
        "gh issue list --state all --json number",
        "gh issue view 5 --json title",
        "gh pr list",
        "gh pr diff 12",
        "gh pr checks",
        "glab mr view 3",
        "glab issue list",
    ):
        assert forge_read_command(command), command
    for command in ("gh issue close 5", "gh issue comment 5 --body x", "ls -la"):
        assert not forge_read_command(command), command

    # #299 (case 3): the redirect scanner reports real targets and ignores
    # quoted `>` characters entirely.
    assert _redirect_targets("echo hi > out.txt") == ["out.txt"]
    assert _redirect_targets('echo hi > "my file.txt"') == ["my file.txt"]
    assert _redirect_targets("awk '$1 >= \"x\"' f.txt") == []
    assert _redirect_targets("jq '.n > 5' d.json > out.txt") == ["out.txt"]
    assert not has_redirect("jq '.[] | select(.n > 5)' data.json")

    # Issue-only forge work with no repo diff (conventions.md) is exempt from
    # worklist coverage entirely.
    assert forge_issue_only_write('gh issue comment 5 --body-file f')
    assert forge_issue_only_write('gh issue close 5')
    assert forge_issue_only_write('gh issue reopen 5')
    assert forge_issue_only_write('gh issue edit 5 --title "new title"')
    assert forge_issue_only_write('gh issue create --title x --body y')
    # A compound command that also matches another write pattern is NOT
    # exempt -- the repo-changing half still faces the gate.
    assert not forge_issue_only_write(
        'gh issue comment 5 --body-file f && git commit -m x'
    )
    # delete/transfer/pin/unpin/lock/unlock keep requiring coverage --
    # conventions.md names only comment/create/edit/close/reopen.
    assert not forge_issue_only_write('gh issue delete 5')
    assert not forge_issue_only_write('gh issue transfer 5 other/repo')
    assert not forge_issue_only_write('gh issue pin 5')
    assert not forge_issue_only_write('gh issue lock 5')
    # Non-issue / read-only commands are not exempt (they simply aren't
    # writes in the first place, so bash_writes already lets them through).
    assert not forge_issue_only_write('gh issue view 5')
    assert not forge_issue_only_write('git commit -m x')

    # MCP surface (judell/bram#261). Mutation recognition is by tool-name
    # token, so read-only servers pass untouched.
    mcp_mutations = [
        "mcp__filesystem__write_file",
        "mcp__filesystem__edit_file",
        "mcp__filesystem__move_file",
        "mcp__filesystem__create_directory",
    ]
    mcp_reads = [
        "mcp__filesystem__read_text_file",
        "mcp__filesystem__list_directory",
        "mcp__xmlui__xmlui_search_howto",
        "mcp__xmlui__xmlui_component_docs",
    ]
    for name in mcp_mutations:
        assert mcp_is_mutation(name), name
    for name in mcp_reads:
        assert not mcp_is_mutation(name), name

    # judell/bram#263: the opt-out must be reachable from every write surface,
    # so it is tested as one function rather than per-branch.
    import tempfile as _tempfile

    with _tempfile.TemporaryDirectory() as td:
        def _transcript(text):
            p = os.path.join(td, "t-%d.jsonl" % abs(hash(text)))
            with open(p, "w", encoding="utf-8") as f:
                f.write(json.dumps({
                    "type": "user",
                    "message": {"content": [{"type": "text", "text": text}]},
                }) + NL)
                # A trailing tool_result-only record must not clobber the
                # real user message (the reason last_user_text skips blanks).
                f.write(json.dumps({
                    "type": "user",
                    "message": {"content": [{"type": "tool_result", "text": ""}]},
                }) + NL)
            return p

        assert last_user_text(_transcript("patch it, just do it")) == "patch it, just do it"
        assert has_opt_out(last_user_text(_transcript("patch it, just do it")))
        assert not has_opt_out(last_user_text(_transcript("skip the worklist")))
        assert not has_opt_out(last_user_text(""))

        # The helper is what every branch calls; project_root is only used for
        # the breadcrumb POST, which is best-effort and silent with no port.
        assert opt_out_clears(
            td, {"transcript_path": _transcript("do the build, just do it")},
            "Bash", "git pull",
        )
        assert not opt_out_clears(
            td, {"transcript_path": _transcript("please do the build")},
            "Bash", "git pull",
        )
        assert not opt_out_clears(td, {}, "mcp__filesystem__write_file", "a.txt")

    # Signature gates. Mirrors the Codex guard's cases.

    with _tempfile.TemporaryDirectory() as td:
        signed = "Jon's Claude speaking from the Bram project:" + NL + NL + "Body."
        unsigned = "Vendored the build; the tooltip renders."

        def verdict(cmd):
            return crossboundary_signature_verdict(cmd, td)[0]

        # Gate 1: not a forge write.
        assert verdict("gh issue view 12 --repo xmlui-org/xmlui") == "skip"
        assert verdict("ls -la") == "skip"
        # Same repo is now inspected exactly like any other — the regression
        # test for the scoping this item removed.
        assert verdict('gh issue comment 5 --body "%s"' % unsigned) == "unsigned"
        assert verdict('gh issue comment 5 --body "%s"' % signed) == "signed"
        assert verdict(
            'gh issue comment 5 --repo judell/bram --body "%s"' % unsigned
        ) == "unsigned"
        # No .git/config anywhere in this tempdir: the verdict must not
        # depend on one being readable.
        assert not os.path.exists(os.path.join(td, ".git", "config"))
        # Gate 3: cross-repo behaviour unchanged in both directions.
        assert verdict(
            'gh issue comment 5 --repo xmlui-org/xmlui --body "%s"' % signed
        ) == "signed"
        assert verdict(
            'gh issue comment 5 --repo xmlui-org/xmlui --body "%s"' % unsigned
        ) == "unsigned"
        # Surfaces _BASH_WRITE_PATTERNS does not match.
        assert verdict('gh pr comment 9 --body "%s"' % unsigned) == "unsigned"
        assert verdict('glab issue note 3 -m "%s"' % unsigned) == "unsigned"
        # Fail-open shapes.
        assert verdict("gh issue comment 5 --body-file -") == "unparsed"
        assert verdict("gh issue comment 5 --body-file /nope/missing.md") == "unparsed"
        # No body to inspect at all (e.g. `gh issue close 5`) still fails open.
        assert verdict("gh issue comment 5") == "unparsed"
        assert verdict("gh issue close 5") == "skip"

        # --body-file must not be swallowed by the --body arm.
        body_path = os.path.join(td, "comment.md")
        with open(body_path, "w", encoding="utf-8") as f:
            f.write(signed)
        assert crossboundary_signature_verdict(
            "gh issue comment 5 --body-file %s" % body_path, td
        ) == ("signed", "body-file")

    # Leading Markdown emphasis on the signature line still counts.
    assert body_is_signed("_Jon's Codex speaking from the XMLUI project:_")
    assert body_is_signed("> **Jon's Claude speaking from the Bram project:**")
    assert not body_is_signed("Bram side — a correction to my green light.")
    assert not body_is_signed("")

    assert mcp_paths({"path": "app/x.xmlui"}) == ["app/x.xmlui"]
    assert mcp_paths({"source": "a", "destination": "b"}) == ["a", "b"]
    assert mcp_paths({"paths": ["a", "b"]}) == []  # unrecognized key -> deny path
    assert mcp_paths({"edits": [{"oldText": "x"}]}) == []
    assert mcp_paths("not-a-dict") == []

    # Subagent lifecycle-call gate (subagent-delegation-serialize-the-gates).
    with _tempfile.TemporaryDirectory() as td:
        lifecycle_cmd = (
            'curl -4 -sS -X POST -H "Content-Type: application/json" '
            "--data @/tmp/body.json http://127.0.0.1:61455/__worklist/mutate"
        )
        subagent_transcript = os.path.join(td, "subagents", "agent-1.jsonl")
        os.makedirs(os.path.dirname(subagent_transcript), exist_ok=True)
        with open(subagent_transcript, "w", encoding="utf-8") as f:
            f.write(json.dumps({
                "type": "user",
                "message": {"content": [{"type": "text", "text": "resolve it"}]},
            }) + NL)
        main_transcript = os.path.join(td, "sess-1.jsonl")
        with open(main_transcript, "w", encoding="utf-8") as f:
            f.write(json.dumps({
                "type": "user",
                "message": {"content": [{"type": "text", "text": "resolve it"}]},
            }) + NL)

        # Keyed on agent_id, not transcript_path: the latter is present on
        # every payload but never carries a subagent path, which is why the
        # original check was inert (ee33cd5).
        #
        # Lifecycle curl + agent_id present -> deny.
        assert subagent_lifecycle_verdict(lifecycle_cmd, "b1iz1vf6f") == (
            "deny", None
        )
        # Same curl, no agent_id -> allow. This is the ORCHESTRATOR's own
        # lifecycle call, the normal case; blocking it would break every gate.
        assert subagent_lifecycle_verdict(lifecycle_cmd, "") == (
            "allow", "no-agent-id"
        )
        assert subagent_lifecycle_verdict(lifecycle_cmd, None) == (
            "allow", "no-agent-id"
        )
        # Whitespace-only agent_id is not an identity -> allow.
        assert subagent_lifecycle_verdict(lifecycle_cmd, "   ") == (
            "allow", "no-agent-id"
        )
        # Non-lifecycle command -> skip even from a subagent.
        assert subagent_lifecycle_verdict("git status", "b1iz1vf6f") == (
            "skip", None
        )
        # A subagent transcript path in the agent_id slot must NOT deny --
        # guards against reintroducing the path-shape assumption by accident.
        assert subagent_lifecycle_verdict("git status", subagent_transcript) == (
            "skip", None
        )
        # main_transcript is retained above for the opt-out cases; reference it
        # here so the fixture stays live rather than becoming dead setup.
        assert isinstance(main_transcript, str)

    # judell/bram#287: subagent worklist-write gate. Same shape of test as
    # subagent_lifecycle_verdict above, but for direct Write/Edit/Bash-redirect
    # writes to the worklist proposal/claim surface itself.
    assert subagent_worklist_write_verdict(WORKLIST_REL, "b1iz1vf6f") == (
        "deny", None
    )
    assert subagent_worklist_write_verdict(
        "resources/worklist-drafts/some-item.md", "b1iz1vf6f"
    ) == ("deny", None)
    # The orchestrator's own writes (no agent_id) must never be blocked.
    assert subagent_worklist_write_verdict(WORKLIST_REL, "") == (
        "skip", "no-agent-id"
    )
    assert subagent_worklist_write_verdict(WORKLIST_REL, None) == (
        "skip", "no-agent-id"
    )
    assert subagent_worklist_write_verdict(WORKLIST_REL, "   ") == (
        "skip", "no-agent-id"
    )
    # Any other target is out of scope regardless of agent_id.
    assert subagent_worklist_write_verdict("app/tools/Main.xmlui", "b1iz1vf6f") == (
        "skip", None
    )

    # judell/bram#262: cross-project direct-edit bypass. A skip-worklist:
    # turn's direct-edit record lands in the SESSION (cwd) project's
    # resources/.worklist-authorization.json, but Write/Edit resolves its
    # project_root from the TARGET file's path -- for a cross-project edit
    # those are two different roots. cross_root_bypass_verdict must consult
    # both: the target root (default, checked first) and the session root,
    # only when it differs from the target root.
    with _tempfile.TemporaryDirectory() as base:
        target_root = os.path.join(base, "target-project")
        session_root = os.path.join(base, "session-project")
        os.makedirs(os.path.join(target_root, "resources"))
        os.makedirs(os.path.join(session_root, "resources"))

        def _write_auth(root, kind="direct-edit", paths=("*",), age_ms=0):
            rec = {
                "kind": kind,
                "paths": list(paths),
                "issuedAtMs": time.time() * 1000 - age_ms,
            }
            with open(os.path.join(root, AUTH_REL), "w", encoding="utf-8") as f:
                json.dump(rec, f)

        def _clear_auth(root):
            p = os.path.join(root, AUTH_REL)
            if os.path.exists(p):
                os.remove(p)

        # resolve_session_root derives the same root find_project_root would
        # for a path inside it, walking up from a nested cwd too, and fails
        # quietly (None) for an unusable or unmanaged cwd.
        _write_auth(session_root)
        assert resolve_session_root({"cwd": session_root}) == os.path.abspath(session_root)
        assert resolve_session_root(
            {"cwd": os.path.join(session_root, "nested", "dir")}
        ) == os.path.abspath(session_root)
        assert resolve_session_root({"cwd": base}) is None  # no marker above base
        assert resolve_session_root({}) is None
        assert resolve_session_root({"cwd": 123}) is None
        _clear_auth(session_root)

        # (a) direct-edit record in target root only -> allow, matched via
        # target -- the existing, unchanged, same-project behavior. Session
        # is never consulted once the target matches.
        _write_auth(target_root)
        matched, via, target_detail, session_detail = cross_root_bypass_verdict(
            target_root, "some/file.txt", session_root
        )
        assert (matched, via, target_detail, session_detail) == (True, "target", "ok", None)
        assert fresh_bypass(target_root, "some/file.txt")
        _clear_auth(target_root)

        # (b) record in the SESSION root only, cwd differs from target ->
        # allow, matched via session. This is the fix: a target-only lookup
        # (the pre-262 behavior) would deny here even though the host wrote
        # a fresh, correct record.
        _write_auth(session_root)
        assert not fresh_bypass(target_root, "some/file.txt")
        matched, via, target_detail, session_detail = cross_root_bypass_verdict(
            target_root, "some/file.txt", session_root
        )
        assert (matched, via, target_detail, session_detail) == (
            True, "session", "absent", "ok",
        )
        _clear_auth(session_root)

        # (c) record in neither -> deny, both lookups run and both report
        # absent, since the roots differ.
        matched, via, target_detail, session_detail = cross_root_bypass_verdict(
            target_root, "some/file.txt", session_root
        )
        assert (matched, via, target_detail, session_detail) == (
            False, None, "absent", "absent",
        )

        # (d) stale record in the session root -> deny; the session lookup
        # names the staleness rather than reporting a false "ok".
        _write_auth(session_root, age_ms=(BYPASS_TTL_SECONDS + 60) * 1000)
        matched, via, target_detail, session_detail = cross_root_bypass_verdict(
            target_root, "some/file.txt", session_root
        )
        assert (matched, via, target_detail, session_detail) == (
            False, None, "absent", "stale",
        )
        _clear_auth(session_root)

        # (e) session root == target root -> the second lookup never runs
        # (degenerate case); behavior is identical to the single-root path
        # in both the matched and unmatched cases.
        _write_auth(target_root)
        matched, via, target_detail, session_detail = cross_root_bypass_verdict(
            target_root, "some/file.txt", target_root
        )
        assert (matched, via, target_detail, session_detail) == (True, "target", "ok", None)
        _clear_auth(target_root)
        matched, via, target_detail, session_detail = cross_root_bypass_verdict(
            target_root, "some/file.txt", target_root
        )
        assert (matched, via, target_detail, session_detail) == (
            False, None, "absent", None,
        )

        # No session_root at all (e.g. cwd absent from payload) -> single
        # lookup, same shape as (e).
        matched, via, target_detail, session_detail = cross_root_bypass_verdict(
            target_root, "some/file.txt", None
        )
        assert (matched, via, target_detail, session_detail) == (
            False, None, "absent", None,
        )


def main():
    payload = json.load(sys.stdin)
    tool_name = payload.get("tool_name", "")

    if tool_name == "Bash":
        ti = payload.get("tool_input", {})
        command = ti.get("command", "") if isinstance(ti, dict) else ""
        preview = command[:200]
        cwd = payload.get("cwd") or os.getcwd()
        if is_gh_body_at_antipattern(command):
            m = _GH_BODY_AT_RX.search(command)
            matched = m.group(0).strip() if m else ""
            _trace_hook(
                "PreToolUse",
                "Bash",
                preview,
                "deny",
                "gh-body-at-antipattern",
                cwd,
            )
            print(
                "gh --body takes a literal string, not stdin or a file reference.\n"
                "Use --body-file - (stdin) or --body-file <path> instead.\n"
                f"Detected: {matched}",
                file=sys.stderr,
            )
            sys.exit(2)
        # Cross-boundary signature. Runs before bash_writes for the same
        # reason the #176 check does: _BASH_WRITE_PATTERNS covers
        # `gh issue ...` but not `gh pr ...` or glab at all, so a later
        # position would miss those surfaces entirely. Non-terminal on
        # pass — the command still faces the worklist gate below.
        cb_verdict, cb_detail = crossboundary_signature_verdict(command, cwd)
        if cb_verdict == "unsigned":
            _trace_hook(
                "PreToolUse",
                "Bash",
                preview,
                "deny",
                "crossboundary-unsigned",
                cwd,
            )
            print(
                "This posts to a repo other than this project's origin, and the "
                "body does not open with the cross-boundary signature.\n"
                "Open the first line with:\n"
                "    <owner>'s <Agent> speaking from the <Project> project:\n"
                "See conventions.md, 'Name the boundary and sign your side' — "
                "every artifact, every comment, not just the first in a thread.",
                file=sys.stderr,
            )
            sys.exit(2)
        elif cb_verdict in ("signed", "unparsed"):
            _trace_hook(
                "PreToolUse",
                "Bash",
                preview,
                "allow",
                "crossboundary-" + cb_verdict + ":" + cb_detail,
                cwd,
            )
        # issue-277 B: full-SHA verification, same trigger and fail-open
        # terms as the signature check above. Non-terminal on pass.
        sha_verdict, sha_detail = forge_sha_verdict(command, cwd)
        if sha_verdict == "bad":
            _trace_hook(
                "PreToolUse",
                "Bash",
                preview,
                "deny",
                "forge-sha-unresolved:" + sha_detail,
                cwd,
            )
            print(
                "The body contains a full commit SHA that does not resolve in "
                "this repository:\n    " + sha_detail + "\n"
                "A fabricated or rebase-orphaned SHA renders as an ordinary "
                "link and 404s silently. Resolve the real hash with "
                "`git rev-parse <short-sha>` (or `git log --oneline`) and "
                "retry. See judell/bram#277.",
                file=sys.stderr,
            )
            sys.exit(2)
        elif sha_verdict in ("ok", "unparsed") and sha_detail != "not-forge-write":
            _trace_hook(
                "PreToolUse",
                "Bash",
                preview,
                "allow",
                "forge-sha-" + sha_verdict + ":" + sha_detail,
                cwd,
            )
        sl_verdict, sl_detail = subagent_lifecycle_verdict(
            command, payload.get("agent_id", "")
        )
        # subagent-lifecycle-check-never-fires step 1: OBSERVE before repairing.
        # The check above tests for a "/subagents/" segment in transcript_path,
        # on an assumption about the payload that was never verified -- and the
        # check has been inert since it shipped (xmlui-org/xmlui-mcp#33, pass 2:
        # a subagent's lifecycle curl reached the host and was stopped by the
        # authorization layer, not here). Log what the payload actually carries
        # whenever the command targets a lifecycle route, so one real call names
        # the discriminator instead of us guessing a second time.
        #
        # KEY NAMES ONLY, never values: the payload carries tool input and a
        # token. `agent_id` / `agent_type` are documented as subagent-specific
        # fields (https://code.claude.com/docs/en/hooks.md) and are the leading
        # candidate, so their presence is reported as a boolean.
        if sl_verdict != "skip":
            _trace_hook(
                "PreToolUse",
                "Bash",
                preview,
                "observe",
                "payload-keys="
                + "|".join(sorted(str(k) for k in payload.keys()))
                + " agent_id="
                + ("yes" if payload.get("agent_id") else "no")
                + " agent_type="
                + ("yes" if payload.get("agent_type") else "no")
                + " tp_has_subagents="
                + ("yes" if "/subagents/" in (payload.get("transcript_path") or "") else "no"),
                cwd,
            )
        if sl_verdict == "deny":
            _trace_hook(
                "PreToolUse",
                "Bash",
                preview,
                "deny",
                "subagent-lifecycle-call",
                cwd,
            )
            print(
                "Worklist lifecycle calls (/__worklist/resolve, "
                "/__worklist/mutate, worklist-commit) stay in the "
                "orchestrator's own turn, after delegated subagents "
                "return. A subagent transcript cannot make them "
                "directly. See 'Delegating worklist items to "
                "subagents' in conventions.md.",
                file=sys.stderr,
            )
            sys.exit(2)
        elif sl_detail == "no-agent-id":
            # The orchestrator's own lifecycle calls land here, which is the
            # normal case, so this is traced rather than treated as anomalous.
            _trace_hook(
                "PreToolUse",
                "Bash",
                preview,
                "allow",
                "subagent-gate-no-agent-id",
                cwd,
            )
        if forge_issue_only_write(command):
            _trace_hook("PreToolUse", "Bash", preview, "allow", "forge-issue-only", cwd)
            sys.exit(0)
        # #299 (case 1): a pure forge read is a read. Asserted explicitly here
        # so the classification is a tested property, not an accident of which
        # verbs the write regexes happen to list.
        if forge_read_command(command) and not bash_writes(command):
            _trace_hook("PreToolUse", "Bash", preview, "allow", "bash-read-only", cwd)
            sys.exit(0)
        if not bash_writes(command):
            # Distinguish "no write pattern matched at all" from "the only
            # redirect target was a no-op sink / temp path" so the new
            # allowance (guard-write-detection-false-positives) is
            # greppable rather than blending into bash-read-only.
            if has_redirect(command) and not redirect_is_write(command):  # #299
                reason = "bash-write-nonrepo-target"
            else:
                reason = "bash-read-only"
            _trace_hook("PreToolUse", "Bash", preview, "allow", reason, cwd)
            sys.exit(0)
        project_root = find_project_root(cwd)
        if project_root is None:
            _trace_hook("PreToolUse", "Bash", preview, "allow", "unmanaged-repo", cwd)
            sys.exit(0)
        # judell/bram#287: same gap as the Write/Edit branch's
        # subagent_worklist_write_verdict, but for a Bash redirect that
        # writes the worklist proposal/claim surface directly -- the
        # lifecycle-channel allow just below is otherwise unconditional
        # (any write-shaped command merely mentioning the drafts path
        # passes, subagent or not). Reuses _redirect_targets, the target
        # extraction _pattern_is_write already builds for redirect_is_write
        # (this branch only runs once bash_writes(command) is True, above),
        # rather than parsing the command a second time. tee / heredoc
        # targets are not extracted anywhere in this guard, so they stay out
        # of scope here rather than growing new parsing for this check. Like
        # subagent_worklist_write_verdict, this fire is still awaited.
        agent_id = payload.get("agent_id", "")
        if isinstance(agent_id, str) and agent_id.strip():
            for raw_target in _redirect_targets(command):
                candidate = (
                    raw_target if os.path.isabs(raw_target)
                    else os.path.join(cwd, raw_target)
                )
                rel_target = normalize_target(project_root, candidate)
                sw_verdict, _ = subagent_worklist_write_verdict(rel_target, agent_id)
                if sw_verdict == "deny":
                    _trace_hook(
                        "PreToolUse", "Bash", preview, "deny",
                        "subagent-worklist-write", cwd,
                    )
                    print(
                        "Worklist proposals and claim edits "
                        "(resources/worklist.json, "
                        "resources/worklist-drafts/*) stay in the "
                        "orchestrator's own turn. A delegated subagent "
                        "cannot write them directly. See 'Delegating "
                        "worklist items to subagents' in conventions.md "
                        "and judell/bram#287.",
                        file=sys.stderr,
                    )
                    sys.exit(2)
        if WORKLIST_DRAFTS_PREFIX in command or ".worklist-intent.json" in command:
            _trace_hook("PreToolUse", "Bash", preview, "allow", "bram-lifecycle-channel", cwd)
            sys.exit(0)
        covered = worklist_covered_files(project_root)
        if covered or fresh_bypass(project_root, "*"):
            _trace_hook("PreToolUse", "Bash", preview, "allow", "covered-by-worklist-item", cwd)
            sys.exit(0)
        if opt_out_clears(project_root, payload, "Bash", preview):
            sys.exit(0)
        # judell/bram#283: worklist-commit prunes on success, so a push
        # right after a gate commit would otherwise find an empty board and
        # get denied. The recent "approved" consumption is the on-disk
        # evidence a gate commit just happened.
        if _PUSH_CMD_RX.search(command) and post_commit_push_grace(project_root):
            _trace_hook(
                "PreToolUse", "Bash", preview, "allow",
                "post-commit-push-grace", cwd,
            )
            sys.exit(0)
        deny_bash_coverage(command, cwd, project_root=project_root)

    if tool_name.startswith("mcp__"):
        os.environ["__BRAM_TRACE_TOOL"] = tool_name
        ti = payload.get("tool_input", {})
        cwd = payload.get("cwd") or os.getcwd()
        if not mcp_is_mutation(tool_name):
            _trace_hook("PreToolUse", tool_name, "", "allow", "mcp-read-only", cwd)
            sys.exit(0)
        project_root = find_project_root(cwd)
        if project_root is None:
            _trace_hook("PreToolUse", tool_name, "", "allow", "unmanaged-repo", cwd)
            sys.exit(0)
        candidates = mcp_paths(ti)
        if not candidates:
            # Mutation-shaped with no path we recognize. Don't guess: deny and
            # name the surface so the guard can be extended deliberately.
            _trace_hook(
                "PreToolUse", tool_name, "", "deny", "mcp-unrecognized-input", cwd
            )
            print(
                f"{tool_name} blocked: this looks like a mutation, but the guard "
                f"could not extract any file path from tool_input.\n"
                f"Propose the change in resources/worklist.json first, or extend "
                f"claude-worklist-guard.py to recognize this tool's input shape.",
                file=sys.stderr,
            )
            sys.exit(2)
        covered = worklist_covered_files(project_root)
        violations = []
        for target in candidates:
            rel = normalize_target(project_root, target)
            if rel is None:
                continue  # outside the project tree
            if rel == WORKLIST_REL:
                # worklist.json mutations must go through a shape the guard can
                # validate (Write/Edit) or through /__worklist/mutate. An MCP
                # write would skip the prune/version/inline-prose checks.
                _trace_hook(
                    "PreToolUse", tool_name, rel, "deny", "mcp-worklist-write", cwd
                )
                print(
                    f"{tool_name} blocked: edits to {WORKLIST_REL} must use "
                    f"Write/Edit (so the guard can validate prunes, the version "
                    f"bump, and inline prose) or /__worklist/mutate for "
                    f"mechanical transitions.",
                    file=sys.stderr,
                )
                sys.exit(2)
            # judell/bram#287: same subagent gate as the Write/Edit and
            # Bash-redirect branches. WORKLIST_REL is already denied for
            # every MCP caller above; without this, a subagent's MCP write
            # to worklist-drafts/* would pass through is_lifecycle_path's
            # blanket allow just below.
            sw_verdict, _ = subagent_worklist_write_verdict(
                rel, payload.get("agent_id", "")
            )
            if sw_verdict == "deny":
                _trace_hook(
                    "PreToolUse", tool_name, rel, "deny",
                    "subagent-worklist-write", cwd,
                )
                print(
                    "Worklist proposals and claim edits "
                    "(resources/worklist.json, resources/worklist-drafts/*) "
                    "stay in the orchestrator's own turn. A delegated "
                    "subagent cannot write them directly. See 'Delegating "
                    "worklist items to subagents' in conventions.md and "
                    "judell/bram#287.",
                    file=sys.stderr,
                )
                sys.exit(2)
            if is_lifecycle_path(rel):
                continue
            if rel in covered:
                continue
            if fresh_bypass(project_root, rel):
                continue
            violations.append(rel)
        if violations:
            if opt_out_clears(project_root, payload, tool_name, violations[0]):
                sys.exit(0)
            deny_coverage(violations[0], opt_out_attempted=False, project_root=project_root)
        _trace_hook(
            "PreToolUse", tool_name, ",".join(candidates)[:120], "allow",
            "covered-by-worklist-item", cwd,
        )
        sys.exit(0)

    if tool_name not in ("Write", "Edit"):
        sys.exit(0)
    # Stash for downstream deny-path trace calls (deny_coverage and
    # deny_mechanical_worklist_change don't otherwise have tool_name).
    os.environ["__BRAM_TRACE_TOOL"] = tool_name

    ti = payload.get("tool_input", {})
    fp = ti.get("file_path", "")
    if not isinstance(fp, str) or not fp:
        sys.exit(0)

    # Locate the project root via the managed-repo marker. If the file isn't
    # inside a Bram-managed project at all, exit cleanly — this
    # hook is a no-op for Claude sessions in unmanaged repos.
    project_root = find_project_root(os.path.dirname(fp) or ".")
    if project_root is None:
        sys.exit(0)

    rel = normalize_target(project_root, fp)
    if rel is None:
        # Target is outside the project tree (e.g., editing files in
        # ~/.codex/ or /tmp/). The worklist gate doesn't apply.
        sys.exit(0)

    # judell/bram#287: subagent worklist-write gate. Must run BEFORE the
    # lifecycle-channel short-circuits just below -- both worklist.json and
    # worklist-drafts/*.md are treated as implicitly-authorized "lifecycle
    # channel" paths with no agent_id test, which is exactly the gap this
    # closes. See subagent_worklist_write_verdict for the full rationale.
    sw_verdict, sw_detail = subagent_worklist_write_verdict(
        rel, payload.get("agent_id", "")
    )
    if sw_verdict == "deny":
        _trace_hook(
            "PreToolUse",
            tool_name,
            rel,
            "deny",
            "subagent-worklist-write",
        )
        print(
            "Worklist proposals and claim edits (resources/worklist.json, "
            "resources/worklist-drafts/*) stay in the orchestrator's own "
            "turn. A delegated subagent cannot write them directly -- it "
            "can widen its own item's coverage, author new items, or edit "
            "claim state with no denial. See 'Delegating worklist items to "
            "subagents' in conventions.md and judell/bram#287.",
            file=sys.stderr,
        )
        sys.exit(2)

    # Pre-Branch-1: lifecycle coordination paths the user already authorized
    # implicitly. Emit allow-via-JSON so Claude does not surface a native
    # permission menu on these bookkeeping writes. worklist.json itself
    # is excluded so Branch 1 can still validate mechanical state changes.
    if rel != WORKLIST_REL and is_lifecycle_path(rel):
        emit_allow_for_lifecycle(rel, "bram-lifecycle-channel")

    # Branch 1: writes to resources/worklist.json — existing prune validation.
    if rel == WORKLIST_REL:
        if not os.path.exists(fp):
            emit_allow_for_lifecycle(rel, "worklist-bootstrap")
        with open(fp, encoding="utf-8") as f:
            old = f.read()
        if payload["tool_name"] == "Write":
            new = ti.get("content", "")
        else:
            o = ti.get("old_string", "")
            n = ti.get("new_string", "")
            new = old.replace(o, n) if ti.get("replace_all") else old.replace(o, n, 1)
        old_items = items_by_id(old)
        new_items = items_by_id(new)
        removed, status_changed = worklist_state_changes(old_items, new_items)
        if not removed and not status_changed:
            inline_bad = worklist_items_with_inline_prose(new)
            if inline_bad:
                deny_inline_prose(inline_bad)
            # Optimistic-concurrency check: once the on-disk file carries
            # a `version` field, every write must bump it by exactly 1.
            # Skip the check when the on-disk file has no version yet
            # (legacy file, pre-migration); a new write that introduces
            # the field at version=1 is the natural migration path.
            old_has_version, old_version = worklist_version_from_text(old)
            new_has_version, new_version = worklist_version_from_text(new)
            if old_has_version and (
                not new_has_version or new_version != old_version + 1
            ):
                deny_stale_worklist_version(
                    old_version, new_version, new_has_version
                )
            emit_allow_for_lifecycle(rel, "worklist-author")
        deny_mechanical_worklist_change(removed, status_changed)

    # Branch 2: worklist draft prose files (already covered by the
    # pre-Branch-1 lifecycle short-circuit; left as a guardrail).
    if is_worklist_draft(rel):
        emit_allow_for_lifecycle(rel, "worklist-draft")

    # Branch 2: writes to any other project file — require worklist coverage,
    # fresh bypass, or explicit opt-out language in the last user message.
    covered = worklist_covered_files(project_root)
    if rel in covered:
        _trace_hook("PreToolUse", tool_name, rel, "allow", "covered-by-worklist-item")
        sys.exit(0)
    # issue-262: consult both the target project's own bypass record and,
    # when it differs, the session (cwd) project's -- a skip-worklist:
    # direct-edit record lands in the ACTIVE project, which for a
    # cross-project edit is the session root, not this target's.
    session_root = resolve_session_root(payload)
    bypass_ok, bypass_via, target_bypass_detail, session_bypass_detail = (
        cross_root_bypass_verdict(project_root, rel, session_root)
    )
    if bypass_ok:
        reason = (
            "fresh-bypass" if bypass_via == "target"
            else "fresh-bypass:session-root=" + session_root
        )
        _trace_hook("PreToolUse", tool_name, rel, "allow", reason)
        sys.exit(0)
    if opt_out_clears(project_root, payload, tool_name, rel):
        sys.exit(0)
    last_msg = last_user_text(payload.get("transcript_path", ""))
    deny_coverage(
        rel, project_root=project_root,
        opt_out_attempted=(
            "worklist" in (last_msg or "").lower()
            and "no" in (last_msg or "").lower()
        ),
        session_root=session_root, session_bypass_detail=session_bypass_detail,
    )


if __name__ == "__main__":
    if len(sys.argv) > 1 and sys.argv[1] == "--self-test":
        self_test()
        print("worklist-guard self-test passed")
        sys.exit(0)
    try:
        main()
    except SystemExit:
        # main() signals its decision by exiting; never convert that.
        raise
    except BaseException as exc:  # noqa: BLE001 - deliberate catch-all
        # Fail closed. A crashing guard exits non-zero-but-not-2, which the
        # provider reads as "hook failed, proceed" -- an enforcement bypass
        # that leaves no receipt, because the crash happens upstream of the
        # guard's own instrumentation. That is exactly how the cp1252
        # transcript crash hid on Windows (judell/bram#249): registered,
        # reported healthy by Setup and Status, never once enforcing.
        #
        # Blocking on a bug is a visible nuisance reported in minutes;
        # allowing on a bug is an invisible hole. Prefer the nuisance. The
        # permission-menu hook deliberately stays fail-open: it is
        # observe-only and must never gate a tool call.
        summary = "%s: %s" % (type(exc).__name__, exc)
        try:
            _trace_hook(
                os.environ.get("__BRAM_TRACE_EVENT", "PreToolUse"),
                os.environ.get("__BRAM_TRACE_TOOL", ""),
                "",
                "error",
                summary[:200],
            )
        except BaseException:
            pass
        sys.stderr.write(
            "Blocked: the Bram worklist guard failed and denied by default."
            + NL
            + "  - "
            + summary[:500]
            + NL
            + "  - This is a guard bug, not a policy decision. Please report it."
            + NL
        )
        sys.exit(2)
