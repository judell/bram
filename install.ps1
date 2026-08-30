param(
  [string]$Version = "",
  [string]$BaseUrl = "",
  [string]$InstallDir = ""
)

$ErrorActionPreference = "Stop"

if (-not $Version) {
  $Version = if ($env:BRAM_VERSION) { $env:BRAM_VERSION } elseif ($env:XMLUI_DESKTOP_VERSION) { $env:XMLUI_DESKTOP_VERSION } else { "latest" }
}
if (-not $BaseUrl -and $env:BRAM_BASE_URL) {
  $BaseUrl = $env:BRAM_BASE_URL
} elseif (-not $BaseUrl -and $env:XMLUI_DESKTOP_BASE_URL) {
  $BaseUrl = $env:XMLUI_DESKTOP_BASE_URL
}

$Repo = "judell/bram"

if ([System.Environment]::OSVersion.Platform -ne [System.PlatformID]::Win32NT) {
  throw "Bram install: install.ps1 is only supported on Windows."
}

function Get-WindowsArchitecture {
  $Candidates = @(
    [System.Environment]::GetEnvironmentVariable("PROCESSOR_ARCHITEW6432", [System.EnvironmentVariableTarget]::Process),
    [System.Environment]::GetEnvironmentVariable("PROCESSOR_ARCHITECTURE", [System.EnvironmentVariableTarget]::Process)
  )
  foreach ($Candidate in $Candidates) {
    if (-not [string]::IsNullOrWhiteSpace($Candidate)) {
      return $Candidate.Trim().ToUpperInvariant()
    }
  }

  try {
    $RuntimeArchitecture = [string][System.Runtime.InteropServices.RuntimeInformation]::OSArchitecture
    if (-not [string]::IsNullOrWhiteSpace($RuntimeArchitecture)) {
      return $RuntimeArchitecture.Trim().ToUpperInvariant()
    }
  } catch {
    # The process environment probes above support Windows PowerShell 5.1.
  }
  return ""
}

$Architecture = Get-WindowsArchitecture
switch ($Architecture) {
  { $_ -in @("AMD64", "X64") } { $Artifact = "bram-windows-amd64.zip" }
  default {
    $NativeArchitecture = [System.Environment]::GetEnvironmentVariable("PROCESSOR_ARCHITEW6432", [System.EnvironmentVariableTarget]::Process)
    $ProcessArchitecture = [System.Environment]::GetEnvironmentVariable("PROCESSOR_ARCHITECTURE", [System.EnvironmentVariableTarget]::Process)
    throw "Bram install: unsupported Windows architecture '$Architecture' (PROCESSOR_ARCHITEW6432='$NativeArchitecture', PROCESSOR_ARCHITECTURE='$ProcessArchitecture')."
  }
}

if ($env:BRAM_INSTALL_VALIDATE_ONLY -eq "1") {
  Write-Output $Artifact
  return
}

if ($BaseUrl) {
  $ResolvedBaseUrl = $BaseUrl.TrimEnd("/")
} elseif ($Version -eq "latest") {
  $ResolvedBaseUrl = "https://github.com/$Repo/releases/latest/download"
} else {
  $ResolvedBaseUrl = "https://github.com/$Repo/releases/download/$Version"
}

if (-not $InstallDir) {
  $InstallDir = Join-Path $HOME "bin"
}

function Download-File {
  param(
    [Parameter(Mandatory = $true)][string]$Url,
    [Parameter(Mandatory = $true)][string]$Path
  )

  $client = New-Object System.Net.WebClient
  try {
    $client.DownloadFile($Url, $Path)
  } finally {
    $client.Dispose()
  }
}

$TempDir = Join-Path ([System.IO.Path]::GetTempPath()) ("bram-install-" + [System.Guid]::NewGuid().ToString("n"))
New-Item -ItemType Directory -Path $TempDir | Out-Null

try {
  $ArtifactPath = Join-Path $TempDir $Artifact
  $SumsPath = Join-Path $TempDir "SHA256SUMS"

  Write-Host "Downloading $Artifact..."
  Download-File -Url "$ResolvedBaseUrl/$Artifact" -Path $ArtifactPath

  Write-Host "Downloading SHA256SUMS..."
  Download-File -Url "$ResolvedBaseUrl/SHA256SUMS" -Path $SumsPath

  $Expected = $null
  foreach ($line in Get-Content -Path $SumsPath) {
    if ($line -match '^\s*([0-9a-fA-F]{64})\s+\*?(.+?)\s*$' -and $matches[2] -eq $Artifact) {
      $Expected = $matches[1].ToLowerInvariant()
      break
    }
  }
  if (-not $Expected) {
    throw "Bram install: $Artifact not found in SHA256SUMS."
  }

  $Actual = (Get-FileHash -Algorithm SHA256 -Path $ArtifactPath).Hash.ToLowerInvariant()
  if ($Actual -ne $Expected) {
    throw "Bram install: SHA256 mismatch for $Artifact. Expected $Expected, got $Actual."
  }
  Write-Host "SHA256 verified."

  Write-Host "Extracting..."
  Expand-Archive -LiteralPath $ArtifactPath -DestinationPath $TempDir -Force

  $Binary = Get-ChildItem -Path $TempDir -Include "bram.exe","xmlui-desktop.exe" -File -Recurse | Select-Object -First 1
  if (-not $Binary) {
    throw "Bram install: bram.exe not found in archive."
  }

  if (-not (Test-Path $InstallDir)) {
    New-Item -ItemType Directory -Path $InstallDir | Out-Null
  }
  $Target = Join-Path $InstallDir "bram.exe"
  Write-Host "Installing to $Target..."
  Copy-Item -LiteralPath $Binary.FullName -Destination $Target -Force

  # The dedicated hook binary rides beside the app: ~/.bram/bram-guard.exe
  # links to the sibling of the running executable (GUI subsystem, so hook
  # spawns never flash a conhost), so it must land in the same dir. Older
  # archives don't carry it; the app then falls back to itself as the hook
  # target, so its absence is not fatal.
  $GuardBinary = Get-ChildItem -Path $TempDir -Include "bram-guard.exe" -File -Recurse | Select-Object -First 1
  if ($GuardBinary) {
    $GuardTarget = Join-Path $InstallDir "bram-guard.exe"
    Copy-Item -LiteralPath $GuardBinary.FullName -Destination $GuardTarget -Force
    Write-Host "Installed: $GuardTarget"
  }

  # Ensure InstallDir is on the user PATH.
  $UserPath = [System.Environment]::GetEnvironmentVariable("Path", [System.EnvironmentVariableTarget]::User)
  $PathParts = if ($UserPath) { $UserPath.Split(";") } else { @() }
  $AlreadyOnPath = $false
  foreach ($p in $PathParts) {
    if ($p.TrimEnd("\") -ieq $InstallDir.TrimEnd("\")) {
      $AlreadyOnPath = $true
      break
    }
  }
  if (-not $AlreadyOnPath) {
    $NewPath = if ($UserPath) { "$UserPath;$InstallDir" } else { $InstallDir }
    [System.Environment]::SetEnvironmentVariable("Path", $NewPath, [System.EnvironmentVariableTarget]::User)
    Write-Host "Added $InstallDir to user PATH. Open a new PowerShell window for it to take effect."
  }

  Write-Host "Installed: $Target"
} finally {
  Remove-Item -LiteralPath $TempDir -Recurse -Force -ErrorAction SilentlyContinue
}
