cmake_minimum_required(VERSION 3.20)
project(smart_dl_ffi LANGUAGES CXX)

set(CMAKE_CXX_STANDARD 17)
set(CMAKE_CXX_STANDARD_REQUIRED ON)
# 源码统一 UTF-8（避免 GBK 代码页下中文注释损坏预处理；新文件也建议纯 ASCII 注释）
add_compile_options(/utf-8)

# libtorrent 由 vcpkg manifest 提供（ffi/vcpkg.json）。
# vcpkg 端口导出名为 LibtorrentRasterbar（见 vcpkg-manifest-install.log 的 usage 提示）：
#   find_package(LibtorrentRasterbar CONFIG REQUIRED)
#   target_link_libraries(main PRIVATE LibtorrentRasterbar::torrent-rasterbar)
find_package(LibtorrentRasterbar CONFIG REQUIRED)

# 静态库：C ABI 内核（Rust btcore 经 build.rs 链接）
add_library(lt_kernel STATIC src/lt_kernel.cpp)
target_include_directories(lt_kernel PRIVATE ${CMAKE_CURRENT_SOURCE_DIR}) # lt.h 位于 ffi/
target_link_libraries(lt_kernel PRIVATE LibtorrentRasterbar::torrent-rasterbar)

# 测试 seeder 可执行（M0 E2E 用；不参与 Rust 链接）
add_executable(seed_main src/seed_main.cpp)
target_link_libraries(seed_main PRIVATE LibtorrentRasterbar::torrent-rasterbar)