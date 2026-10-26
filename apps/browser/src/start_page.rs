//! 内蔵スタートページ。127.0.0.1 の空きポートで静的な HTML を返すだけの最小サーバ。
//! 核は http(s) 以外の URL を拒否するため、`file:` や `data:` ではなく http で配る。
//! 受け付けるのは `Host` が自分自身の `GET /` のみ(DNS rebinding 対策)で、ループバックのみに bind する。
//! 接続ごとに別スレッドで処理し、同時数と1接続の総時間に上限を設ける(接続占有による停止を防ぐ)。

use std::io::{Read, Write};
use std::net::{TcpListener, TcpStream};
use std::sync::Arc;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::thread;
use std::time::{Duration, Instant};

const PAGE: &str = include_str!("../assets/start.html");
/// リクエストヘッダとして読む上限(バイト)。
const HEAD_LIMIT: usize = 4096;
/// 1 接続に使える総時間。
const DEADLINE: Duration = Duration::from_secs(2);
/// 同時に処理する接続数の上限。超えた分は即座に閉じる。
const MAX_CONNECTIONS: usize = 8;

const CSP: &str = "default-src 'none'; style-src 'unsafe-inline'; form-action https://duckduckgo.com; \
                   base-uri 'none'; frame-ancestors 'none'";

/// 処理中の接続数を、drop 時に必ず減らす。
struct ConnGuard(Arc<AtomicUsize>);

impl Drop for ConnGuard {
    fn drop(&mut self) {
        self.0.fetch_sub(1, Ordering::SeqCst);
    }
}

/// サーバを起動し、スタートページの URL(末尾 `/` 付き)を返す。
pub fn spawn() -> std::io::Result<String> {
    let listener = TcpListener::bind(("127.0.0.1", 0))?;
    let port = listener.local_addr()?.port();
    let active = Arc::new(AtomicUsize::new(0));
    thread::Builder::new()
        .name("start-page".into())
        .spawn(move || {
            for stream in listener.incoming().flatten() {
                if active.fetch_add(1, Ordering::SeqCst) >= MAX_CONNECTIONS {
                    active.fetch_sub(1, Ordering::SeqCst);
                    continue; // 上限超過: 閉じて捨てる。
                }
                // スレッドを作れなかった場合も、クロージャごと drop されてカウントが戻る。
                let guard = ConnGuard(Arc::clone(&active));
                let _ = thread::Builder::new()
                    .name("start-page-conn".into())
                    .spawn(move || {
                        // 接続ごとのエラーは無視して次を待つ。
                        let _ = serve(stream, port);
                        drop(guard);
                    });
            }
        })?;
    Ok(format!("http://127.0.0.1:{port}/"))
}

/// ヘッダ終端(`\r\n\r\n`)か上限まで、総時間の締切内で読む。
fn read_head(stream: &mut TcpStream) -> String {
    let start = Instant::now();
    let mut buf = Vec::with_capacity(512);
    let mut chunk = [0u8; 512];
    while buf.len() < HEAD_LIMIT && !buf.windows(4).any(|w| w == b"\r\n\r\n") {
        let Some(left) = DEADLINE.checked_sub(start.elapsed()) else {
            break;
        };
        if stream.set_read_timeout(Some(left)).is_err() {
            break;
        }
        match stream.read(&mut chunk) {
            Ok(0) | Err(_) => break,
            Ok(n) => buf.extend_from_slice(&chunk[..n]),
        }
    }
    String::from_utf8_lossy(&buf).into_owned()
}

fn serve(mut stream: TcpStream, port: u16) -> std::io::Result<()> {
    stream.set_write_timeout(Some(DEADLINE))?;
    let head = read_head(&mut stream);
    stream.write_all(response_for(&head, port).as_bytes())?;
    stream.shutdown(std::net::Shutdown::Write)
}

fn host_header(head: &str) -> Option<&str> {
    head.lines().skip(1).find_map(|l| {
        let (name, value) = l.split_once(':')?;
        name.eq_ignore_ascii_case("host").then(|| value.trim())
    })
}

fn response_for(head: &str, port: u16) -> String {
    let request_line = head.lines().next().unwrap_or("");
    let expected = format!("127.0.0.1:{port}");
    if !request_line.starts_with("GET / ") {
        return "HTTP/1.1 404 Not Found\r\nContent-Length: 0\r\nConnection: close\r\n\r\n"
            .to_owned();
    }
    if host_header(head) != Some(expected.as_str()) {
        return "HTTP/1.1 403 Forbidden\r\nContent-Length: 0\r\nConnection: close\r\n\r\n"
            .to_owned();
    }
    format!(
        "HTTP/1.1 200 OK\r\nContent-Type: text/html; charset=utf-8\r\nContent-Length: {}\r\n\
         Cache-Control: no-store\r\nX-Content-Type-Options: nosniff\r\nX-Frame-Options: DENY\r\n\
         Referrer-Policy: no-referrer\r\nContent-Security-Policy: {CSP}\r\nConnection: close\r\n\r\n{PAGE}",
        PAGE.len()
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    const PORT: u16 = 4000;

    fn get(path: &str, host: &str) -> String {
        response_for(
            &format!("GET {path} HTTP/1.1\r\nHost: {host}\r\n\r\n"),
            PORT,
        )
    }

    #[test]
    fn root_with_matching_host_is_served_with_security_headers() {
        let r = get("/", "127.0.0.1:4000");
        assert!(r.starts_with("HTTP/1.1 200"));
        assert!(r.contains("Content-Security-Policy: default-src 'none'"));
        assert!(r.contains("X-Frame-Options: DENY"));
    }

    #[test]
    fn foreign_host_is_forbidden_to_stop_dns_rebinding() {
        assert!(get("/", "evil.example:4000").starts_with("HTTP/1.1 403"));
        assert!(get("/", "127.0.0.1:4001").starts_with("HTTP/1.1 403"));
        assert!(response_for("GET / HTTP/1.1\r\n\r\n", PORT).starts_with("HTTP/1.1 403"));
    }

    #[test]
    fn other_paths_and_methods_are_404() {
        assert!(get("/secret", "127.0.0.1:4000").starts_with("HTTP/1.1 404"));
        assert!(
            response_for("POST / HTTP/1.1\r\nHost: 127.0.0.1:4000\r\n\r\n", PORT)
                .starts_with("HTTP/1.1 404")
        );
        assert!(response_for("", PORT).starts_with("HTTP/1.1 404"));
    }

    #[test]
    fn page_has_search_form_and_japanese_title() {
        assert!(PAGE.contains("name=\"q\""));
        assert!(PAGE.contains("<title>はじまりのページ</title>"));
    }

    #[test]
    fn stalled_client_does_not_block_a_following_request() {
        let url = spawn().unwrap();
        let port: u16 = url
            .trim_end_matches('/')
            .rsplit(':')
            .next()
            .unwrap()
            .parse()
            .unwrap();
        // 何も送らない接続を 2 本つないだまま、別の接続が即座に応答される。
        let _idle1 = TcpStream::connect(("127.0.0.1", port)).unwrap();
        let _idle2 = TcpStream::connect(("127.0.0.1", port)).unwrap();
        let started = Instant::now();
        let mut c = TcpStream::connect(("127.0.0.1", port)).unwrap();
        c.write_all(format!("GET / HTTP/1.1\r\nHost: 127.0.0.1:{port}\r\n\r\n").as_bytes())
            .unwrap();
        let mut out = String::new();
        c.read_to_string(&mut out).unwrap();
        assert!(out.starts_with("HTTP/1.1 200"));
        assert!(started.elapsed() < Duration::from_millis(1500));
    }
}
