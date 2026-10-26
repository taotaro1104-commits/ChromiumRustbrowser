// Rust 側(crates/chromium-ffi)と C++ shim の取り決め。P1 で Chromium の Content API を呼ぶ shim が実装する。
// 方針: ポインタは公開しない(タブは u64 の ID)。文字列は (ptr, len) の UTF-8 で NUL 終端を要求しない。
// 戻り値: RB_OK=成功 / RB_UNKNOWN_TAB=その ID のタブが無い / それ以外=失敗。
#pragma once
#include <stddef.h>
#include <stdint.h>

#ifdef __cplusplus
extern "C" {
#endif

enum { RB_OK = 0, RB_UNKNOWN_TAB = 1, RB_FAILED = 2 };

typedef struct RbShimApi {
  // 以下はすべて UI スレッドから呼ばれる。`ctx` は shim が起動時に Rust へ渡した不透明な値。
  int32_t (*create_tab)(void* ctx, uint64_t tab, const char* url, size_t url_len);
  int32_t (*navigate)(void* ctx, uint64_t tab, const char* url, size_t url_len);
  int32_t (*freeze)(void* ctx, uint64_t tab);    // WebContents::SetPageFrozen(true)
  int32_t (*unfreeze)(void* ctx, uint64_t tab);  // WebContents::SetPageFrozen(false)
  int32_t (*discard)(void* ctx, uint64_t tab);   // WebContents を破棄し Renderer を解放
  int32_t (*restore)(void* ctx, uint64_t tab, const char* url, size_t url_len,
                     uint32_t scroll_y, const char* form, size_t form_len, int32_t has_form);
  int32_t (*close_tab)(void* ctx, uint64_t tab);
} RbShimApi;

#ifdef __cplusplus
}
#endif
