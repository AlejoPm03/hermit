//! Log lines go to the kernel log and to the PC

use std::fs::OpenOptions;
use std::io::Write;
use std::net::TcpStream;
use std::sync::{Mutex, OnceLock};
use std::time::{Duration, Instant};

use protocol::Message;

macro_rules! log {
    ($($arg:tt)*) => { $crate::log::write(&format!($($arg)*)) };
}
pub(crate) use log;

struct Log {
    start: Instant,
    pc: Mutex<Pc>,
}

/// The connected PC, and the lines it has not got yet
#[derive(Default)]
struct Pc {
    stream: Option<TcpStream>,
    pending: String,
}

impl Pc {
    fn send(&mut self) {
        let Some(stream) = &mut self.stream else { return };
        if stream.write_all(self.pending.as_bytes()).is_ok() {
            self.pending.clear();
        } else {
            self.stream = None;
        }
    }
}

static LOG: OnceLock<Log> = OnceLock::new();

pub fn init() {
    let _ = LOG.set(Log { start: Instant::now(), pc: Mutex::default() });
}

/// Sends the lines so far to the PC, and the next ones as they come
pub fn connect(stream: &TcpStream) {
    let Some(log) = LOG.get() else { return };
    let Ok(stream) = stream.try_clone() else { return };
    // Short timeout, so a dead PC can't slow the flash
    let _ = stream.set_write_timeout(Some(Duration::from_secs(2)));
    if let Ok(mut pc) = log.pc.lock() {
        pc.stream = Some(stream);
        pc.send();
    }
}

/// Keeps the next lines for the next PC connection
pub fn disconnect() {
    if let Some(log) = LOG.get()
        && let Ok(mut pc) = log.pc.lock()
    {
        pc.stream = None;
    }
}

/// Never fails: losing a log line must not stop the flash
pub fn write(message: &str) {
    let Some(log) = LOG.get() else { return };
    let line = format!("flash [+{}s]: {message}", log.start.elapsed().as_secs());
    if let Ok(mut kmsg) = OpenOptions::new().write(true).open("/dev/kmsg") {
        let _ = kmsg.write_all(format!("{line}\n").as_bytes());
    }
    send(&Message::Log(line));
}

pub fn send(message: &Message) {
    let Some(log) = LOG.get() else { return };
    if let Ok(mut pc) = log.pc.lock() {
        pc.pending.push_str(&message.to_line());
        pc.send();
    }
}
