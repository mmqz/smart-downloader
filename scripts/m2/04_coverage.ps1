# M2 coverage gate: nightly source coverage (-C instrument-coverage) + llvm-cov.
# Prereq: rustup toolchain install nightly --profile minimal --component llvm-tools-preview
# Usage: .\scripts\m2\04_coverage.ps1
$ErrorActionPreference = 'Stop'
$root = Split-Path -Parent $PSScriptRoot | Split-Path -Parent
$wt = Join-Path $root 'target'

Push-Location (Join-Path $root 'crates\core')
try {
    cargo +nightly clean -p smart-dl-core
    $env:RUSTFLAGS = '-C instrument-coverage'
    $env:CARGO_INCREMENTAL = '0'
    $env:LLVM_PROFILE_FILE = "$wt\m2cov-%p-%m.profraw"
    cargo +nightly test -p smart-dl-core
    if ($LASTEXITCODE -ne 0) { throw "cargo test failed" }

    $rustup_home = if ($env:RUSTUP_HOME) { $env:RUSTUP_HOME } else { Join-Path $env:USERPROFILE '.rustup' }
    $llvm = Join-Path $rustup_home ("toolchains\nightly-x86_64-pc-windows-msvc\lib\rustlib\x86_64-pc-windows-msvc\bin")

    & (Join-Path $llvm 'llvm-profdata.exe') merge -sparse (Get-ChildItem "$wt\m2cov-*.profraw").FullName -o "$wt\m2cov.profdata"
    if ($LASTEXITCODE -ne 0) { throw "llvm-profdata merge failed" }

    $exes = @(Get-ChildItem "$wt\debug\deps\*.exe", "$wt\debug\build\smart-dl-core\*\out\*.exe" -ErrorAction SilentlyContinue)
    $objs = @($exes | ForEach-Object { '"--object=' + $_.FullName + '"' })
    $cmdline = '"' + (Join-Path $llvm 'llvm-cov.exe') + '" report "-instr-profile=' + "$wt\m2cov.profdata" + '" ' + ($objs -join ' ') + ' --ignore-filename-regex=\.cargo --ignore-filename-regex=rustc'
    cmd /c "$cmdline 2>&1" | Tee-Object -FilePath "$wt\m2cov-report.txt"

    $line = Select-String -Path "$wt\m2cov-report.txt" -Pattern 'TOTAL' | Select-Object -First 1
    if ($line) { Write-Output "[m2_coverage] $($line.Line.Trim())" }
    else { throw "llvm-cov report has no TOTAL line (see $wt\m2cov-report.txt)" }
} finally {
    Pop-Location
    Remove-Item Env:RUSTFLAGS -ErrorAction SilentlyContinue
    Remove-Item Env:LLVM_PROFILE_FILE -ErrorAction SilentlyContinue
}