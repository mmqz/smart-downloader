# M0.3 ASAN 配置就绪（M1 内存模型/alert 生命周期验证前置）

> M0 出口自检清单第 6 项。M1（FFI 全量）用 ASAN 验证 FFI 内存模型
> 与 alert 生命周期（设计 §8.5 / TDD M1 Step 4 / 风险表）。

## 现状

- 未启用任何 sanitizer 的正常构建（`02_build.ps1`）已全绿；本文件 = M1 的 ASAN 配置预案。
- 涉及两侧：C++（lt_kernel.cpp，MSVC）与 Rust（btcore，MSVC toolchain）。

## C++ 侧（MSVC ASAN）

```powershell
# 在 ffi/build-asan 下单独配置（不动正常 build 目录）
cmake -S ffi -B ffi/build-asan `
  -DCMAKE_TOOLCHAIN_FILE="$root\.tools\vcpkg\scripts\buildsystems\vcpkg.cmake" `
  -DVCPKG_TARGET_TRIPLET=x64-windows `
  -DCMAKE_BUILD_TYPE=RelWithDebInfo `
  -DCMAKE_MSVC_RUNTIME_LIBRARY=MultiThreadedDLL `
  -DCMAKE_CXX_FLAGS="/fsanitize=address /Zi /Od"
cmake --build ffi/build-asan --config RelWithDebInfo
```

- MSVC `/fsanitize=address` 需要运行时 `clang_rt.asan_dynamic-x86_64.dll`（VS 安装目录
  `VC\Tools\MSVC\<ver>\bin\Hostx64\x64\` 下有；/fsanitize=address 构建会输出到该目录），
  运行测试时保证其在 PATH 或与 exe 同目录。
- 局限：MSVC ASAN 不捕获 new/delete 不匹配与 vector 越界之外的少数 UB；栈上/全局溢出覆盖
  足够本项目的 FFI 缓冲契约（D13：Rust 预分配 + cap，C++ 填 ≤cap）。

## Rust 侧（nightly -Zsanitizer）

```powershell
# nightly 工具链（BT 侧；MSVC 目标）
$env:RUSTFLAGS="-Zsanitizer=address"
cargo +nightly test -p smart-dl-btcore --target x86_64-pc-windows-msvc
```

- MSVC ASAN 与 Rust 侧 ASAN 运行时不互通，两侧分别出报告，**叠加人工核对 FFI 边界
  （lt.h 契约自测：cap 边界、NULL 入参、双 free）**——run_ffi_safety fuzz 测试放 M1.
- 备注：Rust nightly + MSVC ASAN 对 bindgen 生成的 extern 块可用（C ABI + 不透明指针），
  已在 spike/ 手写 ABI 方案（D14）下验证无额外依赖。

## M1 验收触发条件

`cargo test -p btcore` 全绿后，另跑一轮：
`scripts/m0/03_asan_prep.md` 中 C++ 侧 + Rust 侧两个命令，任一报告 → 记缺陷修到 0 再进 M2。