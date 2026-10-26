//! C ABI の境界。このクレートで `unsafe` を使うのはここだけ。

#![allow(unsafe_code)]

use std::ffi::c_void;

use ffi_traits::{HostError, TabHandle, WebContentsHost};

pub const RB_OK: i32 = 0;
pub const RB_UNKNOWN_TAB: i32 = 1;
pub const RB_FAILED: i32 = 2;

type StrFn = unsafe extern "C" fn(*mut c_void, u64, *const u8, usize) -> i32;
type TabFn = unsafe extern "C" fn(*mut c_void, u64) -> i32;
type RestoreFn =
    unsafe extern "C" fn(*mut c_void, u64, *const u8, usize, u32, *const u8, usize, i32) -> i32;

/// `RbShimApi`(rb_shim.h)と同じ配置の関数表。
#[repr(C)]
#[derive(Clone, Copy)]
pub struct ShimApi {
    pub create_tab: StrFn,
    pub navigate: StrFn,
    pub freeze: TabFn,
    pub unfreeze: TabFn,
    pub discard: TabFn,
    pub restore: RestoreFn,
    pub close_tab: TabFn,
}

/// shim の関数表を呼ぶ `WebContentsHost`。
pub struct ChromiumHost {
    api: ShimApi,
    ctx: *mut c_void,
}

impl ChromiumHost {
    /// # Safety
    /// `api` の各関数は、`ctx` を渡されて UI スレッドから呼ばれる限り安全に動作し、
    /// 渡した文字列(ptr, len)を呼び出しの間だけ読むこと。`ctx` はこの値が生きている間有効であること。
    pub unsafe fn new(api: ShimApi, ctx: *mut c_void) -> Self {
        Self { api, ctx }
    }
}

fn check(tab: TabHandle, code: i32, op: &str) -> Result<(), HostError> {
    match code {
        RB_OK => Ok(()),
        RB_UNKNOWN_TAB => Err(HostError::UnknownTab(tab)),
        other => Err(HostError::Failed(format!("{op} returned {other}"))),
    }
}

impl WebContentsHost for ChromiumHost {
    fn create_tab(&mut self, tab: TabHandle, url: &str) -> Result<(), HostError> {
        // SAFETY: `new` の契約により、shim は呼び出しの間だけ (ptr, len) を読む。
        let code = unsafe { (self.api.create_tab)(self.ctx, tab.0, url.as_ptr(), url.len()) };
        check(tab, code, "create_tab")
    }

    fn navigate(&mut self, tab: TabHandle, url: &str) -> Result<(), HostError> {
        // SAFETY: 同上。
        let code = unsafe { (self.api.navigate)(self.ctx, tab.0, url.as_ptr(), url.len()) };
        check(tab, code, "navigate")
    }

    fn freeze(&mut self, tab: TabHandle) -> Result<(), HostError> {
        // SAFETY: 同上。
        check(tab, unsafe { (self.api.freeze)(self.ctx, tab.0) }, "freeze")
    }

    fn unfreeze(&mut self, tab: TabHandle) -> Result<(), HostError> {
        // SAFETY: 同上。
        check(
            tab,
            unsafe { (self.api.unfreeze)(self.ctx, tab.0) },
            "unfreeze",
        )
    }

    fn discard(&mut self, tab: TabHandle) -> Result<(), HostError> {
        // SAFETY: 同上。
        check(
            tab,
            unsafe { (self.api.discard)(self.ctx, tab.0) },
            "discard",
        )
    }

    fn restore(
        &mut self,
        tab: TabHandle,
        url: &str,
        scroll_y: u32,
        form_state: Option<&str>,
    ) -> Result<(), HostError> {
        let form = form_state.unwrap_or("");
        // SAFETY: 同上。`has_form` が 0 のとき form は空(長さ 0)で、shim は読まない。
        let code = unsafe {
            (self.api.restore)(
                self.ctx,
                tab.0,
                url.as_ptr(),
                url.len(),
                scroll_y,
                form.as_ptr(),
                form.len(),
                i32::from(form_state.is_some()),
            )
        };
        check(tab, code, "restore")
    }

    fn close_tab(&mut self, tab: TabHandle) -> Result<(), HostError> {
        // SAFETY: 同上。
        check(
            tab,
            unsafe { (self.api.close_tab)(self.ctx, tab.0) },
            "close_tab",
        )
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// 偽 shim: 呼び出しを文字列で記録し、`code` を戻り値にする。
    struct Fake {
        log: Vec<String>,
        code: i32,
    }

    fn text(p: *const u8, n: usize) -> String {
        // SAFETY: 呼び出し元(ChromiumHost)が有効な (ptr, len) を渡す。
        String::from_utf8_lossy(unsafe { std::slice::from_raw_parts(p, n) }).into_owned()
    }

    fn record(c: *mut c_void, line: String) -> i32 {
        // SAFETY: `ctx` は `with_host` が渡した `Fake`。
        let f = unsafe { &mut *c.cast::<Fake>() };
        f.log.push(line);
        f.code
    }

    unsafe extern "C" fn f_create(c: *mut c_void, t: u64, p: *const u8, n: usize) -> i32 {
        record(c, format!("create {t} {}", text(p, n)))
    }
    unsafe extern "C" fn f_navigate(c: *mut c_void, t: u64, p: *const u8, n: usize) -> i32 {
        record(c, format!("navigate {t} {}", text(p, n)))
    }
    unsafe extern "C" fn f_freeze(c: *mut c_void, t: u64) -> i32 {
        record(c, format!("freeze {t}"))
    }
    unsafe extern "C" fn f_unfreeze(c: *mut c_void, t: u64) -> i32 {
        record(c, format!("unfreeze {t}"))
    }
    unsafe extern "C" fn f_discard(c: *mut c_void, t: u64) -> i32 {
        record(c, format!("discard {t}"))
    }
    unsafe extern "C" fn f_close(c: *mut c_void, t: u64) -> i32 {
        record(c, format!("close {t}"))
    }
    unsafe extern "C" fn f_restore(
        c: *mut c_void,
        t: u64,
        p: *const u8,
        n: usize,
        y: u32,
        fp: *const u8,
        fl: usize,
        has: i32,
    ) -> i32 {
        let form = if has == 1 { text(fp, fl) } else { "-".into() };
        record(c, format!("restore {t} {} y={y} form={form}", text(p, n)))
    }

    fn api() -> ShimApi {
        ShimApi {
            create_tab: f_create,
            navigate: f_navigate,
            freeze: f_freeze,
            unfreeze: f_unfreeze,
            discard: f_discard,
            restore: f_restore,
            close_tab: f_close,
        }
    }

    /// 偽 shim を接続した `ChromiumHost` で `run` を実行し、shim が受けた呼び出しを返す。
    fn with_host(code: i32, run: impl FnOnce(&mut ChromiumHost)) -> Vec<String> {
        let mut fake = Fake {
            log: Vec::new(),
            code,
        };
        // SAFETY: `fake` はこのスコープの間生きており、偽 shim は `Fake` としてだけ読む。
        let mut host = unsafe { ChromiumHost::new(api(), (&raw mut fake).cast()) };
        run(&mut host);
        fake.log
    }

    #[test]
    fn every_operation_reaches_the_shim_with_its_arguments() {
        let t = TabHandle(7);
        let log = with_host(RB_OK, |h| {
            h.create_tab(t, "https://a.example/").unwrap();
            h.navigate(t, "https://b.example/日本語").unwrap();
            h.freeze(t).unwrap();
            h.unfreeze(t).unwrap();
            h.discard(t).unwrap();
            h.restore(t, "https://a.example/", 420, Some("[[0,\"x\"]]"))
                .unwrap();
            h.restore(t, "https://a.example/", 0, None).unwrap();
            h.close_tab(t).unwrap();
        });
        assert_eq!(
            log,
            [
                "create 7 https://a.example/",
                "navigate 7 https://b.example/日本語",
                "freeze 7",
                "unfreeze 7",
                "discard 7",
                "restore 7 https://a.example/ y=420 form=[[0,\"x\"]]",
                "restore 7 https://a.example/ y=0 form=-",
                "close 7",
            ]
        );
    }

    #[test]
    fn result_codes_map_to_host_errors() {
        let t = TabHandle(3);
        with_host(RB_UNKNOWN_TAB, |h| {
            assert_eq!(h.discard(t), Err(HostError::UnknownTab(t)));
        });
        with_host(RB_FAILED, |h| {
            assert!(matches!(h.freeze(t), Err(HostError::Failed(m)) if m.contains("freeze")));
        });
        with_host(99, |h| {
            assert!(matches!(
                h.create_tab(t, "https://a.example/"),
                Err(HostError::Failed(_))
            ));
        });
    }
}
