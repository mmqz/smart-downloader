# M1.4 覆盖率验收脚本：nightly 源码覆盖率（-C instrument-coverage，1.100 nightlies 的新拼写）
# + llvm-cov（Rust 侧行覆盖）。前置：rustup toolchain install nightly --profile minimal
# --component llvm-tools-preview
# 用法：.\scripts\m1\04_coverage.ps1
$ErrorActionPreference = 'Stop'
$root = Split-Path -Parent $PSScriptRoot | Split-Path -Parent
$rel = Join-Path $root 'ffi\build\Release'

$env:LT_KERNEL_LIB_DIR = $rel
$env:SEED_MAIN = "$rel\seed_main.exe"
$env:PATH = "$rel;" + (Join-Path $root 'ffi\build\vcpkg_installed\x64-windows\bin') + ';' + $env:PATH

Push-Location (Join-Path $root 'crates\btcore')
try {
    $wt = Join-Path $root 'target'   # workspace root 的 target（cargo 输出实际位置）
    cargo +nightly clean -p smart-dl-btcore
    $env:RUSTFLAGS = '-C instrument-coverage'
    $env:LLVM_PROFILE_FILE = "$wt\cov-%p-%m.profraw"
    cargo +nightly test -p smart-dl-btcore
    if ($LASTEXITCODE -ne 0) { throw "cargo test failed" }

    $rustup_home = if ($env:RUSTUP_HOME) { $env:RUSTUP_HOME } else { Join-Path $env:USERPROFILE '.rustup' }
    $llvm = Join-Path $rustup_home ("toolchains\nightly-x86_64-pc-windows-msvc\lib\rustlib\x86_64-pc-windows-msvc\bin")

    # 合并 profraw → profdata
    & (Join-Path $llvm 'llvm-profdata.exe') merge -sparse (Get-ChildItem "$wt\cov-*.profraw").FullName -o "$wt\cov.profdata"
    if ($LASTEXITCODE -ne 0) { throw "llvm-profdata merge failed" }

    # 报告：所有测试二进制为 --object（lib 插桩代码内嵌于各 exe）。
    # nightly 产物在 target\debug\build\smart-dl-btcore\<hash>\out\ 下；
    # pwsh 会把数组拼为单参数（逗号连接）→ 用 cmd /c 显式拼命令行
    $exes = @(Get-ChildItem "$wt\debug\deps\*.exe", "$wt\debug\build\smart-dl-btcore\*\out\*.exe" -ErrorAction SilentlyContinue)
    $objs = @($exes | ForEach-Object { '"--object=' + $_.FullName + '"' })
    $cmdline = '"' + (Join-Path $llvm 'llvm-cov.exe') + '" report "-instr-profile=' + "$wt\cov.profdata" + '" ' + ($objs -join ' ') + ' --ignore-filename-regex=\.cargo --ignore-filename-regex=rustc --ignore-filename-regex=registry --ignore-filename-regex=bindings\.rs'
    cmd /c "$cmdline 2>&1" | Tee-Object -FilePath "$wt\cov-report.txt"

    $line = Select-String -Path "$wt\cov-report.txt" -Pattern 'TOTAL' | Select-Object -First 1
    if ($line) { Write-Output "[04_coverage] $($line.Line.Trim())" }
    else { throw "llvm-cov report 无 TOTAL 行（见 $wt\cov-report.txt）" }
} finally {
    Pop-Location
    Remove-Item Env:RUSTFLAGS -ErrorAction SilentlyContinue
    Remove-Item Env:LLVM_PROFILE_FILE -ErrorAction SilentlyContinue
}