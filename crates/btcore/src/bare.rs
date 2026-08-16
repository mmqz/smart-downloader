//! M0 safe 门面：Bare（最小内核，6 函数）。unsafe 只在本模块。
//! 契约对齐 ffi/lt.h（M0 子集）；完整 BtCore 见 M1。

use std::ffi::{CStr, CString};
use std::os::raw::{c_char, c_int, c_float, c_uint, c_ushort};
use std::path::Path;
use std::ptr;

include!("../bindings.rs"); // bindgen 产物（build.rs 生成）

#[derive(Debug, thiserror::Error)]
pub enum Error {
    #[error("invalid argument")]
    Arg,
    #[error("engine error")]
    Engine,
    #[error("io error")]
    Io,
    #[error("torrent not found: {0}")]
    NotFound(String),
    #[error("buffer too small")]
    BufferTooSmall,
}

impl From<c_int> for Error {
    fn from(code: c_int) -> Self {
        match code {
            lt_err_LT_ERR_ARG => Error::Arg,
            lt_err_LT_ERR_ENGINE => Error::Engine,
            lt_err_LT_ERR_IO => Error::Io,
            lt_err_LT_ERR_NOT_FOUND => Error::NotFound("".into()),
            lt_err_LT_ERR_BUFFER_TOO_SMALL => Error::BufferTooSmall,
            _ => Error::Engine,
        }
    }
}

pub type Result<T> = std::result::Result<T, Error>;

/// M0 最小内核句柄（libtorrent session 的 safe 包装）。
/// 每次新建即独立 session；Drop 时释放。M1 起由 BtCore 取代（多 session + 事件）。
pub struct Bare {
    raw: *mut lt_session,
}

// lt_session 只经 FFI 访问，Send/Sync 安全由 C++ 内核保证（libtorrent session 自身线程安全）
unsafe impl Send for Bare {}
unsafe impl Sync for Bare {}

impl Bare {
    pub fn new(save_path: &Path, session_id: &str) -> Result<Self> {
        let sp = CString::new(save_path.to_string_lossy().as_bytes()).map_err(|_| Error::Arg)?;
        let id = CString::new(session_id).map_err(|_| Error::Arg)?;
        let mut raw: *mut lt_session = ptr::null_mut();
        let code = unsafe {
            lt_session_new(sp.as_ptr(), id.as_ptr(), &mut raw)
        };
        if code != lt_err_LT_OK {
            return Err(Error::from(code));
        }
        Ok(Self { raw })
    }

    /// 返回 40 hex infohash（任务稳定 ID，D8）
    pub fn add_magnet(&self, magnet: &str, web_seeds: &[String]) -> Result<String> {
        let m = CString::new(magnet).map_err(|_| Error::Arg)?;
        // web_seeds：NULL 结尾的 C 字符串数组
        let cseeds: Vec<CString> = web_seeds
            .iter()
            .map(|s| CString::new(s.as_str()).map_err(|_| Error::Arg))
            .collect::<Result<_>>()?;
        let ptrs: Vec<*const c_char> = cseeds.iter().map(|c| c.as_ptr()).collect();
        let seeds_ptr: *const *const c_char = if ptrs.is_empty() { ptr::null() } else { ptrs.as_ptr() };

        let mut ih = [0i8; 41];
        let code = unsafe { lt_add_magnet(self.raw, m.as_ptr(), seeds_ptr, ih.as_mut_ptr()) };
        if code != lt_err_LT_OK {
            return Err(Error::from(code));
        }
        let s = unsafe { CStr::from_ptr(ih.as_ptr()) }.to_string_lossy().into_owned();
        Ok(s)
    }

    pub fn add_peer(&self, ih: &str, ip: &str, port: u16) -> Result<()> {
        let i = CString::new(ih).map_err(|_| Error::Arg)?;
        let a = CString::new(ip).map_err(|_| Error::Arg)?;
        let code = unsafe { lt_add_peer(self.raw, i.as_ptr(), a.as_ptr(), port) };
        if code != lt_err_LT_OK {
            return Err(Error::from(code));
        }
        Ok(())
    }

    /// (progress, state)；state 0 下载 1 完成 2 暂停 3 错误 4 元数据获取中
    pub fn status(&self, ih: &str) -> Result<(f32, i32)> {
        let i = CString::new(ih).map_err(|_| Error::Arg)?;
        let mut st = lt_torrent_status {
            state: 0,
            progress: 0.0,
            downloaded: 0,
            total: 0,
            down_rate: 0,
            up_rate: 0,
            num_peers: 0,
            num_seeds: 0,
            metadata_received: 0,
        };
        let code = unsafe { lt_status(self.raw, i.as_ptr(), &mut st) };
        if code != lt_err_LT_OK {
            return Err(Error::from(code));
        }
        Ok((st.progress, st.state))
    }
}

impl Drop for Bare {
    fn drop(&mut self) {
        unsafe { lt_session_free(self.raw) };
    }
}
