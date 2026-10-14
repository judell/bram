$ErrorActionPreference = "Stop"

$scriptDir = Split-Path -Parent $MyInvocation.MyCommand.Path
$repoRoot = Split-Path -Parent $scriptDir
$engine = Join-Path $scriptDir "demo_instance.py"
$launching = @($args) -contains "launch"

if ($launching) {
  $defaultExe = Join-Path $repoRoot "src-tauri\target\x86_64-pc-windows-msvc\debug\bram.exe"
  $exe = if ($env:BRAM_BIN) { $env:BRAM_BIN } else { $defaultExe }
  $guard = Join-Path (Split-Path -Parent $exe) "bram-guard.exe"
  if (-not (Test-Path -LiteralPath $exe -PathType Leaf)) {
    throw "Debug Bram not found at $exe. Run .\build.ps1 from a VS Developer PowerShell (VsDevCmd) first."
  }

  $running = @(Get-Process bram -ErrorAction SilentlyContinue)
  # The binary depends only on src-tauri/ (app/** is served from disk through
  # the junction), so compare it with the newest git-tracked file there. The
  # old yardstick, HEAD's commit time, refused after scripts- or docs-only
  # commits and missed uncommitted Rust edits (demo-launcher-staleness-from-
  # rust-sources; demo_instance.py's assert_binary_current is the twin).
  $tracked = @(& git -C $repoRoot ls-files src-tauri 2>$null)
  if ($LASTEXITCODE -eq 0 -and $tracked.Count) {
    $newestRel = $null
    $newestTime = [datetime]::MinValue
    foreach ($rel in $tracked) {
      $full = Join-Path $repoRoot $rel
      if (-not (Test-Path -LiteralPath $full -PathType Leaf)) { continue }  # deleted in the worktree
      $t = (Get-Item -LiteralPath $full).LastWriteTime
      if ($t -gt $newestTime) { $newestTime = $t; $newestRel = $rel }
    }
    $exeInfo = Get-Item -LiteralPath $exe
    if ($newestRel -and $exeInfo.LastWriteTime -lt $newestTime) {
      $pids = if ($running.Count) { $running.Id -join ", " } else { "none" }
      $when = $newestTime.ToString("yyyy-MM-dd HH:mm")
      throw "Stale bram.exe: older than $newestRel (changed $when). Running Bram PIDs: $pids. Stop only the intended PID, then rebuild with .\build.ps1."
    }
  }
  if (Test-Path -LiteralPath $guard -PathType Leaf) {
    $guardInfo = Get-Item -LiteralPath $guard
    $exeInfo = Get-Item -LiteralPath $exe
    if ($guardInfo.LastWriteTime -gt $exeInfo.LastWriteTime.AddSeconds(2)) {
      $pids = if ($running.Count) { $running.Id -join ", " } else { "none" }
      throw "Half-built debug artifacts: bram-guard.exe is newer than locked/stale bram.exe. Running Bram PIDs: $pids. Rebuild from VsDevCmd."
    }
  }

  # A junction is recreated/verified on every run. Unlike an exe hardlink it
  # remains correct when cargo atomically replaces bram.exe; unlike a symlink
  # it needs neither elevation nor Developer Mode.
  $appSource = Join-Path $repoRoot "app"
  $appLink = Join-Path (Split-Path -Parent $exe) "app"
  $existing = Get-Item -LiteralPath $appLink -ErrorAction SilentlyContinue
  if ($existing) {
    $target = @($existing.Target) | Select-Object -First 1
    $resolvedTarget = if ($target) { [IO.Path]::GetFullPath($target) } else { "" }
    if ($resolvedTarget -ne [IO.Path]::GetFullPath($appSource)) {
      throw "Refusing to replace non-demo path at $appLink (target: $resolvedTarget). Remove it deliberately, then rerun."
    }
  } else {
    New-Item -ItemType Junction -Path $appLink -Target $appSource | Out-Null
  }
  if (-not (Test-Path -LiteralPath (Join-Path $appLink "tools\Main.xmlui"))) {
    throw "The app junction did not expose the checkout: $appLink"
  }
  $env:BRAM_DEMO_DEFAULT_BINARY = $exe
}

$python = if ($env:PYTHON) { $env:PYTHON } else { "python" }
& $python $engine @args
exit $LASTEXITCODE
