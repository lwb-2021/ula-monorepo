use std::io;
use std::process::{Command, Stdio};

use smol::net::unix::{UnixListener, UnixStream};

use crate::PATH;
use crate::utils::{ChildGuard, drain_stderr};

pub struct Interface {
    listener: UnixListener,
    path: String,
    _process: ChildGuard,
}

impl Interface {
    pub fn start(name: String) -> io::Result<Self> {
        let path = format!("/tmp/ula-{}.sock", std::process::id());
        drop(std::fs::remove_file(&path));
        let listener = UnixListener::bind(&path)?;

        let mut process = Command::new(&name)
            .arg(&path)
            .env("PATH", PATH.to_os_string())
            .stdin(Stdio::inherit())
            .stdout(Stdio::inherit())
            .stderr(Stdio::piped())
            .spawn()?;

        if let Some(stderr) = process.stderr.take() {
            drain_stderr(stderr, "ui");
        }

        Ok(Self {
            listener,
            path,
            _process: ChildGuard(process),
        })
    }

    pub async fn accept(&self) -> io::Result<UnixStream> {
        self.listener.accept().await.map(|(stream, _)| stream)
    }
}

impl Drop for Interface {
    fn drop(&mut self) {
        let _ = std::fs::remove_file(&self.path);
    }
}
