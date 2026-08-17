// spike/cxx/build.rs — cxx bridge 构建 + 链接 vcpkg libtorrent（approach B）
use std::env;
use std::path::PathBuf;

fn main() {
    let manifest = env::var("CARGO_MANIFEST_DIR").unwrap();
    // vcpkg manifest 安装产物位于 ffi/vcpkg_installed/x64-windows（与 ffi/CMakeLists 一致）
    let installed = env::var("VCPKG_INSTALLED").unwrap_or_else(|_| {
        PathBuf::from(&manifest)
            .join("..")
            .join("..")
            .join("ffi")
            .join("vcpkg_installed")
            .join("x64-windows")
            .to_str()
            .unwrap()
            .to_string()
    });
    let inc = format!("{}/include", installed);
    let lib = format!("{}/lib", installed);

    cxx_build::bridge("src/lib.rs")
        .file("src/spike_impl.cpp")
        .include("src")
        .include(&inc)
        .std("c++17")
        .flag_if_supported("/utf-8")
        .compile("ffi_cxx_spike");

    println!("cargo:rerun-if-changed=src/spike_impl.hpp");
    println!("cargo:rerun-if-changed=src/spike_impl.cpp");

    println!("cargo:rustc-link-search=native={}", lib);
    // vcpkg x64-windows 为动态库：链接导入库；DLL 运行时需在 PATH（spike 比较"编译通过"为准）
    for entry in std::fs::read_dir(&lib).expect("read vcpkg lib dir") {
        let p = entry.unwrap().path();
        if p.extension().map(|e| e == "lib").unwrap_or(false) {
            if let Some(stem) = p.file_stem().and_then(|s| s.to_str()) {
                println!("cargo:rustc-link-lib=dylib={}", stem);
            }
        }
    }
}