//! libtorrent FFI 绑定（M0/M1 交付）：手写 extern "C" 对齐 ffi/lt.h。
//! unsafe 只允许出现在本 crate；对外只暴露 safe API（BtCore / Bare）。

pub mod bare;
pub use bare::{Bare, Error, Result};
