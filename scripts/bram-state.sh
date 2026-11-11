# Sourced, not run: `. "$(dirname "$0")/bram-state.sh"`.
#
# bram_state_dir <root> prints a Bram project's state folder: <root>/.bram
# when that directory is Bram's, else <root>/resources. The same rule as the
# host's bram_state_dir (src-tauri/src/lib.rs), including its refusal of a
# .bram/ in the home directory. 0.8.3 accepts both names; 0.8.4 moves the
# folder to .bram/ (#469). Python scripts use scripts/bram_state.py.

# A .bram/ is Bram's when it carries one of Bram's markers: .migrated-from,
# or a .gitignore whose content is exactly `*` (guard_policy::is_bram_state_dir).
_bram_is_state_dir() {
  [ -e "$1/.bram/.migrated-from" ] ||
    { [ -f "$1/.bram/.gitignore" ] && [ "$(tr -d ' \t\r\n' <"$1/.bram/.gitignore")" = "*" ]; }
}

bram_state_dir() {
  _bsd_root="${1:-.}"
  if [ -d "$_bsd_root/.bram" ] && _bram_is_state_dir "$_bsd_root" && ! { [ -n "${HOME:-}" ] && [ -d "$HOME" ] &&
    [ "$(cd "$_bsd_root" 2>/dev/null && pwd -P)" = "$(cd "$HOME" && pwd -P)" ]; }; then
    printf '%s\n' "$_bsd_root/.bram"
  else
    printf '%s\n' "$_bsd_root/resources"
  fi
}
