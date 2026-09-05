# Forge adapter (issue #222, phase 1)

Bram's forge integration is CLI-shaped: each forge's own CLI (`gh`,
`glab`) is the adapter, chosen per project by `project_forge()` in
`src-tauri/src/lib.rs`. This document records the decision, the
mapping, and the phase boundaries.

## Architecture: the `ForgeAdapter` trait

One object-safe trait, one impl per forge, zero forge-awareness at
call sites (`src-tauri/src/lib.rs`):

```rust
trait ForgeAdapter: Sync {
    fn label(&self) -> &'static str;
    fn issues_list(&self, root, limit) -> Result<Vec<Value>, String>;
    fn issues_search_fetch(&self, root, query) -> Result<SearchFetch, String>;
    fn issue_view(&self, root, number) -> Result<Value, String>;
    fn issue_comment(&self, root, number, body) -> Result<(), String>;
    fn issue_close(&self, root, number, comment) -> Result<(), String>;
    fn cross_references(&self, root, number) -> Vec<Value>;   // default: empty
    fn commit_url(&self, html_base, sha) -> String;           // default: /commit/
    fn commit_visible(&self, root, slug, sha) -> Result<bool, String>; // default: git-level
}
```

Routes call `forge_adapter(app).method(…)`. Orchestration that is
forge-neutral stays **above** the trait: activity enrichment, the
local-grep search pipeline (fed by `SearchFetch::LocalGrep` vs
`ServerFiltered` — each forge declares its search strategy), and
close-authority policy (the closing-keyword lint and the
close-on-commit dialog are policy, not adapter behavior).

**Adding a forge (e.g. Codeberg / Forgejo) is three changes:**

1. A detection entry in `project_forge` (host match + the `.bram.json`
   `"forge"` override for self-hosted).
2. One `impl ForgeAdapter` shelling out to the forge's CLI (`fj` /
   `tea`), with a `_to_gh_shape` mapper into the internal issue schema
   (gh's `--json` shape is the canonical internal form). The trait
   defaults mean a minimal impl is genuinely minimal: git-level
   visibility, empty cross-references, `/commit/` URLs all come free.
3. A column in the surface-mapping table below.

## Why a CLI adapter, not API crates

- No maintained Rust crate abstracts issues + commit URLs across
  forges at the level Bram needs.
- Per-forge API crates (`octocrab`, `gitlab`) would introduce an auth
  surface Bram currently gets free: the user's own `gh` / `glab`
  login session.
- The two CLIs have broadly parallel command surfaces
  (`issue list/view/create/comment/close`), so the adapter stays a
  set of small dispatch branches rather than a heavy trait hierarchy.

## Detection

`project_forge()`: a top-level `"forge": "github" | "gitlab"` in the
project's `.bram.json` wins (for ambiguous self-hosted remotes);
otherwise the `origin` remote URL containing `gitlab` selects GitLab,
default GitHub. The detected forge is exposed as `forge` on
`/__app-info`.

## Surface mapping (phase 1)

| Capability | GitHub | GitLab |
| --- | --- | --- |
| Issue list | `gh issue list --json …` | `glab issue list --all --output json --per-page 100`, normalized via `glab_issue_to_gh_shape` (`iid`→`number`, `opened`→`OPEN`, `web_url`→`url`, label strings → `{name}` objects) |
| Issue view | `gh issue view --json …` | `glab issue view --output json`, same normalization (`description`→`body`) |
| Issue search | full fetch + local grep with hit highlighting | `glab issue list --search` server-side; results carry empty `hits` (no highlighting yet) |
| Comment | `gh issue comment` | `glab issue note -m` |
| Close with comment | `gh issue close -c` | `glab issue note -m` then `glab issue close` (glab close has no comment flag; a failed note still closes) |
| Commit visible on origin | `gh api repos/<slug>/commits/<sha>` | `git branch -r --contains <sha>` (git-level; the Push updates remote-tracking refs) |
| Close-on-push commit URL | `<base>/commit/<sha>` | `<base>/-/commit/<sha>` |
| Cross-referenced issues | GitHub timeline API | none (quiet empty — enhancement, not contract) |
| Commit author → forge login | `gh api /user` | skipped (email local-part fallback applies) |

Missing `glab` degrades exactly like missing `gh`: empty envelopes plus
stderr lines, never a hard failure.

## Phase-1 bounds (each a candidate follow-up)

- GitLab issue lists cap at one `--per-page 100` page.
- Comments are not expanded in GitLab list/view payloads
  (`commentsCount` carries the number; bodies need a notes fetch).
- **Notifications read** (issue-338 "Awaiting You" forge source): the inbox
  reads `gh api /notifications?participating=true` and verifies a thread's
  latest-comment author, both GitHub-only. GitLab's analogue is the **todos
  API** (`glab api /todos`), an unbuilt follow-up — the adapter gains a
  read-only `notifications`/`todos` capability with the GitHub arm first.
- No search-hit highlighting on GitLab.
- Worklist-history commit-URL resolution (`parse_github_commit_url`)
  remains GitHub-shaped; GitLab history rows show the raw URL.
- **Out of scope by design**: the release/install/update pipeline
  (GitHub Actions workflow, release bodies, install scripts, the
  `/__app-info` update check). Those concern where *Bram itself* is
  hosted, not the user's project.

## Verification status

GitHub: behavior-preserving extraction; existing issue-flow tests pass
unchanged, plus a GitLab close-comment URL test. GitLab: written to
glab's documented surface; `glab` is not installed on the development
machine, so live verification (Issues tab against a GitLab project,
close-on-push closing a GitLab issue) is pending the first
GitLab-hosted dogfood project.
