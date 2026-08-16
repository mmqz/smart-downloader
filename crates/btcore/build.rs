// crates/btcore/build.rs — bindgen 从 ffi/lt.h 生成绑定 + 链接 lt_kernel 静态库。
// 契约流（D14/F0）：lt.h（手写，契约定死）→ lt_kernel.cpp 实现 → bindgen 生成 Rust 声明。
// 链接：LT_KERNEL_LIB_DIR 环境变量可覆盖 CMake 构建产物目录（默认 ../../ffi/build）。

use std::env;
use std::path::PathBuf;

fn main() {
    let manifest = env::var("CARGO_MANIFEST_DIR").unwrap();
    let lt_h = PathBuf::from(&manifest)
        .join("..")
        .join("..")
        .join("ffi")
        .join("lt.h");

    let bindings = bindgen::Builder::default()
        .header(lt_h.to_str().unwrap())
        .parse_callbacks(Box::new(bindgen::CargoCallbacks::new()))
        .generate_comments(false)
        .generate()
        .expect("bindgen failed on ffi/lt.h");
    bindings
        .write_to_file(PathBuf::from(&manifest).join("bindings.rs"))
        .expect("write bindings.rs failed");

    let lib_dir = env::var("LT_KERNEL_LIB_DIR")
        .map(PathBuf::from)
        .unwrap_or_else(|_| PathBuf::from(&manifest).join("..").join("..").join("ffi").join("build"));
    println!("cargo:rustc-link-search=native={}", lib_dir.display());
    println!("cargo:rustc-link-lib=static=lt_kernel");
    println!("cargo:rerun-if-changed={}", lt_h.display());
}
