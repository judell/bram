# Bram manages the agent in this terminal. It autostarts the configured
# provider on launch (set shell.agent / shell.args in .bram.json) and the
# header switcher / Sessions tab change providers deliberately. Bram tracks
# the current provider host-side, so launching claude or codex by hand here
# does NOT update Bram's UI -- use the header switcher to change providers.
#
# No shell functions wrap claude/codex any more: the host owns current-agent
# state and types launch commands itself.
Write-Host "Bram manages the agent here - use the header switcher to change providers."

# shell-reports-prompt-and-exit-status, Windows half: report this shell's
# foreground edges to the host as private OSC 7779 sequences, byte-identical
# to the ones claude-code-shellrc sends on macOS and Linux:
#   ESC ] 7779 ; bram ; prompt ; <status> BEL  -- back at the prompt; the
#                                                previous command exited <status>
#   ESC ] 7779 ; bram ; exec BEL               -- a command line just started
# xterm.js ignores 7779 (no handler is registered) and strip_ansi removes it.
# The host side is platform-neutral: ShellForeground.seen is false only until
# the first marker arrives, so emitting these is the whole of what moves
# Windows off the prompt-shape guess and onto the same path macOS uses.
#
# Scope note: this file is run as `powershell -NoLogo -NoExit -File`, and a
# -File script's functions DO stay visible to the interactive session that
# follows (verified: a `function prompt` defined here renders as the live
# prompt). `global:` is belt-and-braces, not load-bearing.
function global:__bramEmit([string]$payload) {
  [Console]::Write("$([char]27)]7779;bram;$payload$([char]7)")
}

# Written with [Console]::Write rather than returned inside the prompt string:
# PSReadLine measures the returned text for its redraw column arithmetic, and
# an invisible OSC inside it is a known way to corrupt that. Returning the
# stock prompt unchanged also keeps terminal_attention_prompt_shape's `PS ...>`
# guess working as a second line of defence.
#
# $LASTEXITCODE, not $?: the status that matters is an agent process exiting,
# and claude / codex are native executables, which is exactly what
# $LASTEXITCODE reports. $? is a cmdlet-success boolean, a different question.
function global:prompt {
  $s = if ($null -eq $LASTEXITCODE) { 0 } else { $LASTEXITCODE }
  __bramEmit "prompt;$s"
  "PS $($executionContext.SessionState.Path.CurrentLocation)$('>' * ($nestedPromptLevel + 1)) "
}

# The exec edge. Wrapping PSConsoleHostReadLine is the only hook that fires
# exactly when a command is about to run: it returns the whole submitted
# buffer once, including a multi-line block, so one `exec` fires per submitted
# command -- the same semantics the bash DEBUG trap gets from its
# first-command-after-prompt flag, without needing the flag.
#
# Not an Enter key handler: Enter also fires on an empty line and on a
# continuation inside an unclosed block, so it would announce commands that
# never execute.
#
# Guarded, so a session without PSReadLine keeps today's behaviour instead of
# erroring at startup. Note this edge cannot be exercised with piped stdin --
# PowerShell bypasses PSReadLine entirely there -- so it is validated in a real
# console via scripts\demo-instance.ps1, never by a headless probe.
if (Get-Command PSConsoleHostReadLine -ErrorAction SilentlyContinue) {
  $global:__bramOrigReadLine = (Get-Command PSConsoleHostReadLine).ScriptBlock
  function global:PSConsoleHostReadLine {
    $line = & $global:__bramOrigReadLine
    if ($line -and $line.Trim()) { __bramEmit "exec" }
    $line
  }
}
