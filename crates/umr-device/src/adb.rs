//! Minimal adb host-protocol client (the protocol `adb.exe` itself speaks to the server).
//!
//! Request: 4 hex digits of length + the command, e.g. `000chost:version`.
//! Reply: `OKAY` or `FAIL` + 4-hex length + message. Commands with data reply with a
//! length-prefixed payload; `host:track-devices-l` keeps sending such payloads.

use std::io::{self, Read, Write};
use std::net::{Ipv4Addr, SocketAddr, TcpStream};
use std::time::Duration;

use serde::{Deserialize, Serialize};

/// One device as adb lists it.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Device {
    /// `emulator-5554`, a USB serial, or `host:port` for network devices.
    pub serial: String,
    /// `device`, `offline`, `unauthorized`, `authorizing`, `recovery`, …
    pub state: String,
    pub product: Option<String>,
    pub model: Option<String>,
    pub device: Option<String>,
    pub transport_id: Option<u64>,
}

impl Device {
    #[must_use]
    pub fn is_emulator(&self) -> bool {
        self.serial.starts_with("emulator-")
    }

    #[must_use]
    pub fn is_ready(&self) -> bool {
        self.state == "device"
    }
}

/// Parses `host:devices-l` output (one device per line).
#[must_use]
pub fn parse_devices(text: &str) -> Vec<Device> {
    text.lines()
        .filter_map(|line| {
            let mut parts = line.split_whitespace();
            let serial = parts.next()?.to_string();
            let state = parts.next()?.to_string();
            let mut d = Device {
                serial,
                state,
                product: None,
                model: None,
                device: None,
                transport_id: None,
            };
            for kv in parts {
                match kv.split_once(':') {
                    Some(("product", v)) => d.product = Some(v.into()),
                    Some(("model", v)) => d.model = Some(v.into()),
                    Some(("device", v)) => d.device = Some(v.into()),
                    Some(("transport_id", v)) => d.transport_id = v.parse().ok(),
                    _ => {}
                }
            }
            Some(d)
        })
        .collect()
}

#[derive(Debug)]
pub enum AdbError {
    /// Nothing listens on the port.
    NotRunning,
    /// Something answered that is not an adb server.
    NotAdb(String),
    /// The server refused the command.
    Fail(String),
    Io(io::Error),
}

impl std::fmt::Display for AdbError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::NotRunning => write!(f, "no adb server is running"),
            Self::NotAdb(e) => write!(f, "not an adb server: {e}"),
            Self::Fail(e) => write!(f, "adb: {e}"),
            Self::Io(e) => write!(f, "adb connection: {e}"),
        }
    }
}

impl std::error::Error for AdbError {}

impl From<io::Error> for AdbError {
    fn from(e: io::Error) -> Self {
        if matches!(e.kind(), io::ErrorKind::ConnectionRefused) {
            Self::NotRunning
        } else {
            Self::Io(e)
        }
    }
}

/// One connection to an adb server (each request needs its own connection).
#[derive(Debug)]
pub struct Connection {
    stream: TcpStream,
}

impl Connection {
    pub fn open(port: u16, timeout: Duration) -> Result<Self, AdbError> {
        let addr = SocketAddr::from((Ipv4Addr::LOCALHOST, port));
        // On Windows a refused localhost connection is retried for ~2 s and may end as a
        // timeout instead of "refused": both mean nothing usable listens there.
        let stream = TcpStream::connect_timeout(&addr, timeout).map_err(|e| match e.kind() {
            io::ErrorKind::ConnectionRefused | io::ErrorKind::TimedOut => AdbError::NotRunning,
            _ => AdbError::Io(e),
        })?;
        stream.set_read_timeout(Some(timeout))?;
        stream.set_write_timeout(Some(timeout))?;
        Ok(Self { stream })
    }

    /// Sends a host request and reads `OKAY` / `FAIL`.
    pub fn request(&mut self, cmd: &str) -> Result<(), AdbError> {
        let msg = format!("{:04x}{cmd}", cmd.len());
        self.stream.write_all(msg.as_bytes())?;
        let mut status = [0u8; 4];
        self.stream
            .read_exact(&mut status)
            .map_err(|e| match e.kind() {
                io::ErrorKind::UnexpectedEof => AdbError::NotAdb("connection closed".into()),
                _ => AdbError::from(e),
            })?;
        match &status {
            b"OKAY" => Ok(()),
            b"FAIL" => Err(AdbError::Fail(self.read_payload().unwrap_or_default())),
            other => Err(AdbError::NotAdb(format!(
                "unexpected reply {:?}",
                String::from_utf8_lossy(other)
            ))),
        }
    }

    /// Reads one 4-hex-length-prefixed payload.
    pub fn read_payload(&mut self) -> Result<String, AdbError> {
        let mut len = [0u8; 4];
        self.stream.read_exact(&mut len)?;
        let n = usize::from_str_radix(
            std::str::from_utf8(&len).map_err(|_| AdbError::NotAdb("bad length".into()))?,
            16,
        )
        .map_err(|_| AdbError::NotAdb("bad length".into()))?;
        let mut buf = vec![0u8; n];
        self.stream.read_exact(&mut buf)?;
        Ok(String::from_utf8_lossy(&buf).into_owned())
    }

    /// Removes the read timeout (for long-lived tracking connections).
    pub fn wait_forever(&self) -> Result<(), AdbError> {
        self.stream.set_read_timeout(None)?;
        Ok(())
    }
}

/// `host:version` → the server's protocol version (41 for current platform-tools).
pub fn server_version(port: u16) -> Result<u32, AdbError> {
    let mut c = Connection::open(port, Duration::from_secs(2))?;
    c.request("host:version")?;
    let hex = c.read_payload()?;
    u32::from_str_radix(hex.trim(), 16)
        .map_err(|_| AdbError::NotAdb(format!("bad version '{hex}'")))
}

/// `host:devices-l`.
pub fn devices(port: u16) -> Result<Vec<Device>, AdbError> {
    let mut c = Connection::open(port, Duration::from_secs(3))?;
    c.request("host:devices-l")?;
    Ok(parse_devices(&c.read_payload()?))
}

/// Runs `cmd` in the device's shell (`host:transport:<serial>` then `shell:<cmd>`) and returns
/// its raw output (binary-safe, e.g. raw `screencap`).
pub fn shell_bytes(
    port: u16,
    serial: &str,
    cmd: &str,
    timeout: Duration,
) -> Result<Vec<u8>, AdbError> {
    let mut c = Connection::open(port, timeout)?;
    c.request(&format!("host:transport:{serial}"))?;
    c.request(&format!("shell:{cmd}"))?;
    let mut out = Vec::new();
    c.stream.read_to_end(&mut out)?;
    Ok(out)
}

/// [`shell_bytes`] as trimmed text.
pub fn shell(port: u16, serial: &str, cmd: &str) -> Result<String, AdbError> {
    let out = shell_bytes(port, serial, cmd, Duration::from_secs(10))?;
    Ok(String::from_utf8_lossy(&out).trim().to_string())
}

/// `host:kill` — stops the server on `port`. Refuses the public port (Android Studio's).
pub fn kill_server(port: u16) -> Result<(), AdbError> {
    if port == crate::PUBLIC_ADB_PORT {
        return Err(AdbError::Fail("refusing to stop the adb server on port 5037 (it belongs to the user / Android Studio)".into()));
    }
    let mut c = Connection::open(port, Duration::from_secs(2))?;
    c.request("host:kill")
}

#[cfg(test)]
pub(crate) mod fake {
    //! A fake adb server for tests.
    use std::io::{Read, Write};
    use std::net::TcpListener;
    use std::sync::atomic::{AtomicBool, Ordering};
    use std::sync::{Arc, Mutex};

    pub struct FakeAdb {
        pub port: u16,
        pub devices: Arc<Mutex<String>>,
        pub killed: Arc<AtomicBool>,
        /// `shell:<cmd>` → output (after `host:transport:<serial>`).
        pub shell: Arc<Mutex<std::collections::HashMap<String, Vec<u8>>>>,
    }

    fn read_cmd(s: &mut std::net::TcpStream) -> Option<String> {
        let mut len = [0u8; 4];
        s.read_exact(&mut len).ok()?;
        let n = usize::from_str_radix(std::str::from_utf8(&len).ok()?, 16).ok()?;
        let mut cmd = vec![0u8; n];
        s.read_exact(&mut cmd).ok()?;
        String::from_utf8(cmd).ok()
    }

    fn frame(s: &str) -> String {
        format!("{:04x}{s}", s.len())
    }

    impl FakeAdb {
        pub fn start(version: u32, devices: &str) -> Self {
            let l = TcpListener::bind("127.0.0.1:0").unwrap();
            let port = l.local_addr().unwrap().port();
            let devs = Arc::new(Mutex::new(devices.to_string()));
            let killed = Arc::new(AtomicBool::new(false));
            let shell: Arc<Mutex<std::collections::HashMap<String, Vec<u8>>>> = Arc::default();
            let (d2, k2, sh2) = (devs.clone(), killed.clone(), shell.clone());
            std::thread::spawn(move || {
                for s in l.incoming().flatten() {
                    if k2.load(Ordering::SeqCst) {
                        return; // a killed server stops accepting
                    }
                    let (d3, k3, sh3) = (d2.clone(), k2.clone(), sh2.clone());
                    std::thread::spawn(move || {
                        let mut s = s;
                        let Some(mut cmd) = read_cmd(&mut s) else {
                            return;
                        };
                        if cmd.starts_with("host:transport:") {
                            let _ = s.write_all(b"OKAY");
                            let Some(next) = read_cmd(&mut s) else {
                                return;
                            };
                            cmd = next;
                        }
                        if let Some(c) = cmd.strip_prefix("shell:") {
                            let out = sh3.lock().unwrap().get(c).cloned();
                            match out {
                                Some(o) => {
                                    let _ = s.write_all(b"OKAY");
                                    let _ = s.write_all(&o);
                                }
                                None => {
                                    let _ = s.write_all(
                                        format!("FAIL{}", frame("no such shell command"))
                                            .as_bytes(),
                                    );
                                }
                            }
                            return;
                        }
                        match cmd.as_str() {
                            "host:version" => {
                                let _ = s.write_all(
                                    format!("OKAY{}", frame(&format!("{version:04x}"))).as_bytes(),
                                );
                            }
                            "host:devices-l" => {
                                let list = d3.lock().unwrap().clone();
                                let _ = s.write_all(format!("OKAY{}", frame(&list)).as_bytes());
                            }
                            "host:track-devices-l" => {
                                let _ = s.write_all(b"OKAY");
                                let mut last: Option<String> = None; // real adb sends the current list first
                                loop {
                                    let cur = d3.lock().unwrap().clone();
                                    if last.as_ref() != Some(&cur) {
                                        if s.write_all(frame(&cur).as_bytes()).is_err() {
                                            return;
                                        }
                                        last = Some(cur);
                                    }
                                    if k3.load(Ordering::SeqCst) {
                                        return; // closes the tracking connection
                                    }
                                    std::thread::sleep(std::time::Duration::from_millis(20));
                                }
                            }
                            "host:kill" => {
                                let _ = s.write_all(b"OKAY");
                                k3.store(true, Ordering::SeqCst);
                            }
                            _ => {
                                let _ = s.write_all(
                                    format!("FAIL{}", frame("unknown host service")).as_bytes(),
                                );
                            }
                        }
                    });
                }
            });
            Self {
                port,
                devices: devs,
                killed,
                shell,
            }
        }

        pub fn set_shell(&self, cmd: &str, out: &[u8]) {
            self.shell
                .lock()
                .unwrap()
                .insert(cmd.to_string(), out.to_vec());
        }

        /// The fake sets `killed` after replying to `host:kill`: wait for it.
        pub fn wait_killed(&self) -> bool {
            let start = std::time::Instant::now();
            while !self.killed.load(Ordering::SeqCst)
                && start.elapsed() < std::time::Duration::from_secs(2)
            {
                std::thread::sleep(std::time::Duration::from_millis(10));
            }
            self.killed.load(Ordering::SeqCst)
        }
    }
}

#[cfg(test)]
mod tests {
    use super::fake::FakeAdb;
    use super::*;

    #[test]
    fn parses_devices_l_output() {
        let text = "emulator-5554          device product:sdk_gphone64_x86_64 model:sdk_gphone64_x86_64 device:emu64xa transport_id:1\nR58M123ABC             unauthorized usb:1-1 transport_id:2\n\n";
        let d = parse_devices(text);
        assert_eq!(d.len(), 2);
        assert!(d[0].is_emulator() && d[0].is_ready());
        assert_eq!(d[0].model.as_deref(), Some("sdk_gphone64_x86_64"));
        assert_eq!(d[0].transport_id, Some(1));
        assert!(!d[1].is_emulator() && !d[1].is_ready());
        assert_eq!(d[1].state, "unauthorized");
        assert_eq!(parse_devices(""), Vec::new());
    }

    #[test]
    fn talks_to_an_adb_server() {
        let f = FakeAdb::start(
            41,
            "emulator-5554\tdevice product:p model:m device:d transport_id:3\n",
        );
        assert_eq!(server_version(f.port).unwrap(), 41);
        let d = devices(f.port).unwrap();
        assert_eq!(
            (d[0].serial.as_str(), d[0].transport_id),
            ("emulator-5554", Some(3))
        );
        kill_server(f.port).unwrap();
        assert!(f.wait_killed());
    }

    #[test]
    fn errors_are_classified() {
        // port 1: nothing listens there on Windows (a freed random port could be reused by a
        // test running in parallel)
        assert!(matches!(server_version(1), Err(AdbError::NotRunning)));
        // something else listens and closes
        let other = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
        let p2 = other.local_addr().unwrap().port();
        std::thread::spawn(move || {
            for s in other.incoming().flatten() {
                let mut s = s;
                let _ = s.write_all(b"HTTP/1.1 400 Bad Request\r\n\r\n");
            }
        });
        assert!(matches!(server_version(p2), Err(AdbError::NotAdb(_))));
        assert!(
            matches!(kill_server(crate::PUBLIC_ADB_PORT), Err(AdbError::Fail(_))),
            "never kills port 5037"
        );
    }

    #[test]
    fn shell_runs_on_one_device() {
        let f = FakeAdb::start(41, "");
        f.set_shell(
            "getprop sys.boot_completed",
            b"1
",
        );
        f.set_shell("screencap", &[0, 1, 2, 255]);
        assert_eq!(
            shell(f.port, "emulator-5554", "getprop sys.boot_completed").unwrap(),
            "1"
        );
        assert_eq!(
            shell_bytes(f.port, "emulator-5554", "screencap", Duration::from_secs(2)).unwrap(),
            [0, 1, 2, 255]
        );
        assert!(matches!(
            shell(f.port, "emulator-5554", "reboot"),
            Err(AdbError::Fail(_))
        ));
    }
}
