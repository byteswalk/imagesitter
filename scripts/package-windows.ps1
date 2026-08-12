$ErrorActionPreference = "Stop"

$projectRoot = Split-Path -Parent $PSScriptRoot
$stagedDll = Join-Path $projectRoot "src-tauri\resources\winsitter.dll"
if (-not (Test-Path -LiteralPath $stagedDll -PathType Leaf) -or (Get-Item -LiteralPath $stagedDll).Length -eq 0) {
    if (-not $env:WINSITTER_DLL) {
        throw "A licensed x64 winsitter.dll is required. Set WINSITTER_DLL to its full path."
    }
}

$env:IMAGESITTER_REQUIRE_WINSITTER = "1"
Push-Location $projectRoot
try {
    pnpm tauri build
    if ($LASTEXITCODE -ne 0) { throw "Tauri build failed with exit code $LASTEXITCODE" }
    cargo build --manifest-path src-tauri/Cargo.toml --release --features cli --bin imagesitter-cli
    if ($LASTEXITCODE -ne 0) { throw "CLI build failed with exit code $LASTEXITCODE" }
}
finally {
    Pop-Location
}
