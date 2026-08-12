param([switch]$SkipBuild)

$ErrorActionPreference = "Stop"

$projectRoot = Split-Path -Parent $PSScriptRoot
$version = (Get-Content -LiteralPath (Join-Path $projectRoot "package.json") -Raw | ConvertFrom-Json).version
if ($version -notmatch '^\d+\.\d+\.\d+$') {
    throw "package.json contains an invalid release version: $version"
}
$stagedDll = Join-Path $projectRoot "src-tauri\resources\winsitter.dll"
if (-not (Test-Path -LiteralPath $stagedDll -PathType Leaf) -or (Get-Item -LiteralPath $stagedDll).Length -eq 0) {
    if (-not $env:WINSITTER_DLL) {
        throw "A licensed x64 winsitter.dll is required. Set WINSITTER_DLL to its full path."
    }
}

$env:IMAGESITTER_REQUIRE_WINSITTER = "1"
Push-Location $projectRoot
try {
    if (-not $SkipBuild) {
        pnpm tauri build
        if ($LASTEXITCODE -ne 0) { throw "Tauri build failed with exit code $LASTEXITCODE" }
        cargo build --manifest-path src-tauri/Cargo.toml --release --features cli --bin imagesitter-cli
        if ($LASTEXITCODE -ne 0) { throw "CLI build failed with exit code $LASTEXITCODE" }
    }

    $releaseDir = Join-Path $projectRoot "release"
    $portableDir = Join-Path $releaseDir "ImageSitter-$version-portable"
    $portableZip = "$portableDir.zip"
    $installerName = "ImageSitter_${version}_x64-setup.exe"
    $installerSource = Join-Path $projectRoot "src-tauri\target\release\bundle\nsis\$installerName"
    $installerTarget = Join-Path $releaseDir $installerName
    $guiSource = Join-Path $projectRoot "src-tauri\target\release\imagesitter.exe"
    $cliSource = Join-Path $projectRoot "src-tauri\target\release\imagesitter-cli.exe"
    $notesSource = Join-Path $projectRoot "docs\release-$version.md"

    foreach ($required in @($installerSource, $guiSource, $cliSource, $stagedDll, $notesSource)) {
        if (-not (Test-Path -LiteralPath $required -PathType Leaf) -or (Get-Item -LiteralPath $required).Length -eq 0) {
            throw "Release artifact is missing or empty: $required"
        }
    }

    New-Item -ItemType Directory -Path $releaseDir -Force | Out-Null
    if (Test-Path -LiteralPath $portableDir) {
        $resolvedPortable = [System.IO.Path]::GetFullPath($portableDir)
        $resolvedRelease = [System.IO.Path]::GetFullPath($releaseDir)
        if (-not $resolvedPortable.StartsWith($resolvedRelease + '\', [System.StringComparison]::OrdinalIgnoreCase)) {
            throw "Portable output escaped the release directory"
        }
        Remove-Item -LiteralPath $portableDir -Recurse
    }
    New-Item -ItemType Directory -Path $portableDir | Out-Null
    Copy-Item -LiteralPath $guiSource -Destination (Join-Path $portableDir "ImageSitter.exe")
    Copy-Item -LiteralPath $cliSource -Destination (Join-Path $portableDir "imagesitter-cli.exe")
    Copy-Item -LiteralPath $stagedDll -Destination (Join-Path $portableDir "winsitter.dll")
    Copy-Item -LiteralPath $notesSource -Destination (Join-Path $portableDir "RELEASE_NOTES.md")
    Copy-Item -LiteralPath $installerSource -Destination $installerTarget -Force
    if (Test-Path -LiteralPath $portableZip) { Remove-Item -LiteralPath $portableZip }
    Compress-Archive -LiteralPath $portableDir -DestinationPath $portableZip -CompressionLevel Optimal

    $hashTargets = @(
        $installerTarget,
        $portableZip,
        (Join-Path $portableDir "ImageSitter.exe"),
        (Join-Path $portableDir "imagesitter-cli.exe"),
        (Join-Path $portableDir "winsitter.dll")
    )
    $hashLines = foreach ($artifact in $hashTargets) {
        $hash = (Get-FileHash -LiteralPath $artifact -Algorithm SHA256).Hash
        $relative = $artifact.Substring($releaseDir.TrimEnd('\').Length + 1).Replace('\', '/')
        "$hash  $relative"
    }
    [System.IO.File]::WriteAllLines((Join-Path $releaseDir "SHA256SUMS.txt"), $hashLines)
    Write-Host "Release $version assembled at $releaseDir"
}
finally {
    Pop-Location
}
