<#
.SYNOPSIS
  EchoVena one-line installer for Windows.

  irm https://raw.githubusercontent.com/Ranoes/echovena/main/install.ps1 | iex

  Environment overrides:
    ECHOVENA_VERSION            install a specific tag (e.g. v1.1.0); default: latest
    ECHOVENA_INSTALL_DIR        where to put echovena.exe; default: $env:LOCALAPPDATA\Programs\EchoVena\bin
    ECHOVENA_NO_MODIFY_PATH     set to 1 to skip updating your user PATH
    ECHOVENA_BUILD_FROM_SOURCE  set to 1 to compile via cargo instead of downloading a prebuilt release
    ECHOVENA_INSTALL_YTDLP      set to 1 to auto-install yt-dlp via winget if missing
#>
$ErrorActionPreference = 'Stop'
[Net.ServicePointManager]::SecurityProtocol = [Net.ServicePointManager]::SecurityProtocol -bor [Net.SecurityProtocolType]::Tls12

$Repo       = 'Ranoes/echovena'
$Binary     = 'echovena'
$InstallDir = if ($env:ECHOVENA_INSTALL_DIR) { $env:ECHOVENA_INSTALL_DIR } else { Join-Path $env:LOCALAPPDATA 'Programs\EchoVena\bin' }

function Info($m) { Write-Host "[*] $m" -ForegroundColor DarkGray }
function Ok($m)   { Write-Host "[OK] $m" -ForegroundColor Green }
function Warn($m) { Write-Host "[!] $m" -ForegroundColor Yellow }
function Fail($m) { Write-Host "[X] $m" -ForegroundColor Red; exit 1 }

function Install-FromSource {
  if (-not (Get-Command cargo -ErrorAction SilentlyContinue)) {
    Fail "cargo is not installed. Install Rust from https://rustup.rs and re-run with `$env:ECHOVENA_BUILD_FROM_SOURCE='1'."
  }
  Info "Building EchoVena from source: cargo install --git https://github.com/$Repo.git --locked"
  cargo install --git "https://github.com/$Repo.git" --locked
  if ($LASTEXITCODE -ne 0) {
    Fail "cargo install failed with exit code $LASTEXITCODE."
  }
  Ok "Compiled and installed EchoVena via cargo."
}

Write-Host ""
Write-Host "  ===============================================================" -ForegroundColor Cyan
Write-Host "   EchoVena - Terminal Music Streamer, Sync & 60 FPS Karaoke TUI" -ForegroundColor Cyan
Write-Host "   https://github.com/Ranoes/echovena" -ForegroundColor DarkGray
Write-Host "  ===============================================================" -ForegroundColor Cyan
Write-Host ""

if ($env:ECHOVENA_BUILD_FROM_SOURCE -eq '1') {
  Install-FromSource
} else {
  # --- detect architecture ---------------------------------------------------
  $arch = $env:PROCESSOR_ARCHITECTURE
  if ($arch -eq 'AMD64') {
    # Native x86_64 Windows
  } elseif ($arch -eq 'ARM64') {
    Warn "Installing x86_64 Windows release binary (runs under Windows ARM64 x64 emulation)."
  } else {
    Fail "No prebuilt Windows binary for architecture '$arch'. To compile from source, set `$env:ECHOVENA_BUILD_FROM_SOURCE='1' and re-run, or execute: cargo install --git https://github.com/$Repo.git --locked"
  }

  $asset = "$Binary-windows-x86_64.zip"
  if ($env:ECHOVENA_VERSION) {
    $tag = 'v' + ($env:ECHOVENA_VERSION -replace '^v', '')
    $baseUrl = "https://github.com/$Repo/releases/download/$tag"
    $label = $tag
  } else {
    $baseUrl = "https://github.com/$Repo/releases/latest/download"
    $label = 'latest'
  }

  $downloadUrl = "$baseUrl/$asset"
  $sumsUrl     = "$baseUrl/SHA256SUMS.txt"

  $tmp = Join-Path ([System.IO.Path]::GetTempPath()) ("echovena-" + [System.Guid]::NewGuid().ToString('N'))
  New-Item -ItemType Directory -Path $tmp -Force | Out-Null

  try {
    $zip      = Join-Path $tmp $asset
    $sumsFile = Join-Path $tmp 'SHA256SUMS.txt'

    Info "Downloading $asset ($label) from $downloadUrl ..."
    try {
      Invoke-WebRequest -Uri $downloadUrl -OutFile $zip -UseBasicParsing
    } catch {
      Fail "Could not download $downloadUrl ($($_.Exception.Message)). Check https://github.com/$Repo/releases or set `$env:ECHOVENA_BUILD_FROM_SOURCE='1' to build with cargo."
    }

    Info "Verifying SHA-256 checksum via SHA256SUMS.txt ..."
    try {
      Invoke-WebRequest -Uri $sumsUrl -OutFile $sumsFile -UseBasicParsing
    } catch {
      Fail "Could not download checksum manifest $sumsUrl ($($_.Exception.Message)). Aborting for safety."
    }

    $expectedHash = $null
    foreach ($line in (Get-Content -Path $sumsFile)) {
      if ($line -match '^([0-9a-fA-F]{64})\s+\*?(\S+)\s*$') {
        if ($Matches[2] -eq $asset) {
          $expectedHash = $Matches[1].ToLowerInvariant()
          break
        }
      }
    }
    if (-not $expectedHash) {
      Fail "Asset '$asset' was not listed in SHA256SUMS.txt. Aborting."
    }

    $actualHash = (Get-FileHash -Algorithm SHA256 -Path $zip).Hash.ToLowerInvariant()
    if ($actualHash -ne $expectedHash) {
      Fail "SHA-256 mismatch for $asset! Expected $expectedHash, got $actualHash. Aborting."
    }
    Ok "SHA-256 verified ($actualHash)"

    Expand-Archive -Path $zip -DestinationPath $tmp -Force
    $exe = Join-Path $tmp "$Binary.exe"
    if (-not (Test-Path $exe)) {
      $nested = Get-ChildItem -Path $tmp -Filter "$Binary.exe" -Recurse | Select-Object -First 1
      if ($nested) { $exe = $nested.FullName }
    }
    if (-not (Test-Path $exe)) {
      Fail "Downloaded archive did not contain $Binary.exe."
    }

    New-Item -ItemType Directory -Path $InstallDir -Force | Out-Null
    $destExe = Join-Path $InstallDir "$Binary.exe"
    Copy-Item -Path $exe -Destination $destExe -Force

    $verOut = & $destExe --version
    if ($LASTEXITCODE -ne 0) {
      Fail "Installed binary failed '--version' smoke check (exit code $LASTEXITCODE)."
    }
    Ok "Installed $verOut to $destExe"
  } finally {
    Remove-Item -Recurse -Force $tmp -ErrorAction SilentlyContinue
  }

  # --- add to persistent user PATH and current session -----------------------
  if (Test-Path $InstallDir) {
    $userPath = [Environment]::GetEnvironmentVariable('Path', 'User')
    $userPaths = if ($userPath) { $userPath -split ';' } else { @() }
    if ($userPaths -notcontains $InstallDir) {
      if ($env:ECHOVENA_NO_MODIFY_PATH) {
        Warn "$InstallDir is not on your PATH. Add it with:"
        Write-Host "    `$env:Path = `"$InstallDir;`$env:Path`""
      } else {
        $newUserPath = ((@($userPath, $InstallDir) | Where-Object { $_ }) -join ';')
        [Environment]::SetEnvironmentVariable('Path', $newUserPath, 'User')
        $env:Path = "$InstallDir;$env:Path"
        Ok "Added $InstallDir to User PATH and active PowerShell session."
      }
    } elseif (($env:Path -split ';') -notcontains $InstallDir) {
      $env:Path = "$InstallDir;$env:Path"
    }
  }
}

# --- check optional yt-dlp helper --------------------------------------------
if (Get-Command yt-dlp -ErrorAction SilentlyContinue) {
  Ok "yt-dlp detected in PATH."
} elseif ($env:ECHOVENA_INSTALL_YTDLP -eq '1' -and (Get-Command winget -ErrorAction SilentlyContinue)) {
  Info "Installing optional yt-dlp helper via winget..."
  winget install --id=yt-dlp.yt-dlp -e --silent --accept-source-agreements --accept-package-agreements
  if ($LASTEXITCODE -eq 0) {
    Ok "yt-dlp installed via winget."
  } else {
    Warn "Optional yt-dlp install exited $LASTEXITCODE. Install anytime with: winget install yt-dlp.yt-dlp"
  }
} else {
  Info "Tip: for YouTube fallback stream extraction, install yt-dlp: winget install yt-dlp.yt-dlp"
}

Write-Host ""
Write-Host "=================================================================" -ForegroundColor Green
Write-Host "  EchoVena installation complete! Run 'echovena' to launch." -ForegroundColor Green
Write-Host "=================================================================" -ForegroundColor Green
Write-Host ""
