<#
.SYNOPSIS
  EchoVena installer for Windows.

  irm https://raw.githubusercontent.com/Ranoes/echovena/main/install.ps1 | iex

  Environment overrides:
    ECHOVENA_VERSION        install a specific tag (e.g. v1.0.0); default: latest
    ECHOVENA_INSTALL_DIR    where to put the binary; default: $env:LOCALAPPDATA\Programs\EchoVena\bin
    ECHOVENA_NO_MODIFY_PATH set to any value to skip updating your user PATH
#>
$ErrorActionPreference = 'Stop'

$Repo       = 'Ranoes/echovena'
$Binary     = 'echovena'
$InstallDir = if ($env:ECHOVENA_INSTALL_DIR) { $env:ECHOVENA_INSTALL_DIR } else { Join-Path $env:LOCALAPPDATA 'Programs\EchoVena\bin' }

function Info($m) { Write-Host "· $m" -ForegroundColor DarkGray }
function Ok($m)   { Write-Host "√ $m" -ForegroundColor Green }
function Warn($m) { Write-Host "! $m" -ForegroundColor Yellow }
function Fail($m) { Write-Host "x $m" -ForegroundColor Red; exit 1 }

Write-Host ""
Write-Host "  ███████╗ ██████╗██╗  ██╗ ██████╗ ██╗   ██╗███████╗███╗   ██╗ █████╗ " -ForegroundColor Cyan
Write-Host "  ██╔════╝██╔════╝██║  ██║██╔═══██╗██║   ██║██╔════╝████╗  ██║██╔══██╗" -ForegroundColor Cyan
Write-Host "  █████╗  ██║     ███████║██║   ██║██║   ██║█████╗  ██╔██╗ ██║███████║" -ForegroundColor Cyan
Write-Host "  ██╔══╝  ██║     ██╔══██║██║   ██║╚██╗ ██╔╝██╔══╝  ██║╚██╗██║██╔══██║" -ForegroundColor Cyan
Write-Host "  ███████╗╚██████╗██║  ██║╚██████╔╝ ╚████╔╝ ███████╗██║ ╚████║██║  ██║" -ForegroundColor Cyan
Write-Host "  ╚══════╝ ╚═════╝╚═╝  ╚═╝ ╚═════╝   ╚═══╝  ╚══════╝╚═╝  ╚═══╝╚═╝  ╚═╝" -ForegroundColor Cyan
Write-Host "      Lossless Hi-Res FLAC Streamer & Live Karaoke TUI (Ranoes/echovena)" -ForegroundColor DarkGray
Write-Host ""

# --- detect arch -----------------------------------------------------------
$arch = $env:PROCESSOR_ARCHITECTURE
if ($arch -ne 'AMD64') {
  if ($arch -eq 'ARM64') {
    Warn 'No native ARM64 Windows build yet; installing x86_64 build under Windows emulation.'
  } else {
    Warn "Non-standard architecture '$arch'. Attempting source build..."
  }
}
$asset = "$Binary-windows-x86_64.zip"

# --- check for releases API or source fallback ----------------------------
$downloadUrl = $null
$releaseTag  = $null

try {
  $apiUrl = if ($env:ECHOVENA_VERSION) {
    $tag = 'v' + ($env:ECHOVENA_VERSION -replace '^v', '')
    "https://api.github.com/repos/$Repo/releases/tags/$tag"
  } else {
    "https://api.github.com/repos/$Repo/releases/latest"
  }

  Info "Checking release manifest from $apiUrl..."
  $headers = @{ "User-Agent" = "EchoVena-Installer" }
  $response = Invoke-RestMethod -Uri $apiUrl -Headers $headers -Method Get -TimeoutSec 10 -ErrorAction Stop

  if ($response -and $response.assets) {
    $matchingAsset = $response.assets | Where-Object { $_.name -eq $asset } | Select-Object -First 1
    if ($matchingAsset) {
      $downloadUrl = $matchingAsset.browser_download_url
      $releaseTag  = $response.tag_name
    }
  }
} catch {
  Info "Release asset not found via API ($($_.Exception.Message)). Checking fallback..."
}

# --- download and install binary OR build from source ---------------------
if ($downloadUrl) {
  Info "Installing EchoVena ($releaseTag) for windows/x86_64 from GitHub Releases..."
  $tmp = Join-Path ([System.IO.Path]::GetTempPath()) ("echovena-" + [System.Guid]::NewGuid().ToString('N'))
  New-Item -ItemType Directory -Path $tmp -Force | Out-Null
  try {
    $zip = Join-Path $tmp $asset
    Invoke-WebRequest -Uri $downloadUrl -OutFile $zip -UseBasicParsing
    Expand-Archive -Path $zip -DestinationPath $tmp -Force

    $exe = Join-Path $tmp "$Binary.exe"
    if (-not (Test-Path $exe)) {
      $nested = Get-ChildItem -Path $tmp -Filter "$Binary.exe" -Recurse | Select-Object -First 1
      if ($nested) { $exe = $nested.FullName }
    }

    if (-not (Test-Path $exe)) { Fail "Downloaded archive did not contain $Binary.exe" }

    New-Item -ItemType Directory -Path $InstallDir -Force | Out-Null
    Copy-Item -Path $exe -Destination (Join-Path $InstallDir "$Binary.exe") -Force
    Ok "Installed $Binary.exe to $InstallDir"
  } finally {
    Remove-Item -Recurse -Force $tmp -ErrorAction SilentlyContinue
  }
} else {
  # --- Source Build Fallback ----------------------------------------------
  Warn "Prebuilt binary release asset not found. Falling back to source build via cargo..."
  if (Get-Command cargo -ErrorAction SilentlyContinue) {
    Info "Executing: cargo install --git https://github.com/$Repo.git --locked"
    cargo install --git "https://github.com/$Repo.git" --locked
    if ($LASTEXITCODE -eq 0) {
      Ok "Successfully installed EchoVena from source via cargo!"
    } else {
      Fail "Cargo installation failed with exit code $LASTEXITCODE."
    }
  } else {
    Fail "Prebuilt release not found and cargo is not installed. Please install Rust from https://rustup.rs or wait for release publication."
  }
}

# --- check for yt-dlp optional dependency ----------------------------------
if (-not (Get-Command yt-dlp -ErrorAction SilentlyContinue)) {
  Info "Checking for yt-dlp (recommended for YouTube fallback audio extraction)..."
  if (Get-Command winget -ErrorAction SilentlyContinue) {
    try {
      Info "Attempting automatic winget installation of yt-dlp..."
      winget install --id=yt-dlp.yt-dlp -e --silent --accept-source-agreements --accept-package-agreements
      if ($LASTEXITCODE -eq 0) {
        Ok "yt-dlp installed successfully."
      } else {
        Warn "winget install of yt-dlp returned non-zero. You can manually install it with: winget install yt-dlp.yt-dlp"
      }
    } catch {
      Warn "Could not auto-install yt-dlp. You can install it later with: winget install yt-dlp.yt-dlp"
    }
  } else {
    Warn "yt-dlp is not installed. YouTube stream fallback will be unavailable until yt-dlp is in PATH."
  }
} else {
  Ok "yt-dlp is available in PATH."
}

# --- add to persistent user PATH and current session ----------------------
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
      Ok "Added $InstallDir to persistent User PATH and active session."
    }
  } else {
    # Ensure active session has it
    if (($env:Path -split ';') -notcontains $InstallDir) {
      $env:Path = "$InstallDir;$env:Path"
    }
  }
}

Write-Host ""
Write-Host "=================================================================" -ForegroundColor Green
Write-Host "  EchoVena installation complete! Run 'echovena' to launch." -ForegroundColor Green
Write-Host "=================================================================" -ForegroundColor Green
Write-Host ""
