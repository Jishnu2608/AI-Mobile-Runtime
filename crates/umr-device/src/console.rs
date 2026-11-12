//! The emulator's text console (`localhost:<console port>`), used for `avd snapshot save` and
//! `kill`. The console asks for an auth token and names the file that holds it in its banner,
//! so the client reads the path from there instead of guessing the emulator's home folder.

use std::io::{BufRead, BufReader, Write};
use std::net::{Ipv4Addr, SocketAddr, TcpStream};
use std::time::Duration;

#[derive(Debug)]
pub struct Console {
    reader: BufReader<TcpStream>,
    writer: TcpStream,
}

/// The token file named in the console banner (`... find your <auth_token> in '<path>'`).
#[must_use]
pub fn token_path(banner: &str) -> Option<String> {
    let at = banner.find("<auth_token> in")?;
    let rest = &banner[at..];
    let start = rest.find('\'')? + 1;
    let end = rest[start..].find('\'')? + start;
    Some(rest[start..end].trim().to_string())
}

impl Console {
    /// Connects and authenticates (if the console asks for it).
    pub fn connect(port: u16, timeout: Duration) -> Result<Self, String> {
        let addr = SocketAddr::from((Ipv4Addr::LOCALHOST, port));
        let s = TcpStream::connect_timeout(&addr, timeout)
            .map_err(|e| format!("emulator console {port}: {e}"))?;
        s.set_read_timeout(Some(timeout))
            .map_err(|e| e.to_string())?;
        let writer = s.try_clone().map_err(|e| e.to_string())?;
        let mut c = Self {
            reader: BufReader::new(s),
            writer,
        };
        let banner = c.read_reply()?;
        if banner.contains("Authentication required") {
            let path = token_path(&banner).ok_or("console banner names no token file")?;
            let token = std::fs::read_to_string(&path)
                .map_err(|e| format!("cannot read the console token {path}: {e}"))?;
            c.command(&format!("auth {}", token.trim()))?;
        }
        Ok(c)
    }

    /// Reads until a final `OK` or `KO: ...` line; `KO` becomes an error.
    fn read_reply(&mut self) -> Result<String, String> {
        let mut text = String::new();
        loop {
            let mut line = String::new();
            let n = self
                .reader
                .read_line(&mut line)
                .map_err(|e| format!("emulator console: {e}"))?;
            if n == 0 {
                return Ok(text); // closed (e.g. after `kill`)
            }
            let t = line.trim_end();
            if t == "OK" {
                return Ok(text);
            }
            if let Some(err) = t.strip_prefix("KO:") {
                return Err(format!("emulator console: {}", err.trim()));
            }
            text.push_str(t);
            text.push('\n');
        }
    }

    /// Sends one command and returns its output.
    pub fn command(&mut self, cmd: &str) -> Result<String, String> {
        self.writer
            .write_all(format!("{cmd}\r\n").as_bytes())
            .map_err(|e| format!("emulator console: {e}"))?;
        self.read_reply()
    }

    /// Changes the read timeout (snapshot saves take several seconds).
    pub fn set_timeout(&self, t: Duration) {
        // the reader's handle: on Windows a cloned socket keeps its own timeout
        let _ = self.reader.get_ref().set_read_timeout(Some(t));
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::net::TcpListener;

    #[test]
    fn reads_token_path_from_banner() {
        let b = "Android Console: Authentication required\nAndroid Console: type 'auth <auth_token>' to authenticate\nAndroid Console: you can find your <auth_token> in \n'C:\\Users\\u\\.emulator_console_auth_token'\n";
        assert_eq!(
            token_path(b).as_deref(),
            Some("C:\\Users\\u\\.emulator_console_auth_token")
        );
        assert_eq!(token_path("Android Console: type 'help'"), None);
    }

    #[test]
    fn authenticates_and_runs_commands() {
        let dir = std::env::temp_dir().join(format!("umr-console-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let tok = dir.join("token");
        std::fs::write(&tok, "s3cret\n").unwrap();
        let l = TcpListener::bind("127.0.0.1:0").unwrap();
        let port = l.local_addr().unwrap().port();
        let tok_s = tok.display().to_string();
        std::thread::spawn(move || {
            let (s, _) = l.accept().unwrap();
            let mut w = s.try_clone().unwrap();
            let mut r = BufReader::new(s);
            write!(w, "Android Console: Authentication required\r\nAndroid Console: you can find your <auth_token> in \r\n'{tok_s}'\r\nOK\r\n").unwrap();
            let mut authed = false;
            loop {
                let mut line = String::new();
                if r.read_line(&mut line).unwrap_or(0) == 0 {
                    return;
                }
                match line.trim() {
                    "auth s3cret" => {
                        authed = true;
                        write!(w, "Android Console: Authentication OK\r\nOK\r\n").unwrap();
                    }
                    "avd name" if authed => write!(w, "umr_standard_api36\r\nOK\r\n").unwrap(),
                    _ => write!(w, "KO: unknown command\r\n").unwrap(),
                }
            }
        });
        let mut c = Console::connect(port, Duration::from_secs(3)).unwrap();
        assert_eq!(c.command("avd name").unwrap().trim(), "umr_standard_api36");
        assert!(c.command("bogus").unwrap_err().contains("unknown command"));
        let _ = std::fs::remove_dir_all(&dir);
    }
}
