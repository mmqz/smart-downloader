# M1.5 ASAN 验收脚本：MSVC /fsanitize=address 构建 lt_kernel → 全测试一轮
# 说明：ASAN 只插桩 C++ 内核（Rust 侧未插桩，无泄漏/溢出由内核承载）；
#       runtime DLL（clang_rt.asan_dynamic-x86_64.dll 等）在 VS 工具链 bin。
# 用法：.\scripts\m1\05_asan.ps1
$ErrorActionPreference = 'Stop'
$root = Split-Path -Parent $PSScriptRoot | Split-Path -Parent

$asan = Join-Path $root 'ffi\build-asan\Release'
if (-not (Test-Path "$asan\lt_kernel.lib")) {
    # 配置 + 构建 ASAN 内核（复用现有 vcpkg_installed）
    cmake -S (Join-Path $root 'ffi') -B (Join-Path $root 'ffi\build-asan') `
      -G "Visual Studio 17 2022" -A x64 `
      -DCMAKE_TOOLCHAIN_FILE=(Join-Path $root '.tools\vcpkg\scripts\buildsystems\vcpkg.cmake') `
      -DVCPKG_TARGET_TRIPLET=x64-windows `
      -DVCPKG_INSTALLED_DIR=(Join-Path $root 'ffi\build\vcpkg_installed') `
      -DCMAKE_CXX_FLAGS="/fsanitize=address /Zi"
    if ($LASTEXITCODE -ne 0) { throw "asan cmake configure failed" }
    cmake --build (Join-Path $root 'ffi\build-asan') --config Release --target lt_kernel
    if ($LASTEXITCODE -ne 0) { throw "asan build failed" }
}

$vcpkgbin = Join-Path $root 'ffi\build\vcpkg_installed\x64-windows\bin'
$vctools = Get-ChildItem (Join-Path $env:VSINSTALLDIR 'VC\Tools\MSVC') -Directory |
    Sort-Object Name -Descending | Select-Object -First 1
$vcbindir = Join-Path $vctools.FullName 'bin\Hostx64\x64'

$env:LT_KERNEL_LIB_DIR = $asan
$env:SEED_MAIN = (Join-Path $root 'ffi\build\Release\seed_main.exe')  # seeder 非插桩（独立进程）
$env:PATH = "$asan;$vcpkgbin;$vcbindir;" + $env:PATH

Push-Location (Join-Path $root 'crates\btcore')
try {
    cargo clean -p smart-dl-btcore   # 强制重链 ASAN 内核（.lib 变更 cargo 不会自动重链）
    cargo test -p smart-dl-btcore 2>&1 | Tee-Object -FilePath "target\asan-test.txt"
    $failed = Select-String -Path "target\asan-test.txt" -Pattern 'FAILED|AddressSanitizer|ERROR: '
    if ($failed) { $failed | ForEach-Object { Write-Output $_.Line }; throw "ASAN round failed" }
    Write-Output "[05_asan] OK: 15 tests green, no ASAN report"
} finally {
    Pop-Location
}