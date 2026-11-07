//! Single-instance control over a per-user Unix socket.
//!
//! A second `perkolator` invocation forwards one line (`show` / `toggle`) to
//! the resident instance and exits. Access control is filesystem-based: the
//! socket is 0600 and its directory must be owned by this user and closed to
//! group/other (as `XDG_RUNTIME_DIR` is). Only two inert verbs are accepted.

use std::io::{self, BufRead, BufReader, Write};
use std::os::unix::fs::{MetadataExt, PermissionsExt};
use std::os::unix::net::{UnixListener, UnixStream};
use std::path::{Path, PathBuf};
use std::time::Duration;

use gpui_kit::{App, Entity};

use crate::launcher::LauncherController;

const CONNECT_TIMEOUT: Duration = Duration::from_millis(300);
const MAX_MESSAGE_BYTES: u64 = 64;

pub enum Forwarded {
    Delivered,
    NoInstance,
}

/// Try to hand the request to a running instance.
pub fn try_forward(socket: &Path, message: &str) -> io::Result<Forwarded> {
    let mut stream = match UnixStream::connect(socket) {
        Ok(stream) => stream,
        Err(error)
            if matches!(
                error.kind(),
                io::ErrorKind::NotFound | io::ErrorKind::ConnectionRefused
            ) =>
        {
            return Ok(Forwarded::NoInstance);
        }
        Err(error) => return Err(error),
    };
    stream.set_write_timeout(Some(CONNECT_TIMEOUT))?;
    stream.write_all(message.as_bytes())?;
    stream.write_all(b"\n")?;
    Ok(Forwarded::Delivered)
}

pub struct Listener {
    listener: UnixListener,
    path: PathBuf,
}

impl Listener {
    /// Bind the socket, replacing a stale socket file left by a crashed
    /// instance (only if nothing answers on it and it is a socket we own).
    pub fn bind(path: &Path) -> io::Result<Self> {
        let dir = path.parent().unwrap_or(Path::new("/"));
        let dir_meta = std::fs::metadata(dir)?;
        let own_uid = std::fs::metadata("/proc/self")?.uid();
        if dir_meta.uid() != own_uid || dir_meta.mode() & 0o077 != 0 {
            return Err(io::Error::new(
                io::ErrorKind::PermissionDenied,
                format!(
                    "{} must be owned by this user with mode 0700",
                    dir.display()
                ),
            ));
        }
        if path.exists() {
            let metadata = std::fs::symlink_metadata(path)?;
            use std::os::unix::fs::FileTypeExt;
            if !metadata.file_type().is_socket() {
                return Err(io::Error::new(
                    io::ErrorKind::AlreadyExists,
                    "path exists and is not a socket",
                ));
            }
            if UnixStream::connect(path).is_ok() {
                return Err(io::Error::new(
                    io::ErrorKind::AddrInUse,
                    "another instance is listening",
                ));
            }
            std::fs::remove_file(path)?;
        }
        let listener = UnixListener::bind(path)?;
        std::fs::set_permissions(path, std::fs::Permissions::from_mode(0o600))?;
        Ok(Self {
            listener,
            path: path.to_path_buf(),
        })
    }
}

impl Drop for Listener {
    fn drop(&mut self) {
        let _ = std::fs::remove_file(&self.path);
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Request {
    Show,
    Toggle,
}

pub fn parse_request(line: &str) -> Option<Request> {
    match line.trim() {
        "show" => Some(Request::Show),
        "toggle" => Some(Request::Toggle),
        _ => None,
    }
}

/// Accept connections on a background thread and apply requests on the UI
/// thread.
pub fn serve(listener: Listener, controller: Entity<LauncherController>, cx: &mut App) {
    let (sender, receiver) = async_channel::unbounded::<Request>();
    std::thread::Builder::new()
        .name("perkolator-ipc".into())
        .spawn(move || {
            for stream in listener.listener.incoming() {
                let Ok(stream) = stream else { continue };
                let mut line = String::new();
                let mut reader = BufReader::new(std::io::Read::take(&stream, MAX_MESSAGE_BYTES));
                if reader.read_line(&mut line).is_ok()
                    && let Some(request) = parse_request(&line)
                    && sender.send_blocking(request).is_err()
                {
                    break;
                }
            }
            drop(listener);
        })
        .expect("spawn IPC thread");

    cx.spawn(async move |cx| {
        while let Ok(request) = receiver.recv().await {
            controller.update(cx, |controller, cx| match request {
                Request::Show => controller.show(cx),
                Request::Toggle => controller.toggle(cx),
            });
        }
    })
    .detach();
}

#[cfg(test)]
mod tests {
    use super::*;

    fn private_dir(name: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!("{name}-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        std::fs::set_permissions(&dir, std::fs::Permissions::from_mode(0o700)).unwrap();
        dir
    }

    #[test]
    fn parses_known_requests_only() {
        assert_eq!(parse_request("toggle\n"), Some(Request::Toggle));
        assert_eq!(parse_request(" show "), Some(Request::Show));
        assert_eq!(parse_request("rm -rf /"), None);
    }

    #[test]
    fn forward_without_instance_reports_none() {
        let dir = std::env::temp_dir().join(format!("perk-ipc-test-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let socket = dir.join("none.sock");
        assert!(matches!(
            try_forward(&socket, "show").unwrap(),
            Forwarded::NoInstance
        ));
        std::fs::remove_dir_all(&dir).unwrap();
    }

    #[test]
    fn refuses_shared_directory() {
        let dir = private_dir("perk-ipc-shared");
        std::fs::set_permissions(&dir, std::fs::Permissions::from_mode(0o777)).unwrap();
        let error = Listener::bind(&dir.join("s.sock")).err().unwrap();
        assert_eq!(error.kind(), io::ErrorKind::PermissionDenied);
        std::fs::remove_dir_all(&dir).unwrap();
    }

    #[test]
    fn bind_then_forward_round_trip() {
        let dir = private_dir("perk-ipc-rt");
        let socket = dir.join("rt.sock");
        let listener = Listener::bind(&socket).unwrap();
        assert!(Listener::bind(&socket).is_err(), "second bind must fail");
        assert!(matches!(
            try_forward(&socket, "toggle").unwrap(),
            Forwarded::Delivered
        ));
        let (stream, _) = listener.listener.accept().unwrap();
        let mut line = String::new();
        BufReader::new(stream).read_line(&mut line).unwrap();
        assert_eq!(parse_request(&line), Some(Request::Toggle));
        drop(listener);
        assert!(!socket.exists(), "socket removed on drop");
        std::fs::remove_dir_all(&dir).unwrap();
    }
}
