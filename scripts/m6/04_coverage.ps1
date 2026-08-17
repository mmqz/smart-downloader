# M6 coverage gate for crates/daemon: nightly -C instrument-coverage + llvm-tools.
# Prereq: rustup toolchain install nightly --profile minimal --component llvm-tools-preview
# ASCII only (PS 5.1 reads UTF-8 without BOM as ANSI).
$ErrorActionPreference = 'Stop'
$root = Split-Path -Parent $PSScriptRoot | Split-Path -Parent
$wt = Join-Path $root 'target'

Push-Location (Join-Path $root 'crates\daemon')
try {
    # clean stale profraw: accumulation blows Windows cmdline length
    Remove-Item "$wt\m6cov-*.profraw" -ErrorAction SilentlyContinue
    cargo +nightly clean -p smart-dl-daemon
    $env:RUSTFLAGS = '-C instrument-coverage'
    $env:CARGO_INCREMENTAL = '0'
    $env:LLVM_PROFILE_FILE = "$wt\m6cov-%p-%m.profraw"
    cargo +nightly test -p smart-dl-daemon
    if ($LASTEXITCODE -ne 0) { throw "cargo test failed" }

    $rustup_home = if ($env:RUSTUP_HOME) { $env:RUSTUP_HOME } else { Join-Path $env:USERPROFILE '.rustup' }
    $llvm = Join-Path $rustup_home ("toolchains\nightly-x86_64-pc-windows-msvc\lib\rustlib\x86_64-pc-windows-msvc\bin")

    & (Join-Path $llvm 'llvm-profdata.exe') merge -sparse (Get-ChildItem "$wt\m6cov-*.profraw").FullName -o "$wt\m6cov.profdata"
    if ($LASTEXITCODE -ne 0) { throw "llvm-profdata merge failed" }

    # collect only daemon test binaries. NOTE: cargo 1.100 nightly puts integration
    # test exes under build/<pkg>/<hash>/out/ (not deps/); deps/ accumulates all
    # crates' exes, a full glob blows the cmd.exe 32767-char command line.
    $pats = @('smart_dl_daemon-*', 'events-*', 'ws_backpressure-*', 'cli-*', 'health_leech-*', 'ratio_low-*', 'http_api-*')
    $e1 = @(Get-ChildItem "$wt\debug\deps\*.exe" -ErrorAction SilentlyContinue | Where-Object {
        $n = $_.Name
        $pats | Where-Object { $n -like $_ }
    })
    $e2 = @(Get-ChildItem "$wt\debug\build\smart-dl-daemon\*\out\*.exe" -ErrorAction SilentlyContinue | Where-Object {
        $n = $_.Name
        $pats | Where-Object { $n -like $_ }
    })
    $exes = @($e1 + $e2)
    # no quotes on objects: cmd /c needs balanced quotes; quoted objects break
    # when exe count parity changes ("The syntax of the command is incorrect.")
    $objs = @($exes | ForEach-Object { '--object=' + $_.FullName })
    # exclude core/httpdl/provider instrumented code (daemon depends on them; tests
    # never reach their logic -> 0% rows dilute TOTAL). Gate = daemon source line coverage.
    $cmdline = (Join-Path $llvm 'llvm-cov.exe') + ' report -instr-profile=' + "$wt\m6cov.profdata" + ' ' + ($objs -join ' ') + ' --ignore-filename-regex=\.cargo --ignore-filename-regex=rustc --ignore-filename-regex=registry --ignore-filename-regex=core\\src --ignore-filename-regex=httpdl\\src --ignore-filename-regex=provider\\src'
    cmd /c "$cmdline 2>&1" | Tee-Object -FilePath "$wt\m6cov-report.txt"

    $line = Select-String -Path "$wt\m6cov-report.txt" -Pattern 'TOTAL' | Select-Object -First 1
    if ($line) { Write-Output "[m6_coverage] $($line.Line.Trim())" }
    else { throw "llvm-cov report has no TOTAL line (see $wt\m6cov-report.txt)" }
} finally {
    Pop-Location
    Remove-Item Env:RUSTFLAGS -ErrorAction SilentlyContinue
    Remove-Item Env:LLVM_PROFILE_FILE -ErrorAction SilentlyContinue
}