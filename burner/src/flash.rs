use std::io::{self, ErrorKind, Read, Write};
use std::net::{TcpListener, TcpStream};
use std::sync::mpsc::{Receiver, sync_channel};
use std::thread::{self, sleep};
use std::time::{Duration, Instant};

use anyhow::{Context, Result, bail, ensure};
use flate2::read::DeflateDecoder;
use protocol::PORT;

use crate::MIB;
use crate::config::Config;
use crate::log::{self, log};
use crate::system::open_disk;

const CHUNK: usize = 4 << 20;
const NETWORK_TIMEOUT: Duration = Duration::from_secs(30);

pub fn listen() -> Result<TcpListener> {
    TcpListener::bind(("0.0.0.0", PORT)).with_context(|| format!("cannot listen on port {PORT}"))
}

/// Waits for the PC and sends it the log lines from then on
pub fn accept_within(listener: &TcpListener, timeout: Duration) -> Result<TcpStream> {
    listener.set_nonblocking(true)?;
    let deadline = Instant::now() + timeout;
    let pc = loop {
        match listener.accept() {
            Ok((pc, _)) => break pc,
            Err(error) if error.kind() != ErrorKind::WouldBlock => {
                return Err(error).context("cannot accept the PC");
            }
            Err(_) if Instant::now() > deadline => {
                bail!("the PC did not connect within {}s", timeout.as_secs())
            }
            Err(_) => sleep(Duration::from_millis(100)),
        }
    };
    listener.set_nonblocking(false)?;
    pc.set_nonblocking(false)?;
    log::connect(&pc);
    Ok(pc)
}

/// Retries until the image is written: past the point of no return there is no way back
pub fn flash(config: &Config, listener: &TcpListener, mut pc: TcpStream) {
    let mut attempt = 1;
    while let Err(error) = write(config, pc, attempt) {
        log!("{error:#}, waiting for the PC to send it again");
        log::disconnect();
        pc = loop {
            match accept_within(listener, NETWORK_TIMEOUT) {
                Ok(pc) => break pc,
                Err(error) => log!("{error:#}"),
            }
        };
        attempt += 1;
    }
}

fn write(config: &Config, mut pc: TcpStream, attempt: u32) -> Result<()> {
    pc.set_read_timeout(Some(NETWORK_TIMEOUT))?;
    let mut size = [0; 8];
    pc.read_exact(&mut size).context("no image size from the PC")?;
    let size = u64::from_be_bytes(size);
    ensure!(size > 0, "the image is empty");
    log!("attempt {attempt}: streaming {} MB to {}", size / MIB, config.disk.display());
    let mut disk = open_disk(&config.disk).context("cannot open the disk")?;
    let (mut written, mut last_log) = (0, Instant::now());
    // Decompressed as it arrives, so the image never has to fit in RAM
    for chunk in download(DeflateDecoder::new(pc).take(size)) {
        let chunk = chunk.context("download failed")?;
        disk.write_all(&chunk).context("write failed")?;
        written += chunk.len() as u64;
        if last_log.elapsed() >= Duration::from_secs(5) {
            log!("written {} / {} MB ({}%)", written / MIB, size / MIB, written * 100 / size);
            last_log = Instant::now();
        }
    }
    ensure!(written == size, "the stream ended at {} of {} MB", written / MIB, size / MIB);
    disk.sync_all().context("syncing the disk failed")?;
    log!("write done");
    Ok(())
}

/// Reads on its own thread, so the network and the disk work at the same time. Whole chunks
/// keep the writes page-aligned.
fn download(mut reader: impl Read + Send + 'static) -> Receiver<io::Result<Vec<u8>>> {
    let (sender, chunks) = sync_channel(4);
    thread::spawn(move || {
        loop {
            let mut chunk = Vec::with_capacity(CHUNK);
            match (&mut reader).take(CHUNK as u64).read_to_end(&mut chunk) {
                Ok(0) => return,
                Ok(_) => {
                    if sender.send(Ok(chunk)).is_err() {
                        return;
                    }
                }
                Err(error) => {
                    let _ = sender.send(Err(error));
                    return;
                }
            }
        }
    });
    chunks
}
