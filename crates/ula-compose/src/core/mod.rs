use std::io;
use std::process::{Child, ChildStdin, ChildStdout, Command, Stdio};

use smol::Async;

use crate::PATH;
use crate::utils::{ChildGuard, drain_stderr};

pub struct Core {
    pub stdin: Async<ChildStdin>,
    pub stdout: Async<ChildStdout>,
    pub child: ChildGuard,
}

impl Core {
    pub fn spawn(model: &str) -> io::Result<Self> {
        let mut child: Child = Command::new("ula-core")
            .arg(model)
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .env("PATH", PATH.to_os_string())
            .spawn()?;

        if let Some(stderr) = child.stderr.take() {
            drain_stderr(stderr, "core");
        }

        let mut child = ChildGuard(child);
        let stdin = Async::new(child.0.stdin.take().expect("piped stdin"))?;
        let stdout = Async::new(child.0.stdout.take().expect("piped stdout"))?;

        Ok(Self {
            stdin,
            stdout,
            child,
        })
    }
}
