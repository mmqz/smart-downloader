cmake_minimum_required(VERSION 3.20)
project(smart_dl_ffi LANGUAGES CXX)

set(CMAKE_CXX_STANDARD 17)
set(CMAKE_CXX_STANDARD_REQUIRED ON)

# libtorrent 由 vcpkg manifest 提供（ffi/vcpkg.json）：
#   cmake -DCMAKE_TOOLCHAIN_FILE=<vcpkg>/scripts/buildsystems/vcpkg.cmake ..
find_package(libtorrent CONFIG REQUIRED)

# 静态库：C ABI 内核（Rust btcore 经 build.rs 链接）
add_library(lt_kernel STATIC src/lt_kernel.cpp)
target_link_libraries(lt_kernel PRIVATE libtorrent::libtorrent) # 目标名以 vcpkg 导出为准，编译期核对

# 测试 seeder 可执行（M0 E2E 用；不参与 Rust 链接）
add_executable(seed_main src/seed_main.cpp)
target_link_libraries(seed_main PRIVATE libtorrent::libtorrent)
