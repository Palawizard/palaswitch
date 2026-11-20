# Runs every check of the project: format, lint, tests, release build.
#   pwsh scripts/verify.ps1
# Used as is by the CI.
$ErrorActionPreference = 'Stop'
Set-Location (Split-Path -Parent $PSScriptRoot)

$steps = @(
    @('fmt', '--all', '--', '--check'),
    @('clippy', '--workspace', '--all-targets', '--locked', '--', '-D', 'warnings'),
    @('test', '--workspace', '--locked'),
    @('build', '--workspace', '--release', '--locked')
)

foreach ($step in $steps) {
    Write-Host "==> cargo $($step -join ' ')"
    & cargo @step
    if ($LASTEXITCODE -ne 0) {
        Write-Host "verify failed at: cargo $($step -join ' ')"
        exit $LASTEXITCODE
    }
}

Write-Host 'verify: all checks passed'
