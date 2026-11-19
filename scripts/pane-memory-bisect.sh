#!/bin/sh
# pane-memory-bisect.sh: where does the agent pane's memory go?
#
# Builds variant copies of app/ in a scratch directory, each beside its own
# `bram` symlink, so a demo instance launched through that symlink serves the
# variant (bram_app_root_candidates in src-tauri/src/lib.rs takes `app/` from
# next to the executable's path, and a symlink keeps its own directory). The
# repo's app/ and any running Bram are untouched. For each variant it launches
# the demo instance, samples the renderer's `footprint` while it settles, and
# prints one row: the median of the last three samples.
#
#   scripts/pane-memory-bisect.sh                 # all four variants
#   scripts/pane-memory-bisect.sh hello full      # some of them
#   SETTLE=300 scripts/pane-memory-bisect.sh      # settle seconds (default 180)
#
# Variants, each adding one layer:
#   shell     tools/index.html is a blank page: parent shell + xterm only
#   hello     Main.xmlui is <App><Text>hello</Text></App>: XMLUI + helpers.js
#             + Globals.xs + theme
#   no-pages  Main.xmlui with every <Page> inside <Pages> replaced by one empty
#             page: all chrome, Worklist pane and composer, no tab content
#   full      unchanged
#
# macOS only (footprint). See docs/developing-bram-reference.md
# (§Where the pane's memory goes).
set -eu

script_dir=$(CDPATH= cd -- "$(dirname -- "$0")" && pwd)
repo_root=$(CDPATH= cd -- "$script_dir/.." && pwd)
demo="$script_dir/demo-instance.sh"
settle=${SETTLE:-180}
interval=30
work="${TMPDIR:-/tmp}"
work="${work%/}/bram-bisect"

if [ "$#" -gt 0 ]; then variants="$*"; else variants="shell hello no-pages full"; fi
for v in $variants; do
  case "$v" in shell|hello|no-pages|full) ;; *) echo "unknown variant: $v" >&2; exit 2 ;; esac
done

command -v footprint >/dev/null 2>&1 || { echo "needs macOS footprint(1)" >&2; exit 2; }
bin=$(readlink "$repo_root/bram" 2>/dev/null || true)
case "$bin" in /*) ;; ?*) bin="$repo_root/$bin" ;; esac
[ -x "$bin" ] || { echo "no executable Bram behind $repo_root/bram" >&2; exit 2; }

status=$("$demo" status 2>/dev/null || true)
demo_repo=$(printf '%s\n' "$status" | sed -n 's/^repo: //p' | head -1)
[ -n "$demo_repo" ] || { echo "no demo instance repo; create one with $demo new" >&2; exit 2; }
if printf '%s\n' "$status" | grep -qE 'pid: [0-9]+'; then
  echo "a demo instance is already running; stop it first ($demo stop)" >&2
  exit 2
fi

# One sample: "total dirty reclaimable graphics" in MB
sample() {
  footprint -p "$1" 2>/dev/null | awk '
    function mb(n, u) { if (u=="KB") return n/1024; if (u=="GB") return n*1024; if (u=="B") return 0; return n }
    /Footprint:/ { for (i=1;i<=NF;i++) if ($i=="Footprint:") t=mb($(i+1), $(i+2)) }
    /WebKit malloc$/ { d=mb($1,$2); r=mb($5,$6) }
    /\(graphics\)$/ && /Owned physical footprint/ { g=mb($1,$2) }
    END { printf "%d %d %d %d\n", t+0.5, d+0.5, r+0.5, g+0.5 }'
}

webcontent_pids() {
  ps -axo pid=,command= | awk '/WebKit\.WebContent$/ { print $1 }' | sort -n
}

make_variant() {
  v=$1; dir="$work/$v"
  rm -rf "$dir"; mkdir -p "$dir"
  cp -R "$repo_root/app" "$dir/app"
  ln -s "$bin" "$dir/bram"
  printf '%s\n' "$v" > "$dir/app/__shell/bisect-variant.txt"
  case "$v" in
    shell)
      printf '<!doctype html><html><head><meta charset="utf-8"><title>bisect shell</title></head><body></body></html>\n' \
        > "$dir/app/tools/index.html" ;;
    hello)
      printf '<App>\n  <Text>hello</Text>\n</App>\n' > "$dir/app/tools/Main.xmlui" ;;
    no-pages)
      # Only the first line that starts with the tag: comments mention <Pages> too.
      awk '
        !done && /^[[:space:]]*<Pages[ >]/ { print; print "      <Page url=\"/worklist2\"><Text>no pages</Text></Page>"; inp=1; next }
        inp && /^[[:space:]]*<\/Pages>/ { inp=0; done=1; print; next }
        inp { next }
        { print }' "$repo_root/app/tools/Main.xmlui" > "$dir/app/tools/Main.xmlui"
      tail -1 "$dir/app/tools/Main.xmlui" | grep -q '</App>' || {
        echo "no-pages: variant markup lost its closing </App>; not running it" >&2; exit 3; } ;;
    full) ;;
  esac
}

run_variant() {
  v=$1; dir="$work/$v"
  before=$(webcontent_pids)
  launched_at=$(date -u +%Y-%m-%dT%H:%M:%S)
  BRAM_BIN="$dir/bram" "$demo" launch >/dev/null 2>&1

  # The renderer: a WebContent process that wasn't there before the launch.
  renderer=""; tries=0
  while [ -z "$renderer" ] && [ "$tries" -lt 30 ]; do
    sleep 1; tries=$((tries + 1))
    renderer=$(webcontent_pids | while read -r p; do
      printf '%s\n' "$before" | grep -qx "$p" || echo "$p"; done | tail -1)
  done

  # Prove the demo serves this variant's app/.
  served=""; tries=0
  while [ "$served" != "$v" ] && [ "$tries" -lt 30 ]; do
    sleep 1; tries=$((tries + 1))
    port=$(cat "$demo_repo/.bram/.bram-port" 2>/dev/null || true)
    [ -n "$port" ] && served=$(curl -4 -sS "http://127.0.0.1:$port/__shell/bisect-variant.txt" 2>/dev/null || true)
  done

  rows=""
  if [ -n "$renderer" ] && [ "$served" = "$v" ]; then
    elapsed=0
    while [ "$elapsed" -lt "$settle" ]; do
      sleep "$interval"; elapsed=$((elapsed + interval))
      rows="$rows$(sample "$renderer")
"
    done
  fi
  "$demo" stop >/dev/null 2>&1 || true

  trace="$demo_repo/.bram/bram-traces/bram-trace.log"
  loads=0; errors=0
  if [ -f "$trace" ]; then
    loads=$(awk -v t="[$launched_at" '$1 >= t' "$trace" | grep -c 'path=__app-info status=200' || true)
    errors=$(awk -v t="[$launched_at" '$1 >= t' "$trace" | grep -E 'console-error|console-unhandledrejection' | grep -vc ResizeObserver || true)
  fi

  if [ -z "$renderer" ]; then
    printf '%-9s %s\n' "$v" "no new renderer found"; return
  fi
  if [ "$served" != "$v" ]; then
    printf '%-9s %s\n' "$v" "demo did not serve this variant (got '${served}')"; return
  fi
  # Median of the last three samples, per column.
  printf '%s' "$rows" | tail -3 | awk -v v="$v" -v loads="$loads" -v errors="$errors" '
    { for (c=1;c<=4;c++) col[c,NR]=$c; n=NR }
    function med(c,  a,b,x) { if (n<3) return col[c,n]; a=col[c,1]; b=col[c,2]; x=col[c,3];
      if ((a-b)*(x-a) >= 0) return a; if ((b-a)*(x-b) >= 0) return b; return x }
    END { printf "%-9s %7d %7d %9d %8d %6d %6d\n", v, med(1), med(2), med(3), med(4), loads, errors }'
}

mkdir -p "$work"
echo "settle ${settle}s per variant; work dir $work"
printf '%-9s %7s %7s %9s %8s %6s %6s\n' variant fp_MB dirty reclaim gfx loads errors
for v in $variants; do
  make_variant "$v"
  run_variant "$v"
done
