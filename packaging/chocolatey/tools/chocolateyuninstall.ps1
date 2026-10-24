$ErrorActionPreference = 'Stop'
$packageName = 'echovena'
$toolsDir = "$(Split-Path -parent $MyInvocation.MyCommand.Definition)"

Uninstall-ChocolateyZipPackage -PackageName $packageName -ZipFileName "$toolsDir\echovena.exe"
