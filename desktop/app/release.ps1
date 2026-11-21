# Build assinado + latest.json para o updater. Uso: ./release.ps1, depois o `gh release create` impresso.
$ErrorActionPreference = "Stop"
Set-Location $PSScriptRoot
$env:TAURI_SIGNING_PRIVATE_KEY = Get-Content -Raw "$env:USERPROFILE\.tauri\live-cmd.key"
$env:TAURI_SIGNING_PRIVATE_KEY_PASSWORD = ""
# Versão fixa = a do crate `tauri` no Cargo.lock (2.12.5; bundler >= 2.10.0). Suba as duas juntas.
npx --yes @tauri-apps/cli@2.12.5 build
if ($LASTEXITCODE) { exit $LASTEXITCODE }
$ver = (Get-Content -Raw tauri.conf.json | ConvertFrom-Json).version
if (!$ver) { $ver = (Select-String -Path Cargo.toml -Pattern '^version = "(.+)"').Matches[0].Groups[1].Value }
$dir = "target\release\bundle\nsis"
$exe = Get-ChildItem "$dir\*_${ver}_*-setup.exe" | Select-Object -First 1
$name = $exe.Name -replace ' ', '.' # GitHub troca espaços por pontos
@{ version = $ver; pub_date = (Get-Date).ToUniversalTime().ToString("o")
   platforms = @{ "windows-x86_64" = @{ signature = (Get-Content -Raw "$($exe.FullName).sig").Trim()
     url = "https://github.com/yagoriccomi/live-cmd/releases/download/v$ver/$name" } } } |
  ConvertTo-Json -Depth 5 | ForEach-Object { [IO.File]::WriteAllText("$PWD\$dir\latest.json", $_) } # sem BOM no PS 5.1
"gh release create v$ver `"$($exe.FullName)`" `"$dir\latest.json`" --title v$ver --notes `"live-cmd $ver`""
