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

# issue-332: this raw-binary launch has no adjacent app/, so it serves the
# EMBEDDED app/ tree — and build.rs watches exactly one file under app/, so a
# markup-only edit followed by a build can leave the embed at an older
# vintage with no error anywhere. The binary owns the hash algorithm; we just
# compare two strings. Older binaries lack the flags and skip the check.
try {
  $embHash  = (& $exe --embedded-app-hash 2>$null | Select-Object -Last 1)
  $diskHash = (& $exe --hash-app-dir (Join-Path $repoRoot "app") 2>$null | Select-Object -Last 1)
  if ($embHash -and $diskHash) {
    if ($embHash -ne $diskHash) {
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
