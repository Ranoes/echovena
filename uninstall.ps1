<#
.SYNOPSIS
  EchoVena uninstaller for Windows.

  irm https://raw.githubusercontent.com/Ranoes/echovena/main/uninstall.ps1 | iex

  Parameters / Environment overrides:
    -PurgeData  / $env:ECHOVENA_PURGE_DATA   set to 1 to purge config, database, session & cache (~/.config/echovena, ~/.local/share/echovena, etc.)
    -PurgeMusic / $env:ECHOVENA_PURGE_MUSIC  set to 1 to delete downloaded music archive (~/Music/EchoVena)
    -InstallDir / $env:ECHOVENA_INSTALL_DIR  custom install directory to clean; default: $env:LOCALAPPDATA\Programs\EchoVena\bin
    -Force      / $env:ECHOVENA_FORCE        skip confirmation prompt
#>
[CmdletBinding()]
param(
  [switch]$PurgeData,
  [switch]$PurgeMusic,
  [string]$InstallDir = $(if ($env:ECHOVENA_INSTALL_DIR) { $env:ECHOVENA_INSTALL_DIR } else { Join-Path $env:LOCALAPPDATA 'Programs\EchoVena\bin' }),
  [switch]$Force
)

$ErrorActionPreference = 'Stop'
[Net.ServicePointManager]::SecurityProtocol = [Net.ServicePointManager]::SecurityProtocol -bor [Net.SecurityProtocolType]::Tls12

$Binary = 'echovena'
if ($env:ECHOVENA_PURGE_DATA -eq '1') { $PurgeData = $true }
if ($env:ECHOVENA_PURGE_MUSIC -eq '1') { $PurgeMusic = $true }
if ($env:ECHOVENA_FORCE -eq '1') { $Force = $true }

function Info($m) { Write-Host "[*] $m" -ForegroundColor DarkGray }
function Ok($m)   { Write-Host "[OK] $m" -ForegroundColor Green }
function Warn($m) { Write-Host "[!] $m" -ForegroundColor Yellow }
function Fail($m) { Write-Host "[X] $m" -ForegroundColor Red; exit 1 }

Write-Host ""
Write-Host "  ===============================================================" -ForegroundColor Cyan
Write-Host "   EchoVena Uninstaller - Windows" -ForegroundColor Cyan
Write-Host "   https://github.com/Ranoes/echovena" -ForegroundColor DarkGray
Write-Host "  ===============================================================" -ForegroundColor Cyan
Write-Host ""

# 1. Terminate any active running instances to avoid file-lock errors
$running = Get-Process -Name $Binary -ErrorAction SilentlyContinue
if ($running) {
  Info "Stopping running $Binary process..."
  $running | Stop-Process -Force -ErrorAction SilentlyContinue
  Start-Sleep -Milliseconds 300
}

# 2. Remove standard one-line install directory and executable
$exePath = Join-Path $InstallDir "$Binary.exe"
$parentProgramDir = Split-Path $InstallDir -Parent
$removedAny = $false

if (Test-Path $exePath) {
  Info "Removing executable: $exePath"
  Remove-Item -Path $exePath -Force -ErrorAction SilentlyContinue
  $removedAny = $true
}

if (Test-Path $InstallDir) {
  Info "Removing directory: $InstallDir"
  Remove-Item -Path $InstallDir -Recurse -Force -ErrorAction SilentlyContinue
  $removedAny = $true
}

if ($parentProgramDir -and (Test-Path $parentProgramDir)) {
  $remaining = Get-ChildItem -Path $parentProgramDir -Force -ErrorAction SilentlyContinue
  if (-not $remaining) {
    Remove-Item -Path $parentProgramDir -Recurse -Force -ErrorAction SilentlyContinue
  }
}

# 3. Clean up User PATH environment variable and active session
$userPath = [Environment]::GetEnvironmentVariable('Path', 'User')
if ($userPath) {
  $paths = $userPath -split ';' | Where-Object { $_.Trim() -ne '' }
  $cleanPaths = $paths | Where-Object { $_.TrimEnd('\') -ne $InstallDir.TrimEnd('\') }
  if ($paths.Count -ne $cleanPaths.Count) {
    $newUserPath = ($cleanPaths -join ';')
    [Environment]::SetEnvironmentVariable('Path', $newUserPath, 'User')
    Ok "Removed $InstallDir from User PATH."
  }
}

$sessionPaths = ($env:Path -split ';' | Where-Object { $_.Trim() -ne '' -and $_.TrimEnd('\') -ne $InstallDir.TrimEnd('\') })
$env:Path = ($sessionPaths -join ';')

# 4. Check for Cargo-installed binary (~/.cargo/bin/echovena.exe)
$cargoExe = Join-Path $env:USERPROFILE ".cargo\bin\$Binary.exe"
if (Test-Path $cargoExe) {
  Info "Found Cargo-installed binary at $cargoExe"
  if (Get-Command cargo -ErrorAction SilentlyContinue) {
    Info "Running 'cargo uninstall $Binary'..."
    cargo uninstall $Binary 2>$null | Out-Null
  } else {
    Remove-Item -Path $cargoExe -Force -ErrorAction SilentlyContinue
  }
  Ok "Removed Cargo binary."
  $removedAny = $true
}

# 5. Detect Package Manager installations (Winget / Chocolatey)
if (Get-Command winget -ErrorAction SilentlyContinue) {
  $wingetCheck = winget list --id Ranoes.EchoVena --exact 2>$null
  if ($wingetCheck -match 'Ranoes\.EchoVena') {
    Warn "Detected Winget package installation ('Ranoes.EchoVena')."
    Write-Host "    To remove via Winget, run: winget uninstall Ranoes.EchoVena" -ForegroundColor Yellow
  }
}

if (Get-Command choco -ErrorAction SilentlyContinue) {
  $chocoCheck = choco list --local-only --exact $Binary 2>$null
  if ($chocoCheck -match "$Binary ") {
    Warn "Detected Chocolatey package installation ('$Binary')."
    Write-Host "    To remove via Chocolatey, run: choco uninstall $Binary -y" -ForegroundColor Yellow
  }
}

# 6. Purge application data, databases and caches (if requested)
if ($PurgeData) {
  Info "Purging EchoVena configurations, state, database & caches..."
  $dataDirs = @(
    (Join-Path $env:USERPROFILE ".config\$Binary"),
    (Join-Path $env:USERPROFILE ".local\share\$Binary"),
    (Join-Path $env:USERPROFILE ".local\state\$Binary"),
    (Join-Path $env:USERPROFILE ".cache\$Binary")
  )
  foreach ($d in $dataDirs) {
    if (Test-Path $d) {
      Remove-Item -Path $d -Recurse -Force -ErrorAction SilentlyContinue
      Ok "Deleted: $d"
    }
  }
} else {
  Info "Preserved user data and configs (~/.config/echovena, ~/.local/share/echovena)."
  Write-Host "    (To completely remove user data, run with: .\uninstall.ps1 -PurgeData)" -ForegroundColor DarkGray
}

# 7. Purge downloaded music archive (if requested)
$musicArchive = Join-Path $env:USERPROFILE "Music\EchoVena"
if ($PurgeMusic) {
  if (Test-Path $musicArchive) {
    Info "Purging offline music archive ($musicArchive)..."
    Remove-Item -Path $musicArchive -Recurse -Force -ErrorAction SilentlyContinue
    Ok "Deleted: $musicArchive"
  }
} else {
  if (Test-Path $musicArchive) {
    Info "Preserved offline music archive ($musicArchive)."
  }
}

Write-Host ""
Write-Host "=================================================================" -ForegroundColor Green
Write-Host "  EchoVena uninstallation complete." -ForegroundColor Green
Write-Host "=================================================================" -ForegroundColor Green
Write-Host ""
