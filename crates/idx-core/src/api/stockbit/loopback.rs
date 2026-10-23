//! The local page `idx stockbit login` opens: a loopback server on
//! 127.0.0.1 that takes the pasted session, checks it, and reports back.
//!
//! Stockbit has no OAuth redirect that could hand idx a session, and the
//! browser will not give one site another site's cookie, so the page takes
//! the value two ways, both one click:
//! - **Read my clipboard**: the user copies `credentialStorage` in DevTools
//!   once and clicks the button (`navigator.clipboard`, allowed on localhost).
//! - **Bookmarklet**: a link rendered with this run's port and secret that
//!   reads `document.cookie` on stockbit.com and opens the page with the value
//!   in the URL *fragment*, which browsers never send to the server.
//!
//! Safeguards: bound to 127.0.0.1, a random port, a per-run secret path (no
//! secret, no page), a short life, `no-store`, no referrer, and a strict CSP.
//! The session value is held in memory only on the way to `stockbit::login`;
//! it is never written to a log or the response.

use std::io::{Read, Write};
use std::net::{Ipv4Addr, TcpListener, TcpStream};
use std::time::{Duration, Instant};

use crate::error::IdxError;

/// Largest paste accepted (the cookie is a few KB).
const MAX_BODY: usize = 16 * 1024;
/// Requests time out instead of pinning a thread; short, because a browser
/// preconnect sends nothing at all.
const IO_TIMEOUT: Duration = Duration::from_secs(3);
/// Connections answered at once; beyond this, new ones are dropped.
const MAX_CONNECTIONS: usize = 16;

/// A running login page.
pub struct LoginPage {
    listener: TcpListener,
    port: u16,
    secret: String,
}

impl LoginPage {
    /// Binds 127.0.0.1 on `port` (0 picks a free one).
    pub fn bind(port: u16) -> Result<Self, IdxError> {
        let listener = TcpListener::bind((Ipv4Addr::LOCALHOST, port)).map_err(|e| {
            IdxError::Io(format!(
                "cannot open the Stockbit login page on 127.0.0.1:{port}: {e}"
            ))
        })?;
        let port = listener
            .local_addr()
            .map_err(|e| IdxError::Io(e.to_string()))?
            .port();
        listener
            .set_nonblocking(true)
            .map_err(|e| IdxError::Io(e.to_string()))?;
        Ok(Self {
            listener,
            port,
            secret: random_secret(),
        })
    }

    pub fn url(&self) -> String {
        format!("http://127.0.0.1:{}/login/{}", self.port, self.secret)
    }

    /// The port the page listens on.
    pub fn port(&self) -> u16 {
        self.port
    }

    /// Serves until `verify` accepts a paste or `timeout` passes. `verify`
    /// returns the error text to show on the page when the paste is refused.
    ///
    /// Each connection gets its own thread: browsers open idle preconnects,
    /// and answering them one at a time used to hold the page back for the
    /// read timeout on every one.
    pub fn wait<F>(&self, timeout: Duration, verify: F) -> Result<(), IdxError>
    where
        F: Fn(&str) -> Result<(), String> + Send + Sync + 'static,
    {
        let (done_tx, done_rx) = std::sync::mpsc::channel::<()>();
        let verify = std::sync::Arc::new(verify);
        let page = std::sync::Arc::new(self.page());
        let path = std::sync::Arc::new(format!("/login/{}", self.secret));
        let busy = std::sync::Arc::new(std::sync::atomic::AtomicUsize::new(0));
        let deadline = Instant::now() + timeout;
        loop {
            if done_rx.try_recv().is_ok() {
                return Ok(());
            }
            if Instant::now() >= deadline {
                break;
            }
            match self.listener.accept() {
                Ok((stream, _)) => {
                    // Browsers open a handful; anything beyond this is junk.
                    if busy.load(std::sync::atomic::Ordering::Relaxed) >= MAX_CONNECTIONS {
                        continue;
                    }
                    busy.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
                    let (verify, page, path) = (verify.clone(), page.clone(), path.clone());
                    let busy = busy.clone();
                    let done = done_tx.clone();
                    std::thread::spawn(move || {
                        if serve_once(stream, &path, &page, verify.as_ref()) {
                            let _ = done.send(());
                        }
                        busy.fetch_sub(1, std::sync::atomic::Ordering::Relaxed);
                    });
                }
                Err(e) if e.kind() == std::io::ErrorKind::WouldBlock => {
                    std::thread::sleep(Duration::from_millis(20));
                }
                Err(e) => return Err(IdxError::Io(format!("stockbit login page: {e}"))),
            }
        }
        Err(IdxError::Io(format!(
            "the Stockbit login page timed out after {}s; its URL was {} (re-run it, or pin the \
             port with --port)",
            timeout.as_secs(),
            self.url()
        )))
    }

    /// The page, with this run's port and secret baked into the bookmarklet.
    fn page(&self) -> String {
        let target = self.url();
        // Reads stockbit.com's session cookie and opens the page with it in
        // the fragment (never sent to a server, never in a referrer).
        let bookmarklet = format!(
            "javascript:(()=>{{const m=document.cookie.match(/(?:^|;\\s*)credentialStorage=([^;]*)/);\
if(!m){{alert('No Stockbit session in this browser: log in to stockbit.com first.');return}}\
window.open('{target}#t='+encodeURIComponent(m[1]))}})()"
        );
        PAGE.replace("__TARGET__", &target)
            .replace("__BOOKMARKLET__", &html_escape(&bookmarklet))
    }
}

/// Answers one request; true when a paste was accepted.
fn serve_once(
    mut stream: TcpStream,
    page_path: &str,
    page: &str,
    verify: &dyn Fn(&str) -> Result<(), String>,
) -> bool {
    let _ = stream.set_read_timeout(Some(IO_TIMEOUT));
    let _ = stream.set_write_timeout(Some(IO_TIMEOUT));
    let Some(request) = read_request(&mut stream) else {
        // An idle preconnect or a malformed request: drop it quietly.
        return false;
    };
    match (request.method.as_str(), request.path.as_str()) {
        ("GET", p) if p == page_path => {
            respond(
                &mut stream,
                200,
                "text/html; charset=utf-8",
                page.as_bytes(),
            );
        }
        ("POST", p) if p == page_path => match verify(&request.body) {
            Ok(()) => {
                respond(
                    &mut stream,
                    200,
                    "application/json",
                    br#"{"ok":true,"message":"Connected. You can close this tab."}"#,
                );
                return true;
            }
            Err(message) => {
                let body = serde_json::json!({ "ok": false, "message": message });
                respond(
                    &mut stream,
                    200,
                    "application/json",
                    body.to_string().as_bytes(),
                );
            }
        },
        ("GET", "/favicon.ico") => respond(&mut stream, 204, "text/plain", b""),
        _ => respond(&mut stream, 404, "text/plain", b"not found"),
    }
    false
}

fn random_secret() -> String {
    (0..4)
        .map(|_| format!("{:08x}", fastrand::u32(..)))
        .collect::<Vec<_>>()
        .join("")
}

fn html_escape(value: &str) -> String {
    value
        .replace('&', "&amp;")
        .replace('"', "&quot;")
        .replace('<', "&lt;")
        .replace('>', "&gt;")
}

struct Request {
    method: String,
    path: String,
    body: String,
}

/// A minimal HTTP/1.1 request: request line, headers, and a bounded body.
fn read_request(stream: &mut TcpStream) -> Option<Request> {
    let mut raw = Vec::new();
    let mut chunk = [0u8; 2048];
    let header_end = loop {
        let n = stream.read(&mut chunk).ok()?;
        if n == 0 {
            return None;
        }
        raw.extend_from_slice(&chunk[..n]);
        if let Some(at) = find(&raw, b"\r\n\r\n") {
            break at + 4;
        }
        if raw.len() > MAX_BODY {
            return None;
        }
    };
    let head = String::from_utf8_lossy(&raw[..header_end]).into_owned();
    let mut lines = head.lines();
    let mut request_line = lines.next()?.split_whitespace();
    let method = request_line.next()?.to_string();
    let path = request_line.next()?.split('?').next()?.to_string();
    let length = lines
        .filter_map(|line| line.split_once(':'))
        .find(|(name, _)| name.eq_ignore_ascii_case("content-length"))
        .and_then(|(_, value)| value.trim().parse::<usize>().ok())
        .unwrap_or(0)
        .min(MAX_BODY);
    let mut body = raw[header_end..].to_vec();
    while body.len() < length {
        let n = stream.read(&mut chunk).ok()?;
        if n == 0 {
            break;
        }
        body.extend_from_slice(&chunk[..n]);
    }
    body.truncate(length);
    Some(Request {
        method,
        path,
        body: String::from_utf8_lossy(&body).into_owned(),
    })
}

fn find(haystack: &[u8], needle: &[u8]) -> Option<usize> {
    haystack
        .windows(needle.len())
        .position(|window| window == needle)
}

fn respond(stream: &mut TcpStream, status: u16, content_type: &str, body: &[u8]) {
    let reason = match status {
        200 => "OK",
        204 => "No Content",
        400 => "Bad Request",
        _ => "Not Found",
    };
    let head = format!(
        "HTTP/1.1 {status} {reason}\r\nContent-Type: {content_type}\r\n\
         Content-Length: {}\r\nCache-Control: no-store\r\nReferrer-Policy: no-referrer\r\n\
         X-Content-Type-Options: nosniff\r\n\
         Content-Security-Policy: default-src 'none'; style-src 'unsafe-inline'; \
         script-src 'unsafe-inline'; connect-src 'self'\r\nConnection: close\r\n\r\n",
        body.len()
    );
    let _ = stream.write_all(head.as_bytes());
    let _ = stream.write_all(body);
    let _ = stream.flush();
}

const PAGE: &str = r#"<!doctype html>
<html lang="en"><head><meta charset="utf-8">
<meta name="viewport" content="width=device-width,initial-scale=1">
<title>Connect Stockbit to idx</title>
<style>
:root{--bg:#fff;--fg:#111;--muted:#666;--line:#e5e5e5;--accent:#111;--ok:#1a7f37;--err:#b42318;--card:#fafafa}
@media (prefers-color-scheme:dark){:root{--bg:#0d0d0d;--fg:#f2f2f2;--muted:#9a9a9a;--line:#2a2a2a;--accent:#f2f2f2;--card:#161616}}
*{box-sizing:border-box}
body{margin:0;background:var(--bg);color:var(--fg);font:15px/1.55 ui-sans-serif,system-ui,-apple-system,"Segoe UI",Roboto,sans-serif}
main{max-width:640px;margin:0 auto;padding:48px 24px 64px}
h1{font-size:22px;letter-spacing:-.01em;margin:0 0 6px}
p.lede{color:var(--muted);margin:0 0 28px}
h2{font-size:13px;text-transform:uppercase;letter-spacing:.08em;color:var(--muted);margin:28px 0 10px}
.step{border:1px solid var(--line);border-radius:12px;padding:18px;background:var(--card);margin-bottom:14px}
.step p{margin:0 0 12px}
.btn{display:inline-block;border:1px solid var(--accent);background:var(--accent);color:var(--bg);border-radius:9px;padding:9px 14px;font:inherit;font-weight:600;cursor:pointer;text-decoration:none}
.btn[disabled]{opacity:.5;cursor:default}
.btn.ghost{background:transparent;color:var(--fg);border-color:var(--line);font-weight:500}
code{font:13px ui-monospace,SFMono-Regular,Menlo,monospace;background:var(--card);border:1px solid var(--line);border-radius:6px;padding:1px 5px}
textarea{width:100%;min-height:76px;margin-top:10px;border:1px solid var(--line);border-radius:9px;background:var(--bg);color:var(--fg);padding:10px;font:13px ui-monospace,Menlo,monospace;resize:vertical}
#status{margin-top:18px;font-weight:600;min-height:1.4em}
#status.ok{color:var(--ok)}#status.err{color:var(--err)}
.foot{color:var(--muted);font-size:13px;margin-top:28px}
.foot a{color:inherit}
</style></head>
<body><main>
<h1>Connect Stockbit to idx</h1>
<p class="lede">One step. idx stores the session on this machine only, and never sends it anywhere but Stockbit.</p>

<div class="step">
<p><strong>1.</strong> Open <a href="https://stockbit.com/symbol/BBCA" target="_blank" rel="noreferrer">stockbit.com</a> and make sure you are logged in.</p>
</div>

<div class="step">
<p><strong>2.</strong> While on stockbit.com, click this bookmarklet. It reads your Stockbit session cookie in your own browser and hands it to this page.</p>
<p><a class="btn" href="__BOOKMARKLET__">Send my Stockbit session</a></p>
<p class="lede" style="margin:0">If your browser blocks it, drag the button to your bookmarks bar and click it there.</p>
</div>

<div class="step">
<p><strong>Or copy it by hand.</strong> On stockbit.com: DevTools, Application, Cookies, <code>https://stockbit.com</code>, copy the <code>credentialStorage</code> value, then paste it here.</p>
<textarea id="paste" placeholder="credentialStorage value"></textarea>
<div style="margin-top:10px;display:flex;gap:8px;flex-wrap:wrap">
<button class="btn" id="send">Connect</button>
<button class="btn ghost" id="clip">Paste from clipboard</button>
</div>
</div>

<p id="status"></p>
<p class="foot">idx keeps only the two tokens Stockbit issues, in a file only you can read. Stockbit replaces the refresh token each time it is used, so a browser sharing this session will be signed out once idx refreshes it. This page closes when you finish, or after a few minutes.</p>
</main>
<script>
const target = "__TARGET__";
const status = document.getElementById('status');
const setStatus = (text, kind) => { status.textContent = text; status.className = kind || ''; };
async function send(text) {
  if (!text || !text.trim()) { setStatus('Nothing to send yet.', 'err'); return; }
  setStatus('Checking with Stockbit…');
  try {
    const response = await fetch(target, { method: 'POST', headers: { 'Content-Type': 'text/plain' }, body: text });
    const result = await response.json();
    setStatus(result.message, result.ok ? 'ok' : 'err');
  } catch (error) { setStatus('Could not reach idx: ' + error, 'err'); }
}
document.getElementById('send').addEventListener('click', () => send(document.getElementById('paste').value));
document.getElementById('clip').addEventListener('click', async () => {
  try { send(await navigator.clipboard.readText()); }
  catch (error) { setStatus('Clipboard blocked (' + error + '). Paste into the box instead.', 'err'); }
});
if (location.hash.startsWith('#t=')) {
  const value = decodeURIComponent(location.hash.slice(3));
  history.replaceState(null, '', location.pathname);
  send(value);
}
</script>
</body></html>
"#;

#[cfg(test)]
mod tests {
    use std::io::{Read as _, Write as _};
    use std::net::TcpStream;

    use super::*;

    fn request(url: &str, method: &str, body: &str) -> (u16, String) {
        let rest = url.trim_start_matches("http://");
        let (authority, path) = rest.split_once('/').expect("path");
        let mut stream = TcpStream::connect(authority).expect("connect");
        // A server that never answers must fail the test, not hang it.
        stream
            .set_read_timeout(Some(Duration::from_secs(5)))
            .unwrap();
        write!(
            stream,
            "{method} /{path} HTTP/1.1\r\nHost: {authority}\r\nContent-Length: {}\r\n\r\n{body}",
            body.len()
        )
        .unwrap();
        let mut raw = String::new();
        stream.read_to_string(&mut raw).unwrap();
        let status = raw
            .split_whitespace()
            .nth(1)
            .and_then(|s| s.parse().ok())
            .expect("status");
        let body = raw.split("\r\n\r\n").nth(1).unwrap_or_default().to_string();
        (status, body)
    }

    #[test]
    fn serves_the_page_only_with_the_secret_and_accepts_a_good_paste() {
        let page = LoginPage::bind(0).unwrap();
        let url = page.url();
        let bad = url.replace(&page.secret, "not-the-secret");

        let handle = std::thread::spawn(move || {
            let verify = |paste: &str| match paste.trim() {
                "good" => Ok(()),
                _ => Err("not a Stockbit session".to_string()),
            };
            page.wait(Duration::from_secs(10), verify)
        });
        std::thread::sleep(Duration::from_millis(100));

        assert_eq!(request(&bad, "GET", "").0, 404);
        let (status, html) = request(&url, "GET", "");
        assert_eq!(status, 200);
        assert!(html.contains("Connect Stockbit to idx"));
        assert!(html.contains("javascript:"), "bookmarklet rendered");
        // The bookmarklet carries this run's page and a fragment, not the
        // secret in a query string.
        assert!(html.contains(&format!("{url}#t=")), "bookmarklet target");

        let (status, body) = request(&url, "POST", "nonsense");
        assert_eq!(status, 200);
        assert!(body.contains("\"ok\":false"), "{body}");

        let (status, body) = request(&url, "POST", "good");
        assert_eq!(status, 200);
        assert!(body.contains("\"ok\":true"), "{body}");
        handle.join().unwrap().expect("accepted");
    }

    #[test]
    fn times_out_without_a_paste() {
        let page = LoginPage::bind(0).unwrap();
        let started = Instant::now();
        let err = page
            .wait(Duration::from_millis(150), |_| Err("no".to_string()))
            .unwrap_err();
        assert!(err.to_string().contains("timed out"), "{err}");
        assert!(started.elapsed() < Duration::from_secs(5));
    }

    /// Browsers open idle preconnects; one must not delay the page.
    #[test]
    fn an_idle_connection_does_not_delay_the_page() {
        let page = LoginPage::bind(0).unwrap();
        let url = page.url();
        let handle = std::thread::spawn(move || {
            page.wait(Duration::from_secs(10), |_| {
                Err("not a Stockbit session".to_string())
            })
        });
        std::thread::sleep(Duration::from_millis(100));

        // Two sockets that say nothing, as Chrome's preconnects do.
        let rest = url.trim_start_matches("http://");
        let (authority, _) = rest.split_once('/').unwrap();
        let idle: Vec<TcpStream> = (0..2)
            .map(|_| TcpStream::connect(authority).unwrap())
            .collect();
        std::thread::sleep(Duration::from_millis(100));

        let started = Instant::now();
        let (status, _) = request(&url, "GET", "");
        assert_eq!(status, 200);
        assert!(
            started.elapsed() < Duration::from_secs(2),
            "page waited {:?} behind idle connections",
            started.elapsed()
        );
        drop(idle);
        drop(handle);
    }
}
