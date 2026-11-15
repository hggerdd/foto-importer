param()
$ErrorActionPreference = 'Stop'
$projectRoot = Split-Path -Parent $PSScriptRoot
Push-Location -LiteralPath $projectRoot
try {
    & cargo build --release --locked
    if ($LASTEXITCODE -ne 0) { throw 'Release-Build fehlgeschlagen.' }
    $stamp = Get-Date -Format 'yyyyMMdd-HHmmss'
    $bundle = Join-Path $projectRoot "dist/foto-importer-$stamp"
    New-Item -ItemType Directory -Path $bundle | Out-Null
    $targetDir = if ($env:CARGO_TARGET_DIR) { $env:CARGO_TARGET_DIR } else { Join-Path $projectRoot 'target' }
    $exe = Join-Path $targetDir 'release/foto-importer.exe'
    if (-not (Test-Path -LiteralPath $exe)) { throw "EXE nicht gefunden: $exe" }
    Copy-Item -LiteralPath $exe -Destination $bundle
    Copy-Item -LiteralPath (Join-Path $projectRoot 'README.md') -Destination $bundle
    Copy-Item -LiteralPath (Join-Path $projectRoot 'docs') -Destination $bundle -Recurse
    Copy-Item -LiteralPath (Join-Path $projectRoot 'ADR') -Destination $bundle -Recurse
    $modelFolder = Join-Path $bundle 'models'
    New-Item -ItemType Directory -Path $modelFolder | Out-Null
    Get-ChildItem -LiteralPath (Join-Path $projectRoot 'models') -File | Where-Object { $_.Extension -in '.json', '.md' } | Copy-Item -Destination $modelFolder
    Compress-Archive -LiteralPath $bundle -DestinationPath "$bundle.zip"
    Write-Output "Release: $bundle.zip"
} finally { Pop-Location }
