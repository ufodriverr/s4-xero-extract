param([switch]$Offline)

$ErrorActionPreference = 'Stop'
$previousFlags = $env:CARGO_ENCODED_RUSTFLAGS
Push-Location $PSScriptRoot
try {
    # Keep the local user profile out of compiled source-location strings.
    $env:CARGO_ENCODED_RUSTFLAGS = "--remap-path-prefix=$env:USERPROFILE=/build"
    $cargoArgs = @('build', '--locked', '--release')
    if ($Offline) { $cargoArgs += '--offline' }
    & cargo @cargoArgs
    if ($LASTEXITCODE -ne 0) { throw "cargo build failed with exit code $LASTEXITCODE" }

    $metadataArgs = @('metadata', '--format-version', '1', '--no-deps', '--locked')
    if ($Offline) { $metadataArgs += '--offline' }
    $metadata = & cargo @metadataArgs | ConvertFrom-Json
    if ($LASTEXITCODE -ne 0) { throw "cargo metadata failed with exit code $LASTEXITCODE" }
    $version = ($metadata.packages | Where-Object { $_.name -eq 's4-xero-extract' }).version

    New-Item -ItemType Directory -Force -Path 'dist' | Out-Null
    $zip = Join-Path 'dist' "s4-xero-extract-$version-windows-x64.zip"
    $contents = @(
        'README.md',
        'LEGAL.md',
        'LICENSE',
        'THIRD-PARTY-NOTICES.md',
        'licenses'
    )
    # Windows' own bsdtar writes a ZIP with forward-slash entry names (Compress-Archive on
    # PowerShell 5.1 writes backslashes, which some unzip tools mishandle); the exe at the root.
    & "$env:SystemRoot\System32\tar.exe" -a -c -f $zip -C 'target\release' 's4-xero-extract.exe' -C $PSScriptRoot @contents
    if ($LASTEXITCODE -ne 0) { throw "tar failed with exit code $LASTEXITCODE" }
    Write-Host "Release package: $zip"
}
finally {
    $env:CARGO_ENCODED_RUSTFLAGS = $previousFlags
    Pop-Location
}
