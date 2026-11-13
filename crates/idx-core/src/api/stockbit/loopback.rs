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
use std::sync::mpsc::Receiver;
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

    /// Serves until `verify` accepts a paste or `timeout` passes. The error
    /// `verify` returns for a refused paste is shown on the page.
    ///
    /// Each connection gets its own thread: browsers open idle preconnects,
    /// and answering them one at a time used to hold the page back for the
    /// read timeout on every one. Pastes are checked one at a time, and none
    /// after one is accepted: two at once (a double click, the bookmarklet
    /// and the clipboard button) would each store a session, and the later
    /// could overwrite the one Stockbit had just rotated.
    pub fn wait<F>(&self, timeout: Duration, verify: F) -> Result<(), IdxError>
    where
        F: Fn(&str) -> Result<(), IdxError> + Send + Sync + 'static,
    {
        self.wait_with(timeout, None, verify)
    }

    /// `wait`, also taking session values from `pastes`: the way in for a
    /// user whose browser cannot reach this page (a terminal on another
    /// machine with no port forward). They go through the same check as the
    /// page's, one at a time; a refused one is warned about and the wait
    /// goes on.
    pub fn wait_with<F>(
        &self,
        timeout: Duration,
        pastes: Option<&Receiver<String>>,
        verify: F,
    ) -> Result<(), IdxError>
    where
        F: Fn(&str) -> Result<(), IdxError> + Send + Sync + 'static,
    {
        let (done_tx, done_rx) = std::sync::mpsc::channel::<()>();
        let accepted = std::sync::Mutex::new(false);
        let verify = std::sync::Arc::new(move |paste: &str| {
            let mut accepted = accepted.lock().unwrap_or_else(|e| e.into_inner());
            if !*accepted {
                verify(paste)?;
                *accepted = true;
            }
            Ok(())
        });
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
            if let Some(paste) = pastes.and_then(|pastes| pastes.try_recv().ok()) {
                match (*verify)(&paste) {
                    Ok(()) => return Ok(()),
                    Err(err) => {
                        crate::runtime::warn(format!("{err}; paste it again, or use the page"))
                    }
                }
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

/// The forward to run where the browser is, for the page on `port`:
/// `ssh -L <port>:127.0.0.1:<port> <user>@<host>`.
///
/// The target comes from `SSH_CONNECTION` (`client_ip client_port server_ip
/// server_port`) when idx runs inside an SSH session, else from `USER` and
/// the host name, else it is left for the reader to fill in.
pub fn ssh_forward(port: u16) -> String {
    let target = ssh_target().unwrap_or_else(|| "<this host>".to_string());
    format!("ssh -L {port}:127.0.0.1:{port} {target}")
}

fn ssh_target() -> Option<String> {
    if let Ok(connection) = std::env::var("SSH_CONNECTION") {
        let fields: Vec<&str> = connection.split_whitespace().collect();
        if let Some(server) = fields.get(2) {
            return Some(match std::env::var("USER") {
                Ok(user) => format!("{user}@{server}"),
                Err(_) => (*server).to_string(),
            });
        }
    }
    let user = std::env::var("USER").ok()?;
    Some(format!("{user}@{}", host_name()?))
}

/// This machine's name. Shells set `HOSTNAME` without exporting it, so the
/// kernel's answer is read where there is one.
fn host_name() -> Option<String> {
    std::env::var("HOSTNAME")
        .ok()
        .or_else(|| std::fs::read_to_string("/proc/sys/kernel/hostname").ok())
        .map(|name| name.trim().to_string())
        .filter(|name| !name.is_empty())
}

/// Answers one request; true when a paste was accepted.
fn serve_once(
    mut stream: TcpStream,
    page_path: &str,
    page: &str,
    verify: &dyn Fn(&str) -> Result<(), IdxError>,
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
            Err(err) => {
                let body = serde_json::json!({ "ok": false, "message": err.to_string() });
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
<meta name="color-scheme" content="light dark">
<title>Connect Stockbit to idx</title>
<style>
:root{
color-scheme:light dark;
--bg:#f4f5f8;--glow:rgba(47,92,240,.10);--panel:#fff;--panel-2:#f7f8fb;
--ink:#12141a;--muted:#6a7180;--line:#e5e8ef;--line-strong:#d2d7e2;
--accent:#2f5cf0;--accent-ink:#fff;
--ok:#0e7a48;--ok-bg:#eaf7f0;--ok-line:#bfe6d1;
--err:#b42318;--err-bg:#fdf2f1;--err-line:#f0cecb;
--shadow:0 1px 2px rgba(16,24,40,.05),0 18px 40px -20px rgba(16,24,40,.25)
}
@media (prefers-color-scheme:dark){:root{
--bg:#0b0c0f;--glow:rgba(122,162,255,.09);--panel:#14161b;--panel-2:#191c22;
--ink:#edeef2;--muted:#99a0b0;--line:#252932;--line-strong:#333a46;
--accent:#7aa2ff;--accent-ink:#0b0c0f;
--ok:#5fd39a;--ok-bg:#0f1f19;--ok-line:#1e4135;
--err:#ff8f86;--err-bg:#231413;--err-line:#4a2523;
--shadow:0 1px 2px rgba(0,0,0,.5),0 18px 44px -20px rgba(0,0,0,.9)
}}
*{box-sizing:border-box}
body{margin:0;padding:clamp(20px,5vw,56px) 20px;background:var(--bg);
background-image:radial-gradient(900px 420px at 50% -180px,var(--glow),transparent 70%);
background-repeat:no-repeat;color:var(--ink);
font:15px/1.6 ui-sans-serif,system-ui,-apple-system,"Segoe UI",Roboto,"Helvetica Neue",sans-serif;
-webkit-font-smoothing:antialiased}
.card{max-width:640px;margin:0 auto;background:var(--panel);border:1px solid var(--line);
border-radius:20px;padding:clamp(24px,4.4vw,40px);box-shadow:var(--shadow)}
.brand{display:flex;align-items:center;gap:9px;font-size:13px;font-weight:600;color:var(--muted);margin-bottom:20px}
.mark{display:grid;place-items:center;width:28px;height:28px;border-radius:9px;background:var(--accent);color:var(--accent-ink)}
.mark svg{width:16px;height:16px}
.sep{opacity:.45}
h1{font-size:clamp(22px,3.2vw,26px);line-height:1.25;letter-spacing:-.02em;margin:0 0 8px;text-wrap:balance}
.lede{color:var(--muted);margin:0 0 26px}
.steps{list-style:none;margin:0;padding:0;display:grid;gap:12px}
.step{display:flex;gap:14px;border:1px solid var(--line);background:var(--panel-2);border-radius:14px;padding:16px 18px}
.num{flex:0 0 auto;display:grid;place-items:center;width:24px;height:24px;margin-top:2px;border-radius:50%;
border:1px solid var(--line-strong);background:var(--panel);color:var(--muted);font-size:12px;font-weight:700}
.step h2{font-size:15px;letter-spacing:-.01em;margin:0 0 4px}
.step p{margin:0 0 13px;color:var(--muted);font-size:14px}
.step p:last-child{margin-bottom:0}
.btn{display:inline-flex;align-items:center;gap:8px;border:1px solid transparent;border-radius:10px;
padding:10px 16px;font:inherit;font-size:14px;font-weight:600;line-height:1.2;cursor:pointer;
text-decoration:none;background:var(--accent);color:var(--accent-ink);
box-shadow:0 1px 2px rgba(16,24,40,.10);transition:filter .12s ease,transform .12s ease,background-color .12s ease}
.btn:hover{filter:brightness(1.07)}
.btn:active{transform:translateY(1px)}
.btn.ghost{background:var(--panel);color:var(--ink);border-color:var(--line-strong);box-shadow:none}
.btn.ghost:hover{background:var(--panel-2);filter:none}
.btn[disabled]{opacity:.55;cursor:progress;transform:none}
.hint{margin:13px 0 0;color:var(--muted);font-size:13px}
.manual{margin-top:14px;border:1px solid var(--line);border-radius:14px;background:var(--panel)}
summary{display:flex;align-items:center;justify-content:space-between;gap:12px;padding:14px 18px;
cursor:pointer;font-size:14px;font-weight:600;list-style:none}
summary::-webkit-details-marker{display:none}
summary svg{width:16px;height:16px;flex:0 0 auto;color:var(--muted);transition:transform .16s ease}
.manual[open] summary svg{transform:rotate(180deg)}
.manual-body{padding:0 18px 18px;border-top:1px solid var(--line)}
.mini{margin:15px 0;padding-left:19px;color:var(--muted);font-size:13.5px;line-height:1.75}
.mini li::marker{color:var(--line-strong)}
textarea{width:100%;min-height:84px;border:1px solid var(--line-strong);border-radius:10px;
background:var(--panel);color:var(--ink);padding:11px 12px;resize:vertical;
font:13px/1.5 ui-monospace,SFMono-Regular,Menlo,monospace}
textarea::placeholder{color:var(--muted);opacity:.75}
.row{display:flex;flex-wrap:wrap;gap:8px;margin-top:12px}
code{font:12.5px ui-monospace,SFMono-Regular,Menlo,monospace;background:var(--panel-2);
border:1px solid var(--line);border-radius:6px;padding:1px 5px;white-space:nowrap}
kbd{font:12px ui-monospace,Menlo,monospace;background:var(--panel-2);border:1px solid var(--line-strong);
border-bottom-width:2px;border-radius:5px;padding:0 5px}
em{font-style:normal;color:var(--ink)}
.status{display:flex;align-items:center;gap:10px;margin:18px 0 0;padding:12px 14px;
border:1px solid transparent;border-radius:11px;font-size:14px;font-weight:600}
.status[hidden]{display:none}
.status.pending{color:var(--muted);background:var(--panel-2);border-color:var(--line)}
.status.pending::before{content:"";flex:0 0 auto;width:14px;height:14px;border-radius:50%;
border:2px solid var(--line-strong);border-top-color:var(--accent);animation:spin .7s linear infinite}
.status.err{color:var(--err);background:var(--err-bg);border-color:var(--err-line)}
.status.err::before{content:"!";flex:0 0 auto;display:grid;place-items:center;width:16px;height:16px;
border-radius:50%;background:var(--err);color:var(--err-bg);font-size:11px;font-weight:800}
@keyframes spin{to{transform:rotate(360deg)}}
@media (prefers-reduced-motion:reduce){
.status.pending::before{animation-duration:2.4s}
.btn{transition:none}
}
.done{padding:6px 0 10px;text-align:center}
.done[hidden]{display:none}
.done .check{display:grid;place-items:center;width:52px;height:52px;margin:0 auto 16px;
border-radius:50%;background:var(--ok-bg);border:1px solid var(--ok-line);color:var(--ok)}
.done .check svg{width:26px;height:26px}
.done h2{font-size:18px;letter-spacing:-.01em;margin:0 0 6px}
.done p{margin:0;color:var(--muted);font-size:14px}
.foot{margin-top:26px;padding-top:18px;border-top:1px solid var(--line);
color:var(--muted);font-size:13px;line-height:1.65}
.foot p{margin:0 0 8px}
.foot p:last-child{margin-bottom:0}
:focus-visible{outline:2px solid var(--accent);outline-offset:2px;border-radius:6px}
</style></head>
<body><main class="card">
<div class="brand">
<span class="mark" aria-hidden="true"><svg viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round" stroke-linejoin="round"><path d="M9.7 14.3 14.3 9.7"/><path d="M7.4 12.6l-1.3 1.3a3.6 3.6 0 0 0 5 5l1.3-1.3"/><path d="M16.6 11.4l1.3-1.3a3.6 3.6 0 0 0-5-5L11.6 6.4"/></svg></span>
<span>idx</span><span class="sep" aria-hidden="true">/</span><span>Stockbit</span>
</div>

<header class="head">
<h1>Connect your Stockbit account</h1>
<p class="lede">idx then reads live quotes and the order book with your own session. The session stays on this machine.</p>
</header>

<section id="app">
<ol class="steps">
<li class="step">
<span class="num" aria-hidden="true">1</span>
<div>
<h2>Sign in to Stockbit</h2>
<p>Open stockbit.com in this browser and sign in. If the tab is already signed in, go to the next step.</p>
<a class="btn ghost" href="https://stockbit.com/symbol/BBCA" target="_blank" rel="noreferrer">Open stockbit.com</a>
</div>
</li>
<li class="step">
<span class="num" aria-hidden="true">2</span>
<div>
<h2>Send the session to idx</h2>
<p>While the Stockbit tab is open, click this button. It reads the session cookie in your own browser.</p>
<a class="btn" id="bookmarklet" href="__BOOKMARKLET__">Send my Stockbit session</a>
<p class="hint">If the browser blocks it, drag the button to your bookmarks bar, open stockbit.com, then click it there.</p>
</div>
</li>
</ol>

<details class="manual" id="manual">
<summary><span>Or paste the cookie by hand</span><svg viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round" stroke-linejoin="round" aria-hidden="true"><path d="m6 9 6 6 6-6"/></svg></summary>
<div class="manual-body">
<ol class="mini">
<li>On stockbit.com, open DevTools with <kbd>F12</kbd>.</li>
<li>Go to <em>Application</em>, then <em>Cookies</em>, then <code>https://stockbit.com</code>.</li>
<li>Copy the value of <code>credentialStorage</code> and paste it below.</li>
</ol>
<textarea id="paste" placeholder="credentialStorage value" spellcheck="false" autocomplete="off" autocapitalize="off"></textarea>
<div class="row">
<button class="btn" id="send">Connect</button>
<button class="btn ghost" id="clip">Paste from clipboard</button>
</div>
</div>
</details>

<p id="status" class="status" role="status" aria-live="polite" hidden></p>
</section>

<section id="done" class="done" hidden aria-live="polite">
<span class="check" aria-hidden="true"><svg viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2.4" stroke-linecap="round" stroke-linejoin="round"><path d="m20 6.5-10.5 11L4 12"/></svg></span>
<h2>Connected</h2>
<p id="done-msg">You can close this tab and go back to idx.</p>
</section>

<footer class="foot">
<p>idx keeps only the two tokens Stockbit issues, in a file only you can read. Stockbit replaces the refresh token each time it is used, so a browser that shares this session signs out once idx refreshes it.</p>
<p>This page stops listening after a few minutes.</p>
</footer>
</main>
<script>
const target = "__TARGET__";
const app = document.getElementById('app');
const done = document.getElementById('done');
const statusEl = document.getElementById('status');
const manual = document.getElementById('manual');
const paste = document.getElementById('paste');
const head = document.querySelector('.head');
const buttons = [document.getElementById('send'), document.getElementById('clip')];
let busy = false;

function setStatus(text, kind) {
  statusEl.hidden = !text;
  statusEl.className = 'status' + (kind ? ' ' + kind : '');
  statusEl.textContent = text || '';
}
function setBusy(value) {
  busy = value;
  for (const button of buttons) { button.disabled = value; }
}
function askForPaste(message) {
  if (message) { setStatus(message, 'err'); }
  manual.open = true;
  paste.focus();
}

async function send(value) {
  const text = (value || '').trim();
  if (busy) { return false; }
  if (!text) { askForPaste('Paste the credentialStorage value first.'); return false; }
  setBusy(true);
  setStatus('Checking the session with Stockbit\u2026', 'pending');
  try {
    const response = await fetch(target, {
      method: 'POST',
      headers: { 'Content-Type': 'text/plain' },
      body: text
    });
    const result = await response.json();
    if (result.ok) {
      setStatus('');
      head.hidden = true;
      app.hidden = true;
      done.hidden = false;
      return true;
    }
    setStatus(result.message, 'err');
  } catch (error) {
    setStatus('Could not reach idx: ' + error, 'err');
  } finally {
    setBusy(false);
  }
  return false;
}

document.getElementById('send').addEventListener('click', () => send(paste.value));
document.getElementById('clip').addEventListener('click', async () => {
  try {
    send(await navigator.clipboard.readText());
  } catch (error) {
    askForPaste('The browser blocked the clipboard. Paste the value into the box instead.');
  }
});
paste.addEventListener('keydown', (event) => {
  if (event.key === 'Enter' && !event.shiftKey) {
    event.preventDefault();
    send(paste.value);
  }
});
if (location.hash.startsWith('#t=')) {
  const value = decodeURIComponent(location.hash.slice(3));
  history.replaceState(null, '', location.pathname);
  send(value).then((ok) => {
    if (!ok) { paste.value = value; manual.open = true; }
  });
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

    /// Two pastes at once are checked one at a time, and the second is not
    /// checked at all once the first is accepted.
    #[test]
    fn concurrent_pastes_are_checked_once() {
        let page = LoginPage::bind(0).unwrap();
        let url = page.url();
        let checks = std::sync::Arc::new(std::sync::atomic::AtomicUsize::new(0));
        let counted = checks.clone();
        let handle = std::thread::spawn(move || {
            page.wait(Duration::from_secs(10), move |_| {
                counted.fetch_add(1, std::sync::atomic::Ordering::SeqCst);
                std::thread::sleep(Duration::from_millis(300));
                Ok(())
            })
        });
        std::thread::sleep(Duration::from_millis(100));
        let clients: Vec<_> = (0..2)
            .map(|_| {
                let url = url.clone();
                std::thread::spawn(move || request(&url, "POST", "good"))
            })
            .collect();
        for client in clients {
            let (status, body) = client.join().unwrap();
            assert_eq!(status, 200);
            assert!(body.contains("\"ok\":true"), "{body}");
        }
        handle.join().unwrap().unwrap();
        assert_eq!(checks.load(std::sync::atomic::Ordering::SeqCst), 1);
    }

    #[test]
    fn serves_the_page_only_with_the_secret_and_accepts_a_good_paste() {
        let page = LoginPage::bind(0).unwrap();
        let url = page.url();
        let bad = url.replace(&page.secret, "not-the-secret");

        let handle = std::thread::spawn(move || {
            let verify = |paste: &str| match paste.trim() {
                "good" => Ok(()),
                _ => Err(IdxError::InvalidInput("not a Stockbit session".to_string())),
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

    /// The CSP allows no fetch of its own, so a stray asset would fail
    /// silently; the page also has to keep the two entry points the docs name.
    #[test]
    fn the_page_is_self_contained_and_keeps_both_entry_points() {
        let page = LoginPage::bind(0).unwrap().page();
        for forbidden in ["<script src", "<link", "@import", "url(http"] {
            assert!(!page.contains(forbidden), "page needs {forbidden}");
        }
        assert_eq!(
            page.matches("https://").count(),
            2,
            "one link out, and the cookie path spelled out"
        );
        for url in page.split("https://").skip(1) {
            assert!(
                url.starts_with("stockbit.com"),
                "page links somewhere else: {}",
                &url[..url.len().min(40)]
            );
        }
        assert!(page.contains("https://stockbit.com/symbol/BBCA"));
        assert!(page.contains("Send my Stockbit session"));
        assert!(page.contains("Paste from clipboard"));
    }

    /// A value pasted in the terminal goes through the same check as the
    /// page's: a refused one keeps the wait going, a good one ends it.
    #[test]
    fn a_terminal_paste_ends_the_wait_like_the_page_does() {
        let page = LoginPage::bind(0).unwrap();
        let (tx, rx) = std::sync::mpsc::channel();
        tx.send("nonsense".to_string()).unwrap();
        tx.send("good".to_string()).unwrap();
        let checks = std::sync::Arc::new(std::sync::atomic::AtomicUsize::new(0));
        let counted = checks.clone();
        page.wait_with(Duration::from_secs(10), Some(&rx), move |paste| {
            counted.fetch_add(1, std::sync::atomic::Ordering::SeqCst);
            match paste {
                "good" => Ok(()),
                _ => Err(IdxError::InvalidInput("not a Stockbit session".to_string())),
            }
        })
        .expect("accepted");
        assert_eq!(checks.load(std::sync::atomic::Ordering::SeqCst), 2);
    }

    #[test]
    fn times_out_without_a_paste() {
        let page = LoginPage::bind(0).unwrap();
        let started = Instant::now();
        let err = page
            .wait(Duration::from_millis(150), |_| {
                Err(IdxError::InvalidInput("no".to_string()))
            })
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
                Err(IdxError::InvalidInput("not a Stockbit session".to_string()))
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
