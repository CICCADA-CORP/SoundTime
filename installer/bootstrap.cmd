@echo off
setlocal DisableDelayedExpansion
powershell.exe -NoLogo -NoProfile -Command ^
  "$ErrorActionPreference = 'Stop';" ^
  "$root = Join-Path ([IO.Path]::GetTempPath()) ('SoundTime-' + [guid]::NewGuid());" ^
  "$null = New-Item -ItemType Directory -Path $root;" ^
  "try {" ^
  "    $zip = Join-Path $root 'SoundTime-CLI.zip';" ^
  "    $url = 'https://github.com/CICCADA-CORP/SoundTime/releases/download/installer-v2.0.1/SoundTime-CLI.zip';" ^
  "    $expected = 'd69e011bcf2dfea3920babf5d62810eb547c130afb37c8389f4ae0225cab48db';" ^
  "    Write-Host 'SoundTime CLI 2.0.1 ...';" ^
  "    [Net.ServicePointManager]::SecurityProtocol = [Net.ServicePointManager]::SecurityProtocol -bor [Net.SecurityProtocolType]::Tls12;" ^
  "    $client = New-Object Net.WebClient;" ^
  "    try { $client.DownloadFile($url, $zip) } finally { $client.Dispose() };" ^
  "    $sha = [Security.Cryptography.SHA256]::Create();" ^
  "    $reader = [IO.File]::OpenRead($zip);" ^
  "    try { $actual = [BitConverter]::ToString($sha.ComputeHash($reader)).Replace('-','') } finally { $reader.Dispose(); $sha.Dispose() };" ^
  "    if ($actual -ne $expected) { throw 'SoundTime CLI: SHA-256 mismatch.' };" ^
  "    Add-Type -AssemblyName System.IO.Compression.FileSystem;" ^
  "    [IO.Compression.ZipFile]::ExtractToDirectory($zip, $root);" ^
  "    & (Join-Path $root 'soundtime.cmd');" ^
  "    $code = $LASTEXITCODE;" ^
  "} finally { Remove-Item -LiteralPath $root -Recurse -Force };" ^
  "exit $code"
exit /b %ERRORLEVEL%
