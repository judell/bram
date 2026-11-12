#Requires -Version 5.1
# Capture Bram lockup state for diagnosis (Windows / PowerShell port of
# capture-bram-state.sh).
#
# 1. Snapshots bram-traces\bram-trace.log, .inflight-claim.json,
#    worklist.json, .worklist-authorization.json from the state folder
#    (.bram\) into a staging dir.
# 2. Filters bram-trace.log to the last Duration (default 1h).
# 3. Prompts the user to export an xs-trace via the Inspector
#    (restarting Bram first if it's wedged).
# 4. After confirmation, picks up the newest xs-trace-*.json from
#    $env:USERPROFILE\Downloads.
# 5. Zips everything into $env:USERPROFILE\Downloads\bram-capture-<timestamp>.zip
#
# Usage:
#   .\scripts\capture-bram-state.ps1 [-Duration 1h]
#
# Duration: lookback for bram-trace.log filtering. Accepts Nm or Nh.
# Default 1h. Run from a Bram project directory (one with .bram\).
#
# Execution policy: Windows blocks unsigned .ps1 by default. Invoke with
#   powershell -ExecutionPolicy Bypass -File .\scripts\capture-bram-state.ps1

[CmdletBinding()]
param(
    [Parameter(Position=0)]
    [string]$Duration = '1h'
)

$ErrorActionPreference = 'Stop'

# Parse duration
if ($Duration -match '^(\d+)m$') {
    $minutes = [int]$Matches[1]
} elseif ($Duration -match '^(\d+)h$') {
    $minutes = [int]$Matches[1] * 60
} else {
    Write-Error "Bad duration: $Duration (use Nm or Nh, e.g. 30m or 2h)"
    exit 1
}

# The state folder, by the host's rule (bram_state_dir in src-tauri/src/lib.rs):
# .bram, when it is Bram's (it carries .migrated-from, or a .gitignore that is
# exactly `*`; judell/bram#469) and this isn't the home directory, where .bram
# is the machine folder. Since 0.8.7 nothing reads the pre-0.8.4 resources\.
$state = '.bram'
$bramOwned = (Test-Path '.bram/.migrated-from') -or
    ((Test-Path '.bram/.gitignore' -PathType Leaf) -and ((Get-Content -Raw '.bram/.gitignore').Trim() -eq '*'))
if (-not ((Test-Path '.bram' -PathType Container) -and $bramOwned -and ((Get-Location).Path -ne $HOME))) {
    Write-Error "No Bram .bram\ in $PWD - run from a Bram project directory"
    exit 1
}

$stamp = (Get-Date).ToString('yyyyMMddTHHmmss')
$workdir = Join-Path ([System.IO.Path]::GetTempPath()) ("bram-capture-" + [guid]::NewGuid().ToString('N').Substring(0,8))
$stage = Join-Path $workdir "bram-capture-$stamp"
New-Item -ItemType Directory -Path $stage -Force | Out-Null

try {
    $cutoff = (Get-Date).ToUniversalTime().AddMinutes(-$minutes).ToString("yyyy-MM-ddTHH:mm:ssZ")

    Write-Host "Capturing pre-restart state to $stage"
    Write-Host "  (trace cutoff: $cutoff, $minutes minutes back)"
    Write-Host ""

    # Snapshot the JSON state files
    foreach ($f in '.inflight-claim.json','worklist.json','.worklist-authorization.json') {
        $src = Join-Path $state $f
        if (Test-Path $src -PathType Leaf) {
            Copy-Item $src (Join-Path $stage $f)
            Write-Host "  + $f"
        } else {
            Write-Host "  - $f (not present)"
        }
    }

    # Filter and snapshot bram-trace.log. Mirrors the awk in the bash sibling:
    # set keep on every line that starts with [YYYY...]; continuation lines
    # inherit the prior keep state.
    $traceSrc = Join-Path (Join-Path $state 'bram-traces') 'bram-trace.log'
    if (Test-Path $traceSrc -PathType Leaf) {
        $traceDst = Join-Path $stage 'bram-trace.log'
        $script:keep = $false
        $filtered = Get-Content $traceSrc | ForEach-Object {
            if ($_ -match '^\[([0-9][^\]]*)\]') {
                $ts = $Matches[1]
                $script:keep = ($ts -ge $cutoff)
            }
            if ($script:keep) { $_ }
        }
        if ($null -eq $filtered) { $filtered = @() }
        $filtered | Set-Content -Path $traceDst -Encoding UTF8
        $kept = @($filtered).Count
        Write-Host "  + bram-trace.log ($kept lines since $cutoff)"
    } else {
        Write-Host "  - bram-trace.log (not present)"
    }

    # Capture environment
    $gitHead = 'n/a'
    $gitBranch = 'n/a'
    try {
        $h = & git rev-parse --short HEAD 2>$null
        if ($LASTEXITCODE -eq 0 -and $h) { $gitHead = $h.Trim() }
        $b = & git rev-parse --abbrev-ref HEAD 2>$null
        if ($LASTEXITCODE -eq 0 -and $b) { $gitBranch = $b.Trim() }
    } catch { }
    @(
        "stamp: $stamp"
        "pwd: $PWD"
        "duration: $Duration (cutoff $cutoff)"
        "os: $([System.Environment]::OSVersion.VersionString)"
        "powershell: $($PSVersionTable.PSVersion)"
        "git: $gitHead on $gitBranch"
    ) | Set-Content -Path (Join-Path $stage 'capture-meta.txt') -Encoding UTF8

    Write-Host ""
    Write-Host "Pre-restart state captured. Now export the xs-trace:"
    Write-Host ""
    Write-Host "  1. Open the Inspector (magnifying-glass icon, top-right of right pane)"
    Write-Host "  2. Click Export - it writes xs-trace-<timestamp>.json to $env:USERPROFILE\Downloads"
    Write-Host ""
    Write-Host "  If Bram is wedged so badly the Inspector won't respond, restart Bram"
    Write-Host "  first - but note that exporting from a fresh session won't capture"
    Write-Host "  the locked-up state."
    Write-Host ""
    $null = Read-Host "Press Enter once the xs-trace is in $env:USERPROFILE\Downloads (or Ctrl-C to skip)"

    # Pick up newest xs-trace
    $downloads = Join-Path $env:USERPROFILE 'Downloads'
    $latest = Get-ChildItem -Path (Join-Path $downloads 'xs-trace-*.json') -ErrorAction SilentlyContinue |
              Sort-Object LastWriteTime -Descending |
              Select-Object -First 1
    if ($latest) {
        Copy-Item $latest.FullName (Join-Path $stage $latest.Name)
        Write-Host "  + $($latest.Name)"
    } else {
        Write-Host "  - no xs-trace-*.json found in $downloads (continuing)"
    }

    # Zip everything (top-level entry in the archive is bram-capture-$stamp/)
    $out = Join-Path $downloads "bram-capture-$stamp.zip"
    if (Test-Path $out) { Remove-Item $out -Force }
    Compress-Archive -Path $stage -DestinationPath $out

    Write-Host ""
    Write-Host "Wrote $out"
    Get-Item $out | Format-Table @{n='Size';e={'{0:N0}' -f $_.Length}}, Name -HideTableHeaders
}
finally {
    if (Test-Path $workdir) {
        Remove-Item $workdir -Recurse -Force -ErrorAction SilentlyContinue
    }
}
