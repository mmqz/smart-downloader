# M0.1 工具链验收脚本 01：vcpkg 安装 libtorrent（manifest 模式）
# 注意（本机 2026-08-16 环境事实）：沙箱拦截 schannel →
#   本脚本需在加宽模式（danger-full-access）下由执行器运行；
#   git clone 用 openssl 后端（GIT_CONFIG_* 注入）免加宽。
$ErrorActionPreference = 'Stop'
$root = Split-Path -Parent $PSScriptRoot | Split-Path -Parent   # smart-downloader/
$vk = Join-Path $root '.tools\vcpkg'

if (-not (Test-Path "$vk\vcpkg.exe")) {
  if (Test-Path $vk) { Remove-Item -Recurse -Force $vk }
  $env:GIT_CONFIG_COUNT = 1
  $env:GIT_CONFIG_KEY_0 = 'http.sslBackend'
  $env:GIT_CONFIG_VALUE_0 = 'openssl'
  git clone --depth 1 https://github.com/microsoft/vcpkg $vk
  & "$vk\bootstrap-vcpkg.bat" -disableMetrics
  if ($LASTEXITCODE -ne 0) { throw "bootstrap failed" }
}
Push-Location (Join-Path $root 'ffi')
& "$vk\vcpkg.exe" install --triplet x64-windows
$code = $LASTEXITCODE
Pop-Location
if ($code -ne 0) { throw "vcpkg install failed: $code" }
if (-not (Test-Path "$vk\installed\x64-windows\include\libtorrent\version.hpp")) {
  throw "libtorrent headers missing after install"
}
Write-Output "[01_vcpkg] OK"