//! HTTP/FTP 引擎（M4 交付）：reqwest 传输 + 自研调度层（分块/续传/重试/换源/镜像/校验）。
//! M4a 骨架：Range 探测 / 静态分块规划 / .part 续传决策 / 重试退避 / 引擎生命周期。

// 测试以 `httpdl::` 引用（包名为 smart-dl-httpdl）。
extern crate self as httpdl;

pub mod engine;
pub mod range;
pub mod resume;
pub mod retry;
pub mod static_split;

pub use engine::HttpEngine;