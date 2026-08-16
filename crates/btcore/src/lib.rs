//! libtorrent FFI 绑定（M0/M1 交付）：手写 extern "C" 对齐 ffi/lt.h。
//! unsafe 只允许出现在本 crate；对外只暴露 safe API（BtCore / Bare）。

// bindgen 产物为 C 风格命名，整 crate 放行（绑定层内部约定）
#![allow(non_camel_case_types, non_snake_case)]

pub mod bare;
pub use bare::{Bare, Error, Result};
