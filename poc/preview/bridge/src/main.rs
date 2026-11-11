//! POC-003 bridge (throwaway): adb + scrcpy-server v5.0.1 -> WebSocket.
//!
//! Env: ADB (path to adb.exe), SCRCPY_SERVER (path to server jar), WEB_DIR (static files).
//! HTTP  : 127.0.0.1:27200  (serves WEB_DIR/index.html)
//! WS    : 127.0.0.1:27201  (binary video out, binary control in, text results in)
//!
//! WS out messages:
//!   [0x01][w u32][h u32]                      session (size change)
//!   [0x02][flags u8: bit0 config, bit1 key][pts u64][payload]

use futures_util::{SinkExt, StreamExt};
use std::process::Stdio;
use std::sync::Arc;
use std::time::Duration;
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::{TcpListener, TcpStream};
use tokio::process::Command;
use tokio::sync::{broadcast, mpsc, Mutex};
use tokio_tungstenite::tungstenite::Message;

const SCRCPY_VERSION: &str = "5.0.1";
const FWD_PORT: u16 = 27183;

type Res<T> = Result<T, Box<dyn std::error::Error + Send + Sync>>;

async fn adb(adb: &str, args: &[&str]) -> Res<String> {
    let out = Command::new(adb).args(args).output().await?;
    Ok(String::from_utf8_lossy(&out.stdout).to_string() + &String::from_utf8_lossy(&out.stderr))
}

/// Forward tunnel: server accepts video then control, then sends a dummy byte on the first socket.
async fn connect_retry() -> Res<(TcpStream, TcpStream)> {
    for _ in 0..60 {
        if let Ok(mut v) = TcpStream::connect(("127.0.0.1", FWD_PORT)).await {
            if let Ok(c) = TcpStream::connect(("127.0.0.1", FWD_PORT)).await {
                let mut b = [0u8; 1];
                if let Ok(Ok(_)) = tokio::time::timeout(Duration::from_millis(1500), v.read_exact(&mut b)).await {
                    return Ok((v, c));
                }
            }
        }
        tokio::time::sleep(Duration::from_millis(200)).await;
    }
    Err("could not connect to scrcpy server".into())
}

#[tokio::main]
async fn main() -> Res<()> {
    let adb_path = std::env::var("ADB")?;
    let server = std::env::var("SCRCPY_SERVER")?;
    let web_dir = std::env::var("WEB_DIR")?;
    let max_size = std::env::var("MAX_SIZE").unwrap_or_else(|_| "1920".into());
    let bitrate = std::env::var("BITRATE").unwrap_or_else(|_| "8000000".into());

    println!("[bridge] push: {}", adb(&adb_path, &["push", &server, "/data/local/tmp/scrcpy-server.jar"]).await?.trim());
    let scid = "0badc0de";
    let sock = format!("localabstract:scrcpy_{scid}");
    adb(&adb_path, &["forward", "--remove", &format!("tcp:{FWD_PORT}")]).await.ok();
    println!("[bridge] forward: {}", adb(&adb_path, &["forward", &format!("tcp:{FWD_PORT}"), &sock]).await?.trim());
    // show touches -> visible feedback used by the latency meter (emulator setting only)
    adb(&adb_path, &["shell", "settings", "put", "system", "show_touches", "1"]).await.ok();

    let shell_cmd = format!(
        "CLASSPATH=/data/local/tmp/scrcpy-server.jar app_process / com.genymobile.scrcpy.Server {SCRCPY_VERSION} \
         scid={scid} log_level=info tunnel_forward=true audio=false control=true cleanup=true \
         video_codec=h264 max_size={max_size} video_bit_rate={bitrate} max_fps=60"
    );
    let mut server_proc = Command::new(&adb_path)
        .args(["shell", &shell_cmd])
        .stdout(Stdio::inherit())
        .stderr(Stdio::inherit())
        .kill_on_drop(true)
        .spawn()?;

    let (mut video, control) = connect_retry().await?;
    let mut name = [0u8; 64];
    video.read_exact(&mut name).await?;
    let name = String::from_utf8_lossy(&name).trim_end_matches('\0').to_string();
    let codec = video.read_u32().await?;
    println!("[bridge] device='{name}' codec=0x{codec:08x}");

    let (tx, _) = broadcast::channel::<Arc<Vec<u8>>>(256);
    let last_session: Arc<Mutex<Option<Arc<Vec<u8>>>>> = Arc::new(Mutex::new(None));
    let last_config: Arc<Mutex<Option<Arc<Vec<u8>>>>> = Arc::new(Mutex::new(None));
    let (ctl_tx, mut ctl_rx) = mpsc::channel::<Vec<u8>>(256);

    // control writer
    let (_ctl_r, mut ctl_w) = control.into_split();
    tokio::spawn(async move {
        while let Some(m) = ctl_rx.recv().await {
            if ctl_w.write_all(&m).await.is_err() { break; }
        }
    });

    // video reader
    {
        let tx = tx.clone();
        let ls = last_session.clone();
        let lc = last_config.clone();
        tokio::spawn(async move {
            let mut hdr = [0u8; 12];
            let mut n: u64 = 0;
            loop {
                if video.read_exact(&mut hdr).await.is_err() { println!("[bridge] video closed"); break; }
                if hdr[0] & 0x80 != 0 {
                    let w = u32::from_be_bytes(hdr[4..8].try_into().unwrap());
                    let h = u32::from_be_bytes(hdr[8..12].try_into().unwrap());
                    println!("[bridge] session {w}x{h}");
                    let mut m = vec![1u8];
                    m.extend_from_slice(&w.to_be_bytes());
                    m.extend_from_slice(&h.to_be_bytes());
                    let m = Arc::new(m);
                    *ls.lock().await = Some(m.clone());
                    let _ = tx.send(m);
                    continue;
                }
                let ptsf = u64::from_be_bytes(hdr[0..8].try_into().unwrap());
                let config = ptsf & (1 << 62) != 0;
                let key = ptsf & (1 << 61) != 0;
                let pts = ptsf & ((1u64 << 61) - 1);
                let len = u32::from_be_bytes(hdr[8..12].try_into().unwrap()) as usize;
                let mut m = Vec::with_capacity(10 + len);
                m.push(2u8);
                m.push((config as u8) | ((key as u8) << 1));
                m.extend_from_slice(&pts.to_be_bytes());
                m.resize(10 + len, 0);
                if video.read_exact(&mut m[10..]).await.is_err() { break; }
                let m = Arc::new(m);
                if config { *lc.lock().await = Some(m.clone()); }
                let _ = tx.send(m);
                n += 1;
                if n % 300 == 0 { println!("[bridge] packets={n}"); }
            }
        });
    }

    // HTTP static
    let web = Arc::new(web_dir);
    tokio::spawn(async move {
        let l = TcpListener::bind(("127.0.0.1", 27200)).await.unwrap();
        loop {
            let (mut s, _) = match l.accept().await { Ok(v) => v, Err(_) => continue };
            let web = web.clone();
            tokio::spawn(async move {
                let mut buf = [0u8; 2048];
                let _ = s.read(&mut buf).await;
                let body = tokio::fs::read(format!("{}/index.html", web)).await.unwrap_or_default();
                let head = format!("HTTP/1.1 200 OK\r\nContent-Type: text/html; charset=utf-8\r\nContent-Length: {}\r\nCache-Control: no-store\r\nConnection: close\r\n\r\n", body.len());
                let _ = s.write_all(head.as_bytes()).await;
                let _ = s.write_all(&body).await;
            });
        }
    });

    // WS
    let l = TcpListener::bind(("127.0.0.1", 27201)).await?;
    println!("[bridge] ready http://127.0.0.1:27200/  ws://127.0.0.1:27201/");
    loop {
        tokio::select! {
            _ = server_proc.wait() => { println!("[bridge] server exited"); break; }
            acc = l.accept() => {
                let (s, _) = acc?;
                let mut rx = tx.subscribe();
                let ls = last_session.clone();
                let lc = last_config.clone();
                let ctl_tx = ctl_tx.clone();
                tokio::spawn(async move {
                    let ws = match tokio_tungstenite::accept_async(s).await { Ok(w) => w, Err(_) => return };
                    let (mut wtx, mut wrx) = ws.split();
                    if let Some(m) = ls.lock().await.clone() { let _ = wtx.send(Message::Binary((*m).clone())).await; }
                    if let Some(m) = lc.lock().await.clone() { let _ = wtx.send(Message::Binary((*m).clone())).await; }
                    // ask for a fresh key frame for the new client (RESET_VIDEO)
                    let _ = ctl_tx.send(vec![17u8]).await;
                    let fwd = tokio::spawn(async move {
                        loop {
                            match rx.recv().await {
                                Ok(m) => { if wtx.send(Message::Binary((*m).clone())).await.is_err() { break; } }
                                Err(broadcast::error::RecvError::Lagged(k)) => println!("[bridge] ws lagged {k}"),
                                Err(_) => break,
                            }
                        }
                    });
                    while let Some(Ok(msg)) = wrx.next().await {
                        match msg {
                            Message::Binary(b) => { let _ = ctl_tx.send(b).await; }
                            Message::Text(t) => {
                                println!("[result] {t}");
                                if let Ok(mut f) = tokio::fs::OpenOptions::new().create(true).append(true).open("results.log").await {
                                    let _ = f.write_all(format!("{t}\n").as_bytes()).await;
                                }
                            }
                            _ => {}
                        }
                    }
                    fwd.abort();
                });
            }
        }
    }
    Ok(())
}
