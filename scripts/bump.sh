#!/usr/bin/env bash
set -euo pipefail

# scripts/bump.sh — atomic release flow.
#
# Standard release (preflight → edit → build → commit → tag → push → dispatch):
#   scripts/bump.sh 0.1.16
#
# Local-only (commit + tag locally; skip push and workflow dispatch):
#   scripts/bump.sh 0.1.16 --no-push

usage() {
  cat <<'EOF' >&2
usage: bump.sh <version> [--no-push] [--branch=<name>]

  <version>       N.N.N (e.g. 0.1.16; leading v is stripped)
  --no-push       stop after commit+tag locally; don't push or dispatch
  --branch=NAME   expected current branch (default: main)
EOF
  exit 1
}

# --- Parse args -------------------------------------------------------

VERSION=""
PUSH=1
BRANCH="main"
for arg in "$@"; do
  case "$arg" in
    --no-push) PUSH=0 ;;
    --branch=*) BRANCH="${arg#--branch=}" ;;
    -h|--help) usage ;;
    *)
      if [ -z "$VERSION" ]; then
        VERSION="${arg#v}"
      else
        echo "error: unexpected argument: $arg" >&2
        usage
      fi
      ;;
  esac
done

[ -z "$VERSION" ] && usage

if ! [[ "$VERSION" =~ ^[0-9]+\.[0-9]+\.[0-9]+$ ]]; then
  echo "error: version must be N.N.N or vN.N.N (got: $VERSION)" >&2
  exit 1
fi

TAG="v${VERSION}"
cd "$(dirname "$0")/.."

# --- Preflight --------------------------------------------------------

CURRENT_BRANCH=$(git rev-parse --abbrev-ref HEAD)
if [ "$CURRENT_BRANCH" != "$BRANCH" ]; then
  echo "error: expected branch '$BRANCH' but HEAD is on '$CURRENT_BRANCH'" >&2
  echo "       (override with --branch=<name> if this is intentional)" >&2
  exit 1
fi

if ! git diff --quiet || ! git diff --cached --quiet; then
  echo "error: working tree is dirty — commit or stash first" >&2
  git status --short >&2
  exit 1
fi

if [ $PUSH -eq 1 ]; then
  if ! command -v gh >/dev/null; then
    echo "error: gh CLI not found (required for workflow dispatch); use --no-push to skip" >&2
    exit 1
  fi
  if ! gh auth status >/dev/null 2>&1; then
    echo "error: gh CLI not authenticated; run 'gh auth login' or use --no-push" >&2
    exit 1
  fi
fi

echo "Fetching origin..."
git fetch origin --quiet

AHEAD_OF_LOCAL=$(git rev-list "HEAD..origin/$BRANCH" --count)
if [ "$AHEAD_OF_LOCAL" -ne 0 ]; then
  echo "error: local '$BRANCH' is $AHEAD_OF_LOCAL commit(s) behind origin/$BRANCH" >&2
  echo "       pull or rebase before releasing" >&2
  exit 1
fi

if git rev-parse "$TAG" >/dev/null 2>&1; then
  echo "error: tag $TAG already exists locally" >&2
  exit 1
fi

if [ -n "$(git ls-remote --tags origin "$TAG")" ]; then
  echo "error: tag $TAG already exists on origin" >&2
  exit 1
fi

PRE_SHA=$(git rev-parse HEAD)

# --- Rollback helpers -------------------------------------------------

rollback_disk() {
  echo "  rolling back disk changes..." >&2
  git checkout -- src-tauri/Cargo.toml src-tauri/tauri.conf.json src-tauri/Cargo.lock 2>/dev/null || true
}

rollback_commit() {
  echo "  rolling back commit + tag..." >&2
  git tag -d "$TAG" 2>/dev/null || true
  git reset --hard "$PRE_SHA"
}

# --- Edit + build (rollback disk on failure) --------------------------

echo "Bumping to ${VERSION}..."
sed -i.bak "s/^version = \".*\"/version = \"${VERSION}\"/" src-tauri/Cargo.toml
sed -i.bak "s/\"version\": \"[^\"]*\"/\"version\": \"${VERSION}\"/" src-tauri/tauri.conf.json
rm src-tauri/Cargo.toml.bak src-tauri/tauri.conf.json.bak

echo "Refreshing Cargo.lock (cargo build)..."
if ! cargo build --manifest-path src-tauri/Cargo.toml --quiet; then
  echo "error: cargo build failed" >&2
  rollback_disk
  exit 1
fi

# --- Commit + tag (rollback commit on tag failure) --------------------

echo "Committing + tagging..."
git add src-tauri/Cargo.toml src-tauri/Cargo.lock src-tauri/tauri.conf.json
if ! git commit -m "Release ${TAG}"; then
  echo "error: commit failed" >&2
  rollback_disk
  exit 1
fi
if ! git tag "$TAG"; then
  echo "error: tag creation failed" >&2
  rollback_commit
  exit 1
fi

echo "Local: $(git rev-parse --short HEAD) tagged as ${TAG}"

if [ $PUSH -eq 0 ]; then
  echo
  echo "Skipped push and workflow dispatch (--no-push)."
  echo "To finish manually:"
  echo "  git push --atomic origin $BRANCH $TAG"
  echo "  gh workflow run build.yml -f tag=$TAG"
  exit 0
fi

# --- Atomic push (commit + tag together) ------------------------------

echo "Pushing $BRANCH + $TAG (atomic)..."
if ! git push --atomic origin "$BRANCH" "$TAG"; then
  echo "error: push failed" >&2
  echo "       local commit + tag are intact; retry with:" >&2
  echo "         git push --atomic origin $BRANCH $TAG" >&2
  exit 1
fi

# --- Workflow dispatch ------------------------------------------------

echo "Dispatching Build Binaries workflow with tag ${TAG}..."
if ! gh workflow run build.yml -f tag="$TAG"; then
  echo "error: workflow dispatch failed" >&2
  echo "       push succeeded; retry workflow dispatch with:" >&2
  echo "         gh workflow run build.yml -f tag=$TAG" >&2
  exit 1
fi

# --- Summary ----------------------------------------------------------

REPO=$(gh repo view --json nameWithOwner -q .nameWithOwner 2>/dev/null || echo "<owner>/<repo>")
echo
echo "Released ${TAG}."
echo "  Watch: gh run watch"
echo "  Actions: https://github.com/${REPO}/actions"
