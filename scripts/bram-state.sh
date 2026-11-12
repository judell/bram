# Sourced, not run: `. "$(dirname "$0")/bram-state.sh"`.
#
# bram_state_dir <root> prints a Bram project's state folder: <root>/.bram.
# A root at the home directory answers <root>/resources, since ~/.bram is the
# machine folder (the host's rule, src-tauri/src/lib.rs). Since 0.8.7 there is
# no resources/ fallback: an older project's folder is moved at startup, or
# the start is held (#469). Python scripts use scripts/bram_state.py.

bram_state_dir() {
  _bsd_root="${1:-.}"
  if [ -n "${HOME:-}" ] && [ -d "$HOME" ] &&
    [ "$(cd "$_bsd_root" 2>/dev/null && pwd -P)" = "$(cd "$HOME" && pwd -P)" ]; then
    printf '%s\n' "$_bsd_root/resources"
  else
    printf '%s\n' "$_bsd_root/.bram"
  fi
}
