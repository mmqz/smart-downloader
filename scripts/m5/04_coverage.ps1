# M5 coverage gate for crates/provider: nightly -C instrument-coverage + llvm-tools.
# Prereq: rustup toolchain install nightly --profile minimal --component llvm-tools-preview
# ASCII only (PS 5.1 reads UTF-8 without BOM as ANSI).
$ErrorActionPreference = 'Stop'
$root = Split-Path -Parent $PSScriptRoot | Split-Path -Parent
$wt = Join-Path $root 'target'

Push-Location (Join-Path $root 'crates\provider')
try {
    # clean stale profraw: accumulation blows Windows cmdline length
    Remove-Item "$wt\m5cov-*.profraw" -ErrorAction SilentlyContinue
    cargo +nightly clean -p smart-dl-provider
    $env:RUSTFLAGS = '-C instrument-coverage'
    $env:CARGO_INCREMENTAL = '0'
    $env:LLVM_PROFILE_FILE = "$wt\m5cov-%p-%m.profraw"
    cargo +nightly test -p smart-dl-provider
    if ($LASTEXITCODE -ne 0) { throw "cargo test failed" }

    $rustup_home = if ($env:RUSTUP_HOME) { $env:RUSTUP_HOME } else { Join-Path $env:USERPROFILE '.rustup' }
    $llvm = Join-Path $rustup_home ("toolchains\nightly-x86_64-pc-windows-msvc\lib\rustlib\x86_64-pc-windows-msvc\bin")

    & (Join-Path $llvm 'llvm-profdata.exe') merge -sparse (Get-ChildItem "$wt\m5cov-*.profraw").FullName -o "$wt\m5cov.profdata"
    if ($LASTEXITCODE -ne 0) { throw "llvm-profdata merge failed" }

    # NOTE: cargo 1.100 nightly puts integration test exes under
    # build/<pkg>/<hash>/out/ (not deps/). deps/ accumulates all crates' exes;
    # a full glob blows the cmd.exe 32767-char command line.
    $pats = @('smart_dl_provider-*', 'mock_lifecycle-*', 'quota_backoff-*', 'link_expiry-*', 'fallback_integration-*')
    $e1 = @(Get-ChildItem "$wt\debug\deps\*.exe" -ErrorAction SilentlyContinue | Where-Object {
        $n = $_.Name
        $pats | Where-Object { $n -like $_ }
    })
    $e2 = @(Get-ChildItem "$wt\debug\build\smart-dl-provider\*\out\*.exe" -ErrorAction SilentlyContinue | Where-Object {
        $n = $_.Name
        $pats | Where-Object { $n -like $_ }
    })
    $exes = @($e1 + $e2)
    # no quotes on objects: cmd /c needs balanced quotes; quoted objects break
    # when exe count parity changes ("The syntax of the command is incorrect.")
    $objs = @($exes | ForEach-Object { '--object=' + $_.FullName })
    # exclude core + httpdl instrumented code (provider depends on them; tests
    # never reach their logic -> 0% rows dilute TOTAL). Gate = provider source
    # line coverage.
    $cmdline = (Join-Path $llvm 'llvm-cov.exe') + ' report -instr-profile=' + "$wt\m5cov.profdata" + ' ' + ($objs -join ' ') + ' --ignore-filename-regex=\.cargo --ignore-filename-regex=rustc --ignore-filename-regex=registry --ignore-filename-regex=core\\src --ignore-filename-regex=httpdl\\src'
    cmd /c "$cmdline 2>&1" | Tee-Object -FilePath "$wt\m5cov-report.txt"

    $line = Select-String -Path "$wt\m5cov-report.txt" -Pattern 'TOTAL' | Select-Object -First 1
    if ($line) { Write-Output "[m5_coverage] $($line.Line.Trim())" }
    else { throw "llvm-cov report has no TOTAL line (see $wt\m5cov-report.txt)" }
} finally {
    Pop-Location
    Remove-Item Env:RUSTFLAGS -ErrorAction SilentlyContinue
    Remove-Item Env:LLVM_PROFILE_FILE -ErrorAction SilentlyContinue
}