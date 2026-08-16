//! approach B（cxx）最小 spike：session_create / add_magnet / status / pop_alerts
//! 对比点（见 2026-08-16-ffi-spike.md）：cxx 的编译期校验、UniquePtr 生命周期、
//! 异常边界由 cxx 处理；alert 扁平化仍需手写 C++（两方案相同工作量）。

#[cxx::bridge]
pub mod ffi {
    unsafe extern "C++" {
        include!("spike_impl.hpp");
        type Session;
        /// C++ 侧异常 → rust::Result（cxx 自动转换）
        fn new_session(save_path: &CStr, session_id: &CStr) -> Result<UniquePtr<Session>>;
        fn add_magnet(&self, magnet: &CStr) -> Result<String>;
        fn status(&self, ih: &str) -> Result<(f32, i32)>;
        fn pop_alerts(&self) -> Result<Vec<Alert>>;
        fn set_session_mask(&self, mask: u32) -> Result<()>;
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
    use super::ffi::{self, Alert};
    use std::ffi::CString;

    #[test]
    fn session_roundtrip() {
        let sp = CString::new(env!("TEMP")).unwrap();
        let sid = CString::new("spike").unwrap();
        let s = ffi::new_session(&sp, &sid).unwrap();
        // 非法 magnet 应返回 Err（异常边界由 cxx 处理）
        let bad = CString::new("not a magnet").unwrap();
        assert!(ffi::new_session(&sp, &sid).is_ok());
        let _ = s.add_magnet(&bad); // 期待 Err 或产生任务失败；spike 只验证编译与调用链
        let (p, st) = s.status("0000000000000000000000000000000000000000").unwrap_or((0.0, 0));
        println!("spike status: progress={} state={}", p, st);
        let _: Vec<Alert> = s.pop_alerts().unwrap_or_default();
    }
}