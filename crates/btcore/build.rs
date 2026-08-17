// crates/btcore/build.rs — bindgen 从 ffi/lt.h 生成绑定 + 链接 lt_kernel 静态库。
// 契约流（D14/F0）：lt.h（手写，契约定死）→ lt_kernel.cpp 实现 → bindgen 生成 Rust 声明。
// 链接：
//   LT_KERNEL_LIB_DIR — lt_kernel.lib 所在目录（默认 ../../ffi/build，Release 产物在
//                       ffi/build/Release，构建脚本按存在性探测）。
//   LT_VCPKG_LIB_DIR  — vcpkg 安装的 .lib 目录（默认 ../../ffi/vcpkg_installed/x64-windows/lib），
//                       逐个以 dylib=<stem> 链接（libtorrent 为动态库，lt_kernel 引用其
//                       dllimport 符号；boost/openssl 等传递依赖一并带上）。

use std::env;
use std::path::PathBuf;

fn main() {
    let manifest = env::var("CARGO_MANIFEST_DIR").unwrap();
    let repo = PathBuf::from(&manifest).join("..").join("..");

    let lt_h = repo.join("ffi").join("lt.h");
    let bindings = bindgen::Builder::default()
        .header(lt_h.to_str().unwrap())
        .parse_callbacks(Box::new(bindgen::CargoCallbacks::new()))
        .generate_comments(false)
        .generate()
        .expect("bindgen failed on ffi/lt.h");
    bindings
        .write_to_file(PathBuf::from(&manifest).join("bindings.rs"))
        .expect("write bindings.rs failed");
    println!("cargo:rerun-if-changed={}", lt_h.display());

    // lt_kernel.lib：默认 ffi/build，优先 Release/debug 探测
    let lib_dir = env::var("LT_KERNEL_LIB_DIR")
        .map(PathBuf::from)
        .unwrap_or_else(|_| {
            let mut cand = repo.join("ffi").join("build");
            for sub in ["Release", "Debug", ""] {
                let p = cand.join(sub);
                if p.join("lt_kernel.lib").exists() {
                    return p;
                }
            }
            cand
        });
    println!("cargo:rustc-link-search=native={}", lib_dir.display());
    println!("cargo:rustc-link-lib=static=lt_kernel");

    // vcpkg 动态库导入库：逐个链接（与 ffi cmake 的 LibtorrentRasterbar 传递依赖一致）
    let vcpkg_lib = env::var("LT_VCPKG_LIB_DIR")
        .map(PathBuf::from)
        .unwrap_or_else(|_| repo.join("ffi").join("vcpkg_installed").join("x64-windows").join("lib"));
    println!("cargo:rustc-link-search=native={}", vcpkg_lib.display());
    if let Ok(entries) = std::fs::read_dir(&vcpkg_lib) {
        let mut names: Vec<String> = entries
            .filter_map(|e| e.ok())
            .filter(|e| e.path().extension().map(|x| x == "lib").unwrap_or(false))
            .filter_map(|e| e.path().file_stem().map(|s| s.to_string_lossy().into_owned()))
            .collect();
        names.sort();
        for stem in names {
            println!("cargo:rustc-link-lib=dylib={}", stem);
        }
    }
    println!("cargo:rerun-if-changed={}", vcpkg_lib.display());
}