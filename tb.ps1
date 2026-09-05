$ErrorActionPreference = "Stop"

$repoRoot = Split-Path -Parent $MyInvocation.MyCommand.Path
$exe   = Join-Path $repoRoot "src-tauri\target\x86_64-pc-windows-msvc\debug\bram.exe"
$guard = Join-Path $repoRoot "src-tauri\target\x86_64-pc-windows-msvc\debug\bram-guard.exe"

# Note the target-triple subdirectory: build.ps1 passes
# --target x86_64-pc-windows-msvc, so cargo writes there, NOT to target\debug\.
# A stale target\debug\bram.exe from an older build may still exist and is not
# what you want.
if (-not (Test-Path -LiteralPath $exe)) {
  throw "Build output not found: $exe. Run .\build.ps1 first."
}

# Staleness check. Three separate incidents in one session were all the same
# fault -- a binary older than the commit under test -- and each one produced
# symptoms that looked like product bugs: an unclearable Setup banner, hooks
# comparing against the wrong bundled vintage, and gate results that appeared
# to validate code the running process did not contain. The tell was always an
# mtime predating the commit, and nobody thought to look.
#
# Causes seen: launching an installed binary instead of the build output; and
# `cargo build` silently failing to replace bram.exe because an older instance
# still held the file lock, so a "rebuild + relaunch" produced a new process
# running old code.
#
# If the binary predates HEAD, nothing measured downstream means anything, so
# say so loudly rather than printing a note that scrolls past.
$exeInfo  = Get-Item -LiteralPath $exe
$headDate = $null
try {
  $headIso = & git -C $repoRoot log -1 --format=%cI 2>$null
  if ($LASTEXITCODE -eq 0 -and $headIso) { $headDate = [datetimeoffset]::Parse($headIso).LocalDateTime }
} catch { }

Write-Output ("binary : {0:yyyy-MM-dd HH:mm:ss}  {1:N1} MB  {2}" -f $exeInfo.LastWriteTime, ($exeInfo.Length / 1MB), $exe)
if (Test-Path -LiteralPath $guard) {
  $guardInfo = Get-Item -LiteralPath $guard
  Write-Output ("guard  : {0:yyyy-MM-dd HH:mm:ss}  {1:N1} MB" -f $guardInfo.LastWriteTime, ($guardInfo.Length / 1MB))
} else {
  Write-Warning "No bram-guard.exe artifact; hook spawns fall back to the main binary (expect conhost flashes)."
}

if ($headDate) {
  Write-Output ("HEAD   : {0:yyyy-MM-dd HH:mm:ss}  {1}" -f $headDate, (& git -C $repoRoot log -1 --format='%h %s'))
  if ($exeInfo.LastWriteTime -lt $headDate) {
    Write-Output ""
    Write-Warning ("STALE BINARY: bram.exe ({0:yyyy-MM-dd HH:mm:ss}) is OLDER than HEAD ({1:yyyy-MM-dd HH:mm:ss})." -f $exeInfo.LastWriteTime, $headDate)
    Write-Warning "This process will NOT contain the commit you are testing. Any result you"
    Write-Warning "measure against it is meaningless. Close every running bram.exe (a lingering"
    Write-Warning "instance holds the file lock and makes cargo build fail silently), rebuild,"
    Write-Warning "then relaunch:"
    Write-Warning "    Get-Process bram | Stop-Process -Force ; .\build.ps1 ; .\tb.ps1"
  }
}

# Serve app/ from the checkout, the way `./bram` does on macOS. That symlink is
# load-bearing there: it puts the executable's parent at the repo root, so
# serve_app_file's `exe_dir/app` candidate resolves and all of app/** is served
# from disk per request — edits to .xmlui / helpers.js / vendor go live on a
# pane reload, no rebuild. Launched from the target directory there is no
# adjacent app/, so Bram falls back to the EMBEDDED tree and this dev loop pays
# a full rebuild for a markup-only edit, quietly: the pane renders correctly,
# from an older vintage.
#
# A directory JUNCTION beside the executable is the Windows answer, and the
# choice among three is not arbitrary:
#   - Junction (this): a SIBLING of the exe, so `cargo build` replaces bram.exe
#     and never touches it. Needs no privileges.
#   - Hardlinked bram.exe at the repo root (the literal ./bram analogue):
#     measured working, but cargo replaces the exe by writing a NEW file, so the
#     link would keep pointing at the old inode after any rebuild — a launcher
#     silently running last week's binary while reporting success. Worse than
#     the problem it solves.
#   - Symlink: needs administrator rights or Developer Mode, which would make
#     the default dev loop depend on machine configuration.
#
# Ensured on EVERY run, not created once: `cargo clean` and a fresh clone both
# remove it, and a missing junction silently reverts this loop to embedded
# serving — the exact failure being fixed.
$appLink   = Join-Path (Split-Path -Parent $exe) "app"
$appSource = Join-Path $repoRoot "app"
$servesFromDisk = $false
try {
  $existing = Get-Item -LiteralPath $appLink -ErrorAction SilentlyContinue
  if ($existing -and $existing.Target -and (Split-Path -Parent $existing.Target[0]) -ne $repoRoot) {
    Remove-Item -LiteralPath $appLink -Force -Recurse   # points elsewhere; re-point it
    $existing = $null
  }
  if (-not $existing) {
    New-Item -ItemType Junction -Path $appLink -Target $appSource -ErrorAction Stop | Out-Null
  }
  $servesFromDisk = Test-Path -LiteralPath (Join-Path $appLink "tools\Main.xmlui")
} catch {
  Write-Warning "could not ensure the app/ junction ($_); this launch will serve the EMBEDDED tree."
}

# issue-332: build.rs watches exactly one file under app/, so a markup-only edit
# followed by a build can leave the embed at an older vintage with no error
# anywhere. The binary owns the hash algorithm; we just compare two strings.
# Older binaries lack the flags and skip the check.
#
# Which question this answers depends on what we are about to serve. With the
# junction in place the embedded tree is NOT what this launch renders, so a
# mismatch is not a warning about this run — it is a fact about what a SHIPPED
# binary would serve. Warning about it unconditionally would fire on every
# launch and teach the reader to ignore a preflight that will one day be right.
try {
  $embHash  = (& $exe --embedded-app-hash 2>$null | Select-Object -Last 1)
  $diskHash = (& $exe --hash-app-dir $appSource 2>$null | Select-Object -Last 1)
  if ($embHash -and $diskHash) {
    if ($servesFromDisk) {
      Write-Output ("app/   : served from disk via junction ({0})" -f $appLink)
      if ($embHash -ne $diskHash) {
        # ASCII only inside string literals: PS 5.1 reads a BOM-less .ps1 as
        # ANSI, so a UTF-8 em-dash decodes to three CP1252 chars whose last is
        # a curly quote -- which PowerShell honors as a string terminator and
        # which breaks parsing in a way the error message does not explain.
        # Em-dashes in # comments are harmless; in strings they are not.
        Write-Output ("         (embedded tree is {0}, checkout {1} - affects shipped binaries, not this launch)" -f $embHash, $diskHash)
      }
    } elseif ($embHash -ne $diskHash) {
      Write-Output ""
      Write-Warning ("STALE EMBEDDED app/: binary embeds {0}; on-disk app/ hashes {1}." -f $embHash, $diskHash)
      Write-Warning "This launch serves the embedded tree, so any app/** behavior you observe"
      Write-Warning "reflects the markup baked at the last recompile, not the checkout. Force a"
      Write-Warning "re-embed by rebuilding after any Rust change, or:"
      Write-Warning "    (Get-Item src-tauri\src\lib.rs).LastWriteTime = Get-Date ; .\build.ps1"
    } else {
      Write-Output ("app/   : embedded tree matches on-disk checkout ({0})" -f $embHash)
    }
  }
} catch {
  Write-Warning "embedded-app hash preflight unavailable: $_"
}

# A second instance is the usual cause of a failed rebuild, and of two Bram
# windows disagreeing about shared machine-global state (~/.bram/bram-guard.exe
# is one link, whichever instance ensured it last wins).
$others = @(Get-Process bram -ErrorAction SilentlyContinue)
if ($others.Count -gt 0) {
  Write-Output ""
  Write-Warning ("{0} bram.exe instance(s) already running (PID {1}). They share ~/.bram/bram-guard.exe;" -f $others.Count, ($others.Id -join ', '))
  Write-Warning "the last one to start wins, and a lingering instance blocks the next rebuild."
}

Write-Output ""
& $exe
