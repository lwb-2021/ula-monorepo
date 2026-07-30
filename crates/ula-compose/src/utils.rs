use asynchronous_codec::{FramedWrite, LinesCodec};
use serde::Serialize;

use std::io::{BufRead, BufReader, Read};
use std::process::Child;

pub struct ChildGuard(pub Child);

impl ChildGuard {
    pub fn kill(&mut self) {
        let _ = self.0.kill();
        let _ = self.0.wait();
    }
}
impl Drop for ChildGuard {
    fn drop(&mut self) {
        self.kill();
    }
}

pub fn drain_stderr<R: Read + Send + 'static>(reader: R, tag: &'static str) {
    std::thread::spawn(move || {
        for line in BufReader::new(reader).lines().map_while(Result::ok) {
            log::info!("[{tag}] {line}");
        }
    });
}

pub type LineSink<W> = FramedWrite<W, LinesCodec>;
pub fn json_line<T: Serialize>(message: &T) -> std::io::Result<String> {
    let json = serde_json::to_string(message)?;
    Ok(format!("{json}\n"))
}
