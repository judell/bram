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

switch ([System.Runtime.InteropServices.RuntimeInformation]::OSArchitecture) {
  "X64" { $Artifact = "bram-windows-amd64.zip" }
  default {
    throw "Bram install: unsupported Windows architecture $([System.Runtime.InteropServices.RuntimeInformation]::OSArchitecture)."
  }
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
