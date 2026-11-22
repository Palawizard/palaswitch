# Builds the release exe and packs it into the Velopack installer and update
# packages, in target\pack\releases.
#   pwsh scripts/pack.ps1
# Needs vpk (dotnet tool install -g vpk --version 1.2.161). Used as is by the
# release workflow.
$ErrorActionPreference = 'Stop'
Set-Location (Split-Path -Parent $PSScriptRoot)

$metadata = cargo metadata --format-version 1 --no-deps --locked | ConvertFrom-Json
$version = ($metadata.packages | Where-Object { $_.name -eq 'palaswitch' }).version
Write-Host "==> palaswitch $version"

& cargo build -p palaswitch --release --locked
if ($LASTEXITCODE -ne 0) { exit $LASTEXITCODE }

# A fresh Windows has no Visual C++ runtime: the exe must not need one.
$exe = [System.Text.Encoding]::ASCII.GetString([System.IO.File]::ReadAllBytes("$PWD\target\release\palaswitch.exe"))
if ($exe -match '(?i)vcruntime\d+\.dll|api-ms-win-crt-') {
    Write-Error "palaswitch.exe needs the Visual C++ runtime ($($Matches[0])): check .cargo\config.toml"
    exit 1
}

$app = 'target\pack\app'
$releases = 'target\pack\releases'
foreach ($dir in @($app, $releases)) {
    if (Test-Path $dir) { Remove-Item -Recurse -Force $dir }
    New-Item -ItemType Directory -Force $dir | Out-Null
}
Copy-Item 'target\release\palaswitch.exe' $app

# A tray utility gets a Start menu entry only, no desktop shortcut. Each
# package is full: the exe is small, and no earlier release is needed. The
# installer is the one way to get the app: no portable archive.
& vpk pack `
    --packId palaswitch `
    --packVersion $version `
    --packDir $app `
    --mainExe palaswitch.exe `
    --runtime win-x64 `
    --packTitle palaswitch `
    --packAuthors Palawizard `
    --shortcuts StartMenuRoot `
    --delta None `
    --noPortable `
    --outputDir $releases
if ($LASTEXITCODE -ne 0) { exit $LASTEXITCODE }

Get-ChildItem $releases | Format-Table Name, Length
