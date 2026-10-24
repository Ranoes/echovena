$ErrorActionPreference = 'Stop'

$packageName = 'echovena'
$version = "1.0.0"
$url = "https://github.com/Ranoes/echovena/releases/download/v$version/echovena-windows-x86_64.zip"
$checksum = ""
$checksumType = 'sha256'

$toolsDir = "$(Split-Path -parent $MyInvocation.MyCommand.Definition)"

$packageArgs = @{
  packageName   = $packageName
  unzipLocation = $toolsDir
  url           = $url
  checksum      = $checksum
  checksumType  = $checksumType
}

Install-ChocolateyZipPackage @packageArgs
