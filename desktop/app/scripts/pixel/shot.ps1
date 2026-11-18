# Captura uma página em 1440x900 com o Edge headless. Uso: shot.ps1 <arquivo.html> <saida.png> [hash]
param([string]$Page, [string]$Out, [string]$Hash = "", [int]$W = 1440, [int]$H = 900)
$edge = "${env:ProgramFiles(x86)}\Microsoft\Edge\Application\msedge.exe"
$uri = ([System.Uri](Resolve-Path $Page).Path).AbsoluteUri + $Hash
$prof = Join-Path $env:TEMP ("pixel-edge-" + [guid]::NewGuid())
& $edge --headless=new --disable-gpu-sandbox --hide-scrollbars --force-device-scale-factor=1 --allow-file-access-from-files `
  --user-data-dir="$prof" --virtual-time-budget=4000 --window-size="$W,$H" --screenshot="$Out" $uri 2>$null | Out-Null
Remove-Item -Recurse -Force $prof -ErrorAction SilentlyContinue
