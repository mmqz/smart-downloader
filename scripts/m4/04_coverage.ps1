# M4a coverage gate: nightly source coverage (-C instrument-coverage) + llvm-cov.
# Prereq: rustup toolchain install nightly --profile minimal --component llvm-tools-preview
# Usage: .\scripts\m4\04_coverage.ps1
$ErrorActionPreference = 'Stop'
$root = Split-Path -Parent $PSScriptRoot | Split-Path -Parent
$wt = Join-Path $root 'target'

Push-Location (Join-Path $root 'crates\httpdl')
try {
    # clean stale profraw: accumulation blows Windows cmdline length
    Remove-Item "$wt\m4cov-*.profraw" -ErrorAction SilentlyContinue
    cargo +nightly clean -p smart-dl-httpdl
    $env:RUSTFLAGS = '-C instrument-coverage'
    $env:CARGO_INCREMENTAL = '0'
    $env:LLVM_PROFILE_FILE = "$wt\m4cov-%p-%m.profraw"
    cargo +nightly test -p smart-dl-httpdl
    if ($LASTEXITCODE -ne 0) { throw "cargo test failed" }

    $rustup_home = if ($env:RUSTUP_HOME) { $env:RUSTUP_HOME } else { Join-Path $env:USERPROFILE '.rustup' }
    $llvm = Join-Path $rustup_home ("toolchains\nightly-x86_64-pc-windows-msvc\lib\rustlib\x86_64-pc-windows-msvc\bin")

    & (Join-Path $llvm 'llvm-profdata.exe') merge -sparse (Get-ChildItem "$wt\m4cov-*.profraw").FullName -o "$wt\m4cov.profdata"
    if ($LASTEXITCODE -ne 0) { throw "llvm-profdata merge failed" }

    $exes = @(Get-ChildItem "$wt\debug\deps\*.exe", "$wt\debug\build\smart-dl-httpdl\*\out\*.exe" -ErrorAction SilentlyContinue)
    # no quotes on objects: cmd /c needs balanced quotes; quoted objects break
    # when exe count parity changes ("The syntax of the command is incorrect.")
    # NOTE: keep this file ASCII-only -- PS 5.1 reads UTF-8-no-BOM as ANSI,
    # CJK comments corrupt the cmdline string.
    $objs = @($exes | ForEach-Object { '--object=' + $_.FullName })
    # exclude core instrumented code (httpdl depends on core; tests never reach
    # core logic -> 0% rows dilute TOTAL). Gate = httpdl source line coverage.
    $cmdline = (Join-Path $llvm 'llvm-cov.exe') + ' report -instr-profile=' + "$wt\m4cov.profdata" + ' ' + ($objs -join ' ') + ' --ignore-filename-regex=\.cargo --ignore-filename-regex=rustc --ignore-filename-regex=registry --ignore-filename-regex=core\\src'
    cmd /c "$cmdline 2>&1" | Tee-Object -FilePath "$wt\m4cov-report.txt"

    $line = Select-String -Path "$wt\m4cov-report.txt" -Pattern 'TOTAL' | Select-Object -First 1
    if ($line) { Write-Output "[m4_coverage] $($line.Line.Trim())" }
    else { throw "llvm-cov report has no TOTAL line (see $wt\m4cov-report.txt)" }
} finally {
    Pop-Location
    Remove-Item Env:RUSTFLAGS -ErrorAction SilentlyContinue
    Remove-Item Env:LLVM_PROFILE_FILE -ErrorAction SilentlyContinue
}