# scripts/ci.ps1
$runtimeDir = Join-Path $PSScriptRoot "..\\runtime\\windows"
$runtimeDll = Join-Path $runtimeDir "libiomp5md.dll"
if (Test-Path $runtimeDll) {
    $env:PATH = "$runtimeDir;$env:PATH"
}

Write-Host "Running cargo format..."
cargo fmt --all -- --check
if ($LASTEXITCODE -ne 0) { exit $LASTEXITCODE }

Write-Host "Running cargo clippy..."
cargo clippy --all-targets -- -D warnings
if ($LASTEXITCODE -ne 0) { exit $LASTEXITCODE }

Write-Host "Running cargo test..."
cargo test --all
exit $LASTEXITCODE
