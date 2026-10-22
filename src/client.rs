use std::fs;
use std::io::{BufRead, BufReader, Write};
use std::os::unix::fs::FileTypeExt;
use std::os::unix::net::UnixStream;

use anyhow::{Context, Result, bail};

use crate::paths::AppPaths;
use crate::protocol::{Request, Response};

pub fn request(paths: &AppPaths, request: &Request) -> Result<Response> {
    let mut stream = connect_and_send(paths, request)?;
    let mut line = String::new();
    BufReader::new(&mut stream)
        .read_line(&mut line)
        .context("failed to read daemon response")?;
    if line.is_empty() {
        bail!("timer daemon closed the connection without responding");
    }
    serde_json::from_str(&line).context("timer daemon returned an invalid response")
}

pub fn attach(paths: &AppPaths, id: Option<u64>) -> Result<UnixStream> {
    connect_and_send(paths, &Request::Attach { id })
}

fn connect_and_send(paths: &AppPaths, request: &Request) -> Result<UnixStream> {
    let mut stream = UnixStream::connect(&paths.socket)
        .with_context(|| format!("failed to connect to {}", paths.socket.display()))?;
    serde_json::to_writer(&mut stream, request).context("failed to encode daemon request")?;
    stream
        .write_all(b"\n")
        .context("failed to send daemon request")?;
    stream.flush().context("failed to flush daemon request")?;
    Ok(stream)
}

pub fn current_tty_path() -> Option<String> {
    for descriptor in [1, 0, 2] {
        let Ok(link) = fs::read_link(format!("/proc/self/fd/{descriptor}")) else {
            continue;
        };
        let Ok(metadata) = fs::metadata(&link) else {
            continue;
        };
        if metadata.file_type().is_char_device() {
            if let Some(path) = link.to_str() {
                if path.starts_with("/dev/pts/")
                    || path.starts_with("/dev/tty")
                    || path == "/dev/console"
                {
                    return Some(path.to_owned());
                }
            }
        }
    }
    None
}

pub fn expect_response(response: Response) -> Result<Response> {
    match response {
        Response::Error { message } => bail!(message),
        response => Ok(response),
    }
}
