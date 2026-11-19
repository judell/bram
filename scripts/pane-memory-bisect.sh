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
#   DRY=1 scripts/pane-memory-bisect.sh           # build and check copies only
#   RUNS=3 scripts/pane-memory-bisect.sh full no-data
#                                    # repeat, interleaved; prints median and range
#
# Variants, each adding one layer:
#   shell     tools/index.html is a blank page: parent shell + xterm only
#   hello     Main.xmlui is <App><Text>hello</Text></App>: XMLUI + helpers.js
#             + Globals.xs + theme
#   no-pages  Main.xmlui with every <Page> inside <Pages> replaced by one empty
#             page: all chrome, Worklist pane and composer, no tab content
#   full      unchanged
#
# And the full pane minus one group of <App>'s direct children (the elements at
# 2-space indent in Main.xmlui; anything nested deeper stays):
#   no-data     every top-level DataSource, PushSource and ChangeListener
#   no-dialogs  the top-level ModalDialogs
#   no-header   AppHeader and NavPanel
#   no-footer   Footer (the composer)
#
# Tracing is forced off in every copy (appGlobals.xsVerbose: false) whatever
# the working tree says, so runs compare with each other. VERBOSE=1 forces it
# on instead.
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
  case "$v" in shell|hello|no-pages|full|no-data|no-dialogs|no-header|no-footer) ;;
    *) echo "unknown variant: $v" >&2; exit 2 ;; esac
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

# Remove <App>'s direct children whose tag matches the regex alternation $2.
# An element starts on a line "  <Tag" (2-space indent) and runs until the next
# line at indent 0-2 that opens something new (`<` not followed by `/`, which
# includes comments) or closes the App. Its attribute lines are indented deeper,
# and its closing `  </Tag>` or `  />` is skipped along with it.
drop_top_level() {
  f="$1/app/tools/Main.xmlui"
  # A closing tag at indent 0-2 that isn't the dropped element's own ends the
  # skip too: some top-level wrappers (a <Theme>) don't indent their children.
  awk -v tags="^  <($2)([[:space:]>/]|\$)" '
    skip && /^ ? ?<[^\/]/   { skip=0 }
    skip && /^ ? ?<\// {
      if ($0 ~ "^  </" cur ">") { skip=0; next }
      skip=0
    }
    !skip && $0 ~ tags      { skip=1; dropped++; cur=$0; sub(/^  </, "", cur); sub(/[[:space:]>\/].*$/, "", cur) }
    !skip { print }
    END { print dropped + 0 > "/dev/stderr" }' "$repo_root/app/tools/Main.xmlui" > "$f" 2> "$1/dropped.txt"
  tail -1 "$f" | grep -q '</App>' || { echo "$(basename "$1"): markup lost its closing </App>" >&2; exit 3; }
  [ "$(cat "$1/dropped.txt")" -gt 0 ] || { echo "$(basename "$1"): matched no elements" >&2; exit 3; }
}

# Is every tag in an .xmlui file closed by its matching tag? XMLUI's markup-error
# page logs nothing to the trace, so a broken variant would otherwise be
# measured as a near-empty pane. Quote-aware: attribute values hold `=>` and `>`.
check_markup() {
  awk 'BEGIN { RS="\001" } {
    s=$0; n=length(s); i=1; depth=0; bad=""
    while (i <= n && bad == "") {
      c=substr(s,i,1)
      if (substr(s,i,4) == "<!--") { j=index(substr(s,i+4),"-->"); if (!j) { bad="unclosed comment"; break } i+=j+6; continue }
      if (c != "<") { i++; continue }
      close_tag = (substr(s,i+1,1) == "/"); k = i + 1 + close_tag; name=""
      while (k <= n && substr(s,k,1) ~ /[A-Za-z0-9_.:-]/) { name = name substr(s,k,1); k++ }
      if (name == "") { i++; continue }
      q=""; self=0
      while (k <= n) {
        ch=substr(s,k,1)
        if (q != "") { if (ch == q) q=""; k++; continue }
        if (ch == "\"" || ch == "\x27") { q=ch; k++; continue }
        if (ch == "/" && substr(s,k+1,1) == ">") { self=1; k+=2; break }
        if (ch == ">") { k++; break }
        k++
      }
      if (close_tag) {
        if (depth == 0 || stack[depth] != name) { bad="</" name "> closes <" (depth ? stack[depth] : "nothing") ">" } else depth--
      } else if (!self) { stack[++depth]=name }
      i=k
    }
    if (bad == "" && depth) bad="<" stack[depth] "> never closed"
    if (bad != "") { print bad; exit 1 }
  }' "$1"
}

make_variant() {
  v=$1; dir="$work/$v"
  rm -rf "$dir"; mkdir -p "$dir"
  cp -R "$repo_root/app" "$dir/app"
  ln -s "$bin" "$dir/bram"
  printf '%s\n' "$v" > "$dir/app/__shell/bisect-variant.txt"
  if [ "${VERBOSE:-0}" = 1 ]; then verbose=true; else verbose=false; fi
  sed -E "s/(\"xsVerbose\"[[:space:]]*:[[:space:]]*)(true|false)/\\1$verbose/" \
    "$repo_root/app/tools/config.json" > "$dir/app/tools/config.json"
  grep -q "\"xsVerbose\": *$verbose" "$dir/app/tools/config.json" || {
    echo "$v: could not set xsVerbose to $verbose in the copy's config.json" >&2; exit 3; }
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
    no-data)    drop_top_level "$dir" "DataSource|PushSource|ChangeListener" ;;
    no-dialogs) drop_top_level "$dir" "ModalDialog" ;;
    no-header)  drop_top_level "$dir" "AppHeader|NavPanel" ;;
    no-footer)  drop_top_level "$dir" "Footer" ;;
    full) ;;
  esac
  case "$v" in shell) ;; *)
    why=$(check_markup "$dir/app/tools/Main.xmlui") || {
      echo "$v: variant markup is broken ($why); not running it" >&2; exit 3; } ;;
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
    END { printf "%-10s %7d %7d %9d %8d %6d %6d\n", v, med(1), med(2), med(3), med(4), loads, errors }'
}

mkdir -p "$work"
if [ "${DRY:-0}" = 1 ]; then
  for v in $variants; do make_variant "$v"; echo "$v: built and checked in $work/$v"; done
  exit 0
fi
runs=${RUNS:-1}
echo "settle ${settle}s per variant, $runs run(s) each, interleaved; work dir $work"
printf '%-10s %7s %7s %9s %8s %6s %6s\n' variant fp_MB dirty reclaim gfx loads errors
: > "$work/rows.txt"
r=1
while [ "$r" -le "$runs" ]; do
  for v in $variants; do
    make_variant "$v"
    run_variant "$v" | tee -a "$work/rows.txt"
  done
  r=$((r + 1))
done

# Several runs: the median and range of each variant's footprint.
if [ "$runs" -gt 1 ]; then
  echo
  printf '%-10s %7s %7s %7s %4s\n' variant median min max n
  for v in $variants; do
    awk -v v="$v" '$1 == v && $2 ~ /^[0-9]+$/ { print $2 }' "$work/rows.txt" | sort -n | awk -v v="$v" '
      { a[++n] = $1 }
      END { if (!n) { printf "%-10s %s\n", v, "no valid runs"; exit }
            m = (n % 2) ? a[(n + 1) / 2] : int((a[n / 2] + a[n / 2 + 1]) / 2)
            printf "%-10s %7d %7d %7d %4d\n", v, m, a[1], a[n], n }'
  done
fi
