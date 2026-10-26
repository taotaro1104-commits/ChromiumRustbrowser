//! 内蔵スタートページ。127.0.0.1 の空きポートで静的な HTML を返すだけの最小サーバ。
//! 核は http(s) 以外の URL を拒否するため、`file:` や `data:` ではなく http で配る。
//! 受け付けるのは `GET /` のみで、外部からは到達できない(ループバックのみに bind)。

use std::io::{Read, Write};
use std::net::{TcpListener, TcpStream};
use std::thread;
use std::time::Duration;

const PAGE: &str = include_str!("../assets/start.html");
const READ_LIMIT: u64 = 4096;
const IO_TIMEOUT: Duration = Duration::from_secs(2);

/// サーバを起動し、スタートページの URL(末尾 `/` 付き)を返す。
pub fn spawn() -> std::io::Result<String> {
    let listener = TcpListener::bind(("127.0.0.1", 0))?;
    let url = format!("http://127.0.0.1:{}/", listener.local_addr()?.port());
    thread::Builder::new()
        .name("start-page".into())
        .spawn(move || {
            for stream in listener.incoming().flatten() {
                // 接続ごとのエラーは無視して次を待つ。
                let _ = serve(stream);
            }
        })?;
    Ok(url)
}

fn serve(mut stream: TcpStream) -> std::io::Result<()> {
    stream.set_read_timeout(Some(IO_TIMEOUT))?;
    stream.set_write_timeout(Some(IO_TIMEOUT))?;
    let mut buf = Vec::new();
    (&stream).take(READ_LIMIT).read_to_end(&mut buf).ok();
    let head = String::from_utf8_lossy(&buf);
    let response = response_for(head.lines().next().unwrap_or(""));
    stream.write_all(response.as_bytes())
}

fn response_for(request_line: &str) -> String {
    if request_line.starts_with("GET / ") {
        format!(
            "HTTP/1.1 200 OK\r\nContent-Type: text/html; charset=utf-8\r\nContent-Length: {}\r\n\
             Cache-Control: no-store\r\nX-Content-Type-Options: nosniff\r\nConnection: close\r\n\r\n{PAGE}",
            PAGE.len()
        )
    } else {
        "HTTP/1.1 404 Not Found\r\nContent-Length: 0\r\nConnection: close\r\n\r\n".to_owned()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn root_is_served_and_everything_else_is_404() {
        assert!(response_for("GET / HTTP/1.1").starts_with("HTTP/1.1 200"));
        assert!(response_for("GET /secret HTTP/1.1").starts_with("HTTP/1.1 404"));
        assert!(response_for("POST / HTTP/1.1").starts_with("HTTP/1.1 404"));
        assert!(response_for("").starts_with("HTTP/1.1 404"));
    }

    #[test]
    fn page_has_search_form_and_japanese_title() {
        assert!(PAGE.contains("name=\"q\""));
        assert!(PAGE.contains("<title>はじまりのページ</title>"));
    }
}
