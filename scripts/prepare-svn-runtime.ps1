param(
    [string]$SourceDirectory
)

$ErrorActionPreference = 'Stop'

$projectRoot = Split-Path -Parent $PSScriptRoot
$targetDirectory = Join-Path $projectRoot 'src-tauri/resources/svn'
$targetBin = Join-Path $targetDirectory 'bin'
$targetLicenses = Join-Path $targetDirectory 'licenses'

if (-not $SourceDirectory) {
    $command = Get-Command svn.exe -ErrorAction SilentlyContinue
    $candidatePaths = @()
    if ($command) { $candidatePaths += $command.Source }
    if ($env:ProgramFiles) { $candidatePaths += (Join-Path $env:ProgramFiles 'SlikSvn/bin/svn.exe') }
    if (${env:ProgramFiles(x86)}) { $candidatePaths += (Join-Path ${env:ProgramFiles(x86)} 'SlikSvn/bin/svn.exe') }
    $svnPath = $candidatePaths |
        Where-Object { $_ -and (Test-Path -LiteralPath $_ -PathType Leaf) } |
        Select-Object -First 1
    if ($svnPath) {
        $SourceDirectory = Split-Path -Parent $svnPath
    } elseif (Test-Path -LiteralPath (Join-Path $projectRoot 'src-tauri/resources/svn/bin/svn.exe') -PathType Leaf) {
        Write-Host 'Bundled SVN runtime already exists.'
        return
    } else {
        throw 'svn.exe was not found. Install a redistributable Windows SVN client or pass -SourceDirectory.'
    }
}

$svnPath = Join-Path $SourceDirectory 'svn.exe'
if (-not (Test-Path -LiteralPath $svnPath -PathType Leaf)) {
    throw "svn.exe was not found in $SourceDirectory."
}

Remove-Item -LiteralPath $targetDirectory -Recurse -Force -ErrorAction SilentlyContinue
New-Item -ItemType Directory -Path $targetBin, $targetLicenses | Out-Null
Copy-Item -LiteralPath $svnPath -Destination (Join-Path $targetBin 'svn.exe')

Get-ChildItem -LiteralPath $SourceDirectory -Filter '*.dll' -File | Copy-Item -Destination $targetBin

$sourceRoot = Split-Path -Parent $SourceDirectory
$licenseFiles = Get-ChildItem -LiteralPath $sourceRoot -Recurse -File -ErrorAction SilentlyContinue |
    Where-Object { $_.Name -match '^(LICENSE|NOTICE|COPYING)(\..*)?$|\.(txt|html?)$' }
if (-not $licenseFiles) {
    throw "No redistributor license files were found below $sourceRoot."
}
$licenseFiles | Copy-Item -Destination $targetLicenses -Force

$manifest = [ordered]@{
    source = 'SlikSVN Windows x64'
    svnPath = 'bin/svn.exe'
    preparedAt = (Get-Date).ToUniversalTime().ToString('o')
}
$manifest | ConvertTo-Json | Set-Content -LiteralPath (Join-Path $targetDirectory 'manifest.json') -Encoding UTF8
Write-Host "Prepared bundled SVN runtime in $targetDirectory"
