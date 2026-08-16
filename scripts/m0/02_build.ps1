# M0.1 工具链验收脚本 02：CMake 构建 ffi（lt_kernel 静态库 + seed_main 可执行）
# 用法：.\scripts\m0\02_build.ps1
$ErrorActionPreference = 'Stop'
$root = Split-Path -Parent $PSScriptRoot | Split-Path -Parent   # smart-downloader/
$vk = Join-Path $root '.tools\vcpkg'
$toolchain = "$vk\scripts\buildsystems\vcpkg.cmake"
if (-not (Test-Path $toolchain)) { throw "vcpkg toolchain not found: $toolchain" }

$buildDir = Join-Path $root 'ffi\build'
New-Item -ItemType Directory -Force -Path $buildDir | Out-Null

# VS 生成器（CMake 自发现 VS 2022 BuildTools）；Release 配置
cmake -S (Join-Path $root 'ffi') -B $buildDir `
  -DCMAKE_TOOLCHAIN_FILE="$toolchain" `
  -DVCPKG_TARGET_TRIPLET=x64-windows `
  -DCMAKE_BUILD_TYPE=Release `
  -DCMAKE_MSVC_RUNTIME_LIBRARY=MultiThreadedDLL
if ($LASTEXITCODE -ne 0) { throw "cmake configure failed" }

cmake --build $buildDir --config Release --parallel
if ($LASTEXITCODE -ne 0) { throw "cmake build failed" }

if (-not (Test-Path "$buildDir\Release\lt_kernel.lib")) { throw "lt_kernel.lib missing" }
if (-not (Test-Path "$buildDir\Release\seed_main.exe")) { throw "seed_main.exe missing" }
Write-Output "[02_build] OK: lt_kernel.lib + seed_main.exe"