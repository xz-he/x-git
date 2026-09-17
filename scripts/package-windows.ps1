$ErrorActionPreference = 'Stop'
$projectRoot = Split-Path -Parent $PSScriptRoot
$previousKey = $env:TAURI_SIGNING_PRIVATE_KEY
$previousPassword = $env:TAURI_SIGNING_PRIVATE_KEY_PASSWORD
Push-Location $projectRoot
try {
    node scripts/release-version.mjs --check
    if ($LASTEXITCODE -ne 0) { throw 'Version validation failed.' }
    if (-not $env:TAURI_SIGNING_PRIVATE_KEY) {
        $keyPath = Join-Path $projectRoot '.release-keys/updater.key'
        if (-not (Test-Path -LiteralPath $keyPath -PathType Leaf)) {
            throw 'Signing key missing. Restore .release-keys/updater.key from your backup or set TAURI_SIGNING_PRIVATE_KEY. See RELEASE.md.'
        }
        $env:TAURI_SIGNING_PRIVATE_KEY = $keyPath
    }
    if (-not $env:TAURI_SIGNING_PRIVATE_KEY_PASSWORD) { $env:TAURI_SIGNING_PRIVATE_KEY_PASSWORD = '' }
    npm run tauri -- build --bundles nsis --ci
    if ($LASTEXITCODE -ne 0) { throw 'Signed installer build failed.' }
} finally {
    $env:TAURI_SIGNING_PRIVATE_KEY = $previousKey
    $env:TAURI_SIGNING_PRIVATE_KEY_PASSWORD = $previousPassword
    Pop-Location
}
