# Build Bram (debug) on Windows and log the build: the Windows twin of
# scripts/build.sh.
#
#   powershell -NoProfile -File scripts\build.ps1 [extra cargo args]
#
# Runs VsDevCmd (-arch=x64), then
#   cargo build --manifest-path src-tauri\Cargo.toml --target x86_64-pc-windows-msvc
# from the repo root. Cargo's output and exit status pass through.
#
# Before building:
#   - deletes earlier bram.exe.old-* copies in debug\ and debug\deps\ that no
#     process holds (a copy still in use is skipped and counted);
#   - measures the target dir's bytes, the deps file count, free bytes on the
#     target's volume, and the age of a stale sibling target\debug tree, and
#     warns when free space is under BRAM_DISK_WARN_GB (default 5) or the target
#     dir is over BRAM_TARGET_WARN_GB (default 20);
#   - when a running Bram holds debug\bram.exe or debug\deps\bram.exe, renames
#     BOTH aside to bram.exe.old-<yyyyMMddHHmmss>. Cargo links deps\bram.exe and
#     uplifts it to debug\bram.exe; moving only the debug name left the linker
#     failing with LNK1104 on deps\bram.exe (2026-11-03). It refuses (exit 2)
#     when free space is below 2 GB, or below the largest free-space drop any
#     logged build recorded (free_bytes_used), whichever is larger: a build
#     that then fails for lack of space would strand the launch with no exe.
#     If the build fails anyway and left no exe, the aside copies are moved back.
#
# After building, appends one JSON line to <state>\build-log.jsonl (state is
# .bram\ when it exists under the repo root, else resources\; the rule in
# scripts/bram_state.py). Field names match build.sh where they overlap.
#
# It never cleans: a clean forces a full rebuild, which is someone's choice to
# make (build-warns-on-disk-and-target-size; #452).

$ErrorActionPreference = "Continue"

$repoRoot = Split-Path -Parent (Split-Path -Parent $PSCommandPath)
$vsDevCmd = "C:\Program Files (x86)\Microsoft Visual Studio\2022\BuildTools\Common7\Tools\VsDevCmd.bat"
$triple = "x86_64-pc-windows-msvc"

if ($env:CARGO_TARGET_DIR) { $targetDir = $env:CARGO_TARGET_DIR } else { $targetDir = Join-Path $repoRoot "src-tauri\target" }
$debugDir = Join-Path $targetDir "$triple\debug"
$depsDir = Join-Path $debugDir "deps"
$staleTree = Join-Path $targetDir "debug"

# State folder: the same rule as scripts/bram_state.py, inlined.
$stateDir = Join-Path $repoRoot "resources"
$newState = Join-Path $repoRoot ".bram"
if (Test-Path -LiteralPath $newState -PathType Container) {
  $home1 = [System.IO.Path]::GetFullPath($env:USERPROFILE).TrimEnd('\')
  $root1 = [System.IO.Path]::GetFullPath($repoRoot).TrimEnd('\')
  if ($home1 -ne $root1) { $stateDir = $newState }
}
$logPath = Join-Path $stateDir "build-log.jsonl"

function Write-Warn([string]$msg) { [Console]::Error.WriteLine("[build.ps1] $msg") }

function Get-EnvNumber([string]$name, [double]$default) {
  $v = [Environment]::GetEnvironmentVariable($name)
  $n = 0.0
  if ($v -and [double]::TryParse($v, [ref]$n)) { return $n }
  return $default
}

# Bytes under a directory, without following junctions or symlinks (debug\app
# is a junction to the checkout's app\).
function Get-TreeBytes([string]$path) {
  if (-not (Test-Path -LiteralPath $path -PathType Container)) { return $null }
  [long]$total = 0
  $stack = New-Object System.Collections.Stack
  $stack.Push((New-Object System.IO.DirectoryInfo($path)))
  while ($stack.Count -gt 0) {
    $dir = $stack.Pop()
    try { $entries = $dir.GetFileSystemInfos() } catch { continue }
    foreach ($e in $entries) {
      if ($e -is [System.IO.DirectoryInfo]) {
        if (($e.Attributes -band [System.IO.FileAttributes]::ReparsePoint) -eq 0) { $stack.Push($e) }
      } else {
        $total += $e.Length
      }
    }
  }
  return $total
}

function Get-FileCount([string]$path) {
  if (-not (Test-Path -LiteralPath $path -PathType Container)) { return $null }
  try { return @([System.IO.Directory]::GetFileSystemEntries($path)).Count } catch { return $null }
}

function Get-FreeBytes([string]$path) {
  try {
    $full = [System.IO.Path]::GetFullPath($path)
    $drive = New-Object System.IO.DriveInfo([System.IO.Path]::GetPathRoot($full))
    return [long]$drive.AvailableFreeSpace
  } catch { return $null }
}

# Days since the sibling target\debug tree was last written (its newest
# top-level entry). The Windows launch builds and runs the target-triple tree,
# so this one only grows stale.
function Get-StaleTreeDays([string]$path) {
  if (-not (Test-Path -LiteralPath $path -PathType Container)) { return $null }
  $newest = $null
  foreach ($e in (Get-ChildItem -LiteralPath $path -Force -ErrorAction SilentlyContinue)) {
    if ($e.Name -eq ".cargo-lock") { continue }
    if ($null -eq $newest -or $e.LastWriteTime -gt $newest) { $newest = $e.LastWriteTime }
  }
  if ($null -eq $newest) { return $null }
  return [math]::Round(((Get-Date) - $newest).TotalDays, 1)
}

function Test-InUse([string]$path) {
  if (-not (Test-Path -LiteralPath $path -PathType Leaf)) { return $false }
  try {
    $fs = [System.IO.File]::Open($path, [System.IO.FileMode]::Open, [System.IO.FileAccess]::ReadWrite, [System.IO.FileShare]::None)
    $fs.Close()
    return $false
  } catch { return $true }
}

function Format-GB($bytes) {
  if ($null -eq $bytes) { return "?" }
  return ("{0:N1} GB" -f ($bytes / 1GB))
}

if (-not (Test-Path -LiteralPath $vsDevCmd)) {
  Write-Warn "Visual Studio Developer Command Prompt not found: $vsDevCmd"
  exit 1
}

# Previous log lines: the last one for head/tree deltas, all of them for the
# largest recorded free-space drop.
$prev = $null
$maxUsed = [long]0
if (Test-Path -LiteralPath $logPath) {
  foreach ($line in (Get-Content -LiteralPath $logPath -Encoding UTF8)) {
    if (-not $line.Trim()) { continue }
    try { $rec = $line | ConvertFrom-Json } catch { continue }
    $prev = $rec
    if ($rec.PSObject.Properties["free_bytes_used"] -and $rec.free_bytes_used -and [long]$rec.free_bytes_used -gt $maxUsed) {
      $maxUsed = [long]$rec.free_bytes_used
    }
  }
}

# 1. Delete earlier aside copies that nothing holds.
$oldDeleted = 0
$oldHeld = 0
foreach ($dir in @($debugDir, $depsDir)) {
  if (-not (Test-Path -LiteralPath $dir)) { continue }
  foreach ($f in (Get-ChildItem -LiteralPath $dir -Filter "bram.exe.old-*" -File -Force -ErrorAction SilentlyContinue)) {
    try {
      [System.IO.File]::Delete($f.FullName)
      $oldDeleted++
    } catch {
      $oldHeld++
    }
  }
}

# 2. Measure and warn.
$targetBytes = Get-TreeBytes $targetDir
$freeBefore = Get-FreeBytes $targetDir
$staleDays = Get-StaleTreeDays $staleTree
$diskWarnGB = Get-EnvNumber "BRAM_DISK_WARN_GB" 5
$targetWarnGB = Get-EnvNumber "BRAM_TARGET_WARN_GB" 20

$remedy = "Remedies: cargo clean --manifest-path src-tauri/Cargo.toml --profile dev"
if ($null -ne $staleDays) {
  $remedy += "; or delete the stale src-tauri\target\debug tree (last written $staleDays days ago; the Windows launch never uses it)"
}
if ($null -ne $freeBefore -and $freeBefore -lt ($diskWarnGB * 1GB)) {
  Write-Warn ("only {0} free on the target's volume (warn under {1} GB, BRAM_DISK_WARN_GB). A full disk fails the link and breaks traces and worklist writes. {2}" -f (Format-GB $freeBefore), $diskWarnGB, $remedy)
}
if ($null -ne $targetBytes -and $targetBytes -gt ($targetWarnGB * 1GB)) {
  Write-Warn ("src-tauri\target holds {0} (warn over {1} GB, BRAM_TARGET_WARN_GB). {2}" -f (Format-GB $targetBytes), $targetWarnGB, $remedy)
}

# 3. Move a running Bram's exe aside, both names.
$exeNames = @((Join-Path $debugDir "bram.exe"), (Join-Path $depsDir "bram.exe"))
$held = $false
foreach ($p in $exeNames) { if (Test-InUse $p) { $held = $true } }
$renamed = @()
if ($held) {
  $minFree = [long][math]::Max(2GB, $maxUsed)
  if ($null -ne $freeBefore -and $freeBefore -lt $minFree) {
    Write-Warn ("a running Bram holds bram.exe, and only {0} is free (need {1} to build safely). Not renaming it aside: a build that fails for space would leave no exe to relaunch. Free space first. {2}" -f (Format-GB $freeBefore), (Format-GB $minFree), $remedy)
    exit 2
  }
  $stamp = Get-Date -Format "yyyyMMddHHmmss"
  foreach ($p in $exeNames) {
    if (-not (Test-Path -LiteralPath $p -PathType Leaf)) { continue }
    $dest = "$p.old-$stamp"
    try {
      [System.IO.File]::Move($p, $dest)
      $renamed += $dest
      Write-Warn "renamed aside (a running Bram holds it): $dest"
    } catch {
      Write-Warn "could not rename $p aside: $($_.Exception.Message)"
    }
  }
}

# 4. Build.
$tree = (git -C $repoRoot stash create 2>$null)
if (-not $tree) { $tree = (git -C $repoRoot rev-parse HEAD 2>$null) }
$head = (git -C $repoRoot rev-parse HEAD 2>$null)
if (-not $head) { $head = "" }
$otherCargo = @(Get-Process -Name cargo, rustc -ErrorAction SilentlyContinue).Count
$caller = $null
try {
  $me = Get-CimInstance Win32_Process -Filter "ProcessId=$PID" -ErrorAction Stop
  $parent = Get-CimInstance Win32_Process -Filter "ProcessId=$($me.ParentProcessId)" -ErrorAction Stop
  $caller = $parent.Name
} catch { $caller = $null }

$extra = ""
foreach ($a in $args) { $extra += " `"$a`"" }
$started = (Get-Date).ToUniversalTime().ToString("yyyy-MM-ddTHH:mm:ssZ")
$sw = [System.Diagnostics.Stopwatch]::StartNew()
Push-Location $repoRoot
try {
  cmd /c "call ""$vsDevCmd"" -arch=x64 && cargo build --manifest-path src-tauri\Cargo.toml --target $triple$extra"
  $status = $LASTEXITCODE
} finally {
  Pop-Location
}
$sw.Stop()

# A failed build that left no exe: move the aside copies back, so the launch
# is not stranded.
if ($status -ne 0 -and $renamed.Count -gt 0) {
  foreach ($dest in $renamed) {
    $orig = $dest -replace '\.old-\d{14}$', ''
    if (-not (Test-Path -LiteralPath $orig)) {
      try { [System.IO.File]::Move($dest, $orig); Write-Warn "build failed; restored $orig" } catch { Write-Warn "build failed; could not restore $orig" }
    }
  }
}

# 5. Log.
$linesChanged = $null
$lockChanged = $null
if ($prev -and $prev.tree -and $tree) {
  $added = 0; $removed = 0; $lockChanged = $false
  foreach ($row in (git -C $repoRoot diff --numstat $prev.tree $tree -- src-tauri 2>$null)) {
    $parts = $row -split "`t"
    if ($parts.Count -lt 3) { continue }
    $n = 0
    if ([int]::TryParse($parts[0], [ref]$n)) { $added += $n }
    if ([int]::TryParse($parts[1], [ref]$n)) { $removed += $n }
    if ($parts[2].EndsWith("Cargo.lock")) { $lockChanged = $true }
  }
  $linesChanged = $added + $removed
}
$headShort = $head
if ($head.Length -gt 12) { $headShort = $head.Substring(0, 12) }
$headMoved = $null
if ($prev) { $headMoved = ($prev.head -ne $headShort) }

$freeAfter = Get-FreeBytes $targetDir
$freeUsed = $null
if ($null -ne $freeBefore -and $null -ne $freeAfter) { $freeUsed = [long]($freeBefore - $freeAfter) }

$rec = [ordered]@{
  started               = $started
  seconds               = [math]::Round($sw.Elapsed.TotalSeconds, 2)
  exit                  = [int]$status
  head                  = $headShort
  head_moved            = $headMoved
  lines_changed         = $linesChanged
  cargo_lock_changed    = $lockChanged
  other_cargo           = $otherCargo
  load                  = $null
  caller                = $caller
  tree                  = $tree
  deps_files            = (Get-FileCount $depsDir)
  target_bytes          = (Get-TreeBytes $targetDir)
  free_bytes            = $freeAfter
  free_bytes_before     = $freeBefore
  free_bytes_used       = $freeUsed
  stale_debug_tree_days = $staleDays
  # Paths relative to the target-triple debug dir.
  renamed_aside         = @($renamed | ForEach-Object { $_.Substring($debugDir.Length).TrimStart('\') })
  old_copies_deleted    = $oldDeleted
  old_copies_held       = $oldHeld
  platform              = "windows"
}

try {
  if (-not (Test-Path -LiteralPath $stateDir)) { New-Item -ItemType Directory -Force -Path $stateDir | Out-Null }
  $json = ConvertTo-Json -InputObject $rec -Compress -Depth 3
  [System.IO.File]::AppendAllText($logPath, $json + "`n", (New-Object System.Text.UTF8Encoding($false)))
} catch {
  Write-Warn "could not append to ${logPath}: $($_.Exception.Message)"
}

if ($oldHeld -gt 0) { Write-Warn "$oldHeld earlier bram.exe.old-* copies are still held by a running process; the next build retries them." }

exit $status
