//! approach B（cxx）最小 spike：session_create / add_magnet / status / pop_alerts
//! 对比点（见 2026-08-16-ffi-spike.md）：cxx 的编译期校验、UniquePtr 生命周期、
//! 异常边界由 cxx 处理；alert 扁平化仍需手写 C++（两方案相同工作量）。

#[cxx::bridge]
pub mod ffi {
    unsafe extern "C++" {
        include!("spike_impl.hpp");
        type Session;
        /// C++ 侧异常 → rust::Result（cxx 自动转换）
        fn new_session(save_path: &str, session_id: &str) -> Result<UniquePtr<Session>>;
        fn add_magnet(&self, magnet: &str) -> Result<String>;
        fn status(&self, ih: &str) -> Result<Status>;
        fn pop_alerts(&self) -> Result<Vec<Alert>>;
        fn set_session_mask(&self, mask: u32) -> Result<()>;
    }
    struct Status {
        progress: f32,
        state: i32,
    }
    struct Alert {
        kind: i32,
        ih: String,
        msg: String,
        at: i64,
    }
}

#[cfg(test)]
mod tests {
    use super::ffi::{self, Alert, Status};

    #[test]
    fn session_roundtrip() {
        let s = ffi::new_session(env!("TEMP"), "spike").unwrap();
        // 非法 magnet 应返回 Err（异常边界由 cxx 处理）
        let bad = "not a magnet";
        let _ = s.add_magnet(bad); // 期待 Err 或产生任务失败；spike 只验证编译与调用链
        let st = s.status("0000000000000000000000000000000000000000").unwrap_or(Status { progress: 0.0, state: 0 });
        println!("spike status: progress={} state={}", st.progress, st.state);
        let _: Vec<Alert> = s.pop_alerts().unwrap_or_default();
    }
}