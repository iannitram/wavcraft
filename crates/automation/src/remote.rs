//! The remote backend: a running app, reached over its control channel
//! (`apps/wavcraft/src/control.rs`). The channel is loopback only and wants the token as its
//! first line; then one request per line, one answer per line.

use std::io::{self, BufRead, BufReader, Write};
use std::net::{Ipv4Addr, SocketAddr, TcpStream};
use std::path::Path;
use std::time::{Duration, Instant};

use crate::{Backend, Request};

/// How long an `OPEN` may take before the remote gives up waiting for `busy=0`.
const OPEN_TIMEOUT: Duration = Duration::from_secs(600);

/// A connection to a running app's control channel.
pub struct Remote {
    reader: BufReader<TcpStream>,
    writer: TcpStream,
}

impl Remote {
    /// Connect to `127.0.0.1:<port>` and present `token`. Fails when the app refuses the token.
    pub fn connect(port: u16, token: &str) -> io::Result<Remote> {
        let stream = TcpStream::connect(SocketAddr::from((Ipv4Addr::LOCALHOST, port)))?;
        let writer = stream.try_clone()?;
        let mut remote = Remote { reader: BufReader::new(stream), writer };
        let answer = remote.round_trip(&format!("TOKEN {token}"))?;
        if answer == "OK" { Ok(remote) } else { Err(io::Error::new(io::ErrorKind::PermissionDenied, answer)) }
    }

    /// Connect using the environment the app reads: `WAVCRAFT_CONTROL_PORT` and the token in the
    /// file named by `WAVCRAFT_CONTROL_TOKEN_FILE`.
    pub fn from_env() -> io::Result<Remote> {
        let missing = |what: &str| io::Error::new(io::ErrorKind::NotFound, format!("{what} is not set"));
        let port: u16 = std::env::var("WAVCRAFT_CONTROL_PORT")
            .map_err(|_| missing("WAVCRAFT_CONTROL_PORT"))?
            .trim()
            .parse()
            .map_err(|e| io::Error::new(io::ErrorKind::InvalidInput, format!("WAVCRAFT_CONTROL_PORT: {e}")))?;
        let token_file = std::env::var_os("WAVCRAFT_CONTROL_TOKEN_FILE").ok_or_else(|| missing("WAVCRAFT_CONTROL_TOKEN_FILE"))?;
        let token = read_token(Path::new(&token_file))?;
        Remote::connect(port, &token)
    }

    /// Send one line and read the answer line.
    fn round_trip(&mut self, line: &str) -> io::Result<String> {
        writeln!(self.writer, "{line}")?;
        let mut answer = String::new();
        if self.reader.read_line(&mut answer)? == 0 {
            return Err(io::Error::new(io::ErrorKind::UnexpectedEof, "the app closed the control channel"));
        }
        Ok(answer.trim_end().to_string())
    }

    /// `OPEN` runs in the background: poll `STATE` until it reports `busy=0`.
    fn open(&mut self, path: &Path) -> Result<(), String> {
        let answer = self.round_trip(&Request::Open(path.to_path_buf()).line()).map_err(|e| e.to_string())?;
        if let Some(why) = answer.strip_prefix("ERR ") {
            return Err(why.to_string());
        }
        let started = Instant::now();
        loop {
            let state = self.round_trip("STATE").map_err(|e| e.to_string())?;
            let body = state.strip_prefix("OK ").ok_or_else(|| state.clone())?;
            if body.split_whitespace().any(|f| f == "busy=0") {
                return Ok(());
            }
            if started.elapsed() > OPEN_TIMEOUT {
                return Err("the open did not finish in time".to_string());
            }
            std::thread::sleep(Duration::from_millis(20));
        }
    }
}

/// The token in `path`: its first line, trimmed (the app wrote it and checks its owner-only mode).
fn read_token(path: &Path) -> io::Result<String> {
    let text = std::fs::read_to_string(path)?;
    Ok(text.trim().to_string())
}

impl Backend for Remote {
    fn execute(&mut self, req: &Request) -> Result<String, String> {
        if let Request::Open(path) = req {
            return self.open(path).map(|()| String::new());
        }
        let answer = self.round_trip(&req.line()).map_err(|e| e.to_string())?;
        if let Some(text) = answer.strip_prefix("OK") {
            Ok(text.trim_start().to_string())
        } else {
            Err(answer.strip_prefix("ERR ").unwrap_or(&answer).to_string())
        }
    }
}
