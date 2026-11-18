//! Flashes the target with the burner, then waits for it to boot the new image

use std::fs::{self, File};
use std::io::{self, BufRead, BufReader, Write};
use std::net::{Shutdown, TcpStream};
use std::path::{Path, PathBuf};
use std::sync::mpsc::{Receiver, RecvTimeoutError, channel};
use std::thread::{self, sleep};
use std::time::{Duration, Instant};

use anyhow::{Context, Result, anyhow, bail, ensure};
use flate2::Compression;
use flate2::read::DeflateEncoder;
use protocol::{Message, PORT};

use crate::ssh::Ssh;

const REMOTE_DIR: &str = "/run/flash";
/// With --require-marker, only targets with this file get flashed
pub const MARKER: &str = "/etc/allow-reflash";
/// The burner waits 30s for this PC after starting and after the takeover
const CONNECT_TIMEOUT: Duration = Duration::from_secs(30);
const SILENCE_TIMEOUT: Duration = Duration::from_secs(120);
const BOOT_TIMEOUT: Duration = Duration::from_secs(600);

pub fn flash(ssh: &Ssh, image: &Path, burner: Option<&Path>, require_marker: bool) -> Result<()> {
    // A compressed image would be written as is and not boot
    ensure!(
        image.extension().is_some_and(|ext| ext == "img"),
        "{} is not a raw .img image",
        image.display()
    );
    if require_marker {
        check_marker(ssh)?;
    }
    let burner = match burner {
        Some(burner) => burner.to_path_buf(),
        None => find_burner(&ssh.run("uname -m")?)?,
    };
    let old_boot = boot_id(ssh)?;

    let size =
        fs::metadata(image).with_context(|| format!("cannot read {}", image.display()))?.len();
    let disk = boot_disk(ssh)?;

    // In RAM, away from the disk it overwrites, and executable
    ssh.run(&format!(
        "mountpoint -q {REMOTE_DIR} || (sudo mkdir -p {REMOTE_DIR} && sudo mount -t tmpfs flash {REMOTE_DIR})"
    ))?;
    ssh.put(&burner, &format!("{REMOTE_DIR}/burner"))?;
    ssh.run(&format!("echo '{disk}' | sudo tee {REMOTE_DIR}/disk"))?;
    ssh.run(&format!("sudo chmod +x {REMOTE_DIR}/burner && sudo setsid -f {REMOTE_DIR}/burner </dev/null >/dev/null 2>&1"))?;
    // It hangs up when it hands over to PID 1
    follow(&read_lines(connect(ssh.host())?))?;
    send(ssh.host(), image, size)?;

    println!("waiting for {} to boot the new image", ssh.host());
    wait_new_boot(ssh, &old_boot)?;
    if require_marker {
        ssh.run(&format!("sudo touch {MARKER} && sync"))?;
    }
    Ok(())
}

fn check_marker(ssh: &Ssh) -> Result<()> {
    if ssh.run(&format!("test -e {MARKER} && echo yes || echo no"))? != "yes" {
        bail!("{} has no {MARKER}, create it to allow flashing it", ssh.host());
    }
    Ok(())
}

/// The burner cargo built for the target's CPU, as uname -m names it
fn find_burner(machine: &str) -> Result<PathBuf> {
    let triple = match machine {
        "armv7l" => "armv7-unknown-linux-musleabihf",
        "aarch64" => "aarch64-unknown-linux-musl",
        _ => bail!("no burner target for {machine}"),
    };
    // Cargo puts cross builds next to this binary's: target/<triple>/release
    let exe = std::env::current_exe().context("cannot find this binary")?;
    let target_dir =
        exe.parent().and_then(Path::parent).context("cannot find cargo's target directory")?;
    let burner = target_dir.join(triple).join("release/burner");
    ensure!(
        burner.exists(),
        "no burner built for {machine}, build it with: cargo build --release -p burner --target {triple}"
    );
    Ok(burner)
}

/// The disk the target runs from: SD card, USB or NVMe
fn boot_disk(ssh: &Ssh) -> Result<String> {
    Ok(format!("/dev/{}", ssh.run("lsblk -no PKNAME $(findmnt -no SOURCE /)")?))
}

fn boot_id(ssh: &Ssh) -> Result<String> {
    ssh.run("cat /proc/sys/kernel/random/boot_id")
}

/// Connects to the burner, retrying while it starts or takes over
fn connect(host: &str) -> Result<TcpStream> {
    let deadline = Instant::now() + CONNECT_TIMEOUT;
    loop {
        match TcpStream::connect((host, PORT)) {
            Ok(burner) => return Ok(burner),
            Err(error) if Instant::now() > deadline => {
                return Err(error)
                    .with_context(|| format!("cannot connect to the burner on {host}"));
            }
            Err(_) => sleep(Duration::from_secs(1)),
        }
    }
}

/// Sends the image to the burner while printing its lines, again when it hangs up before rebooting
fn send(host: &str, image: &Path, size: u64) -> Result<()> {
    loop {
        let mut burner = connect(host)?;
        let lines = read_lines(burner.try_clone()?);
        let image =
            File::open(image).with_context(|| format!("cannot open {}", image.display()))?;
        // The fastest level, so compressing doesn't hold the stream back
        let mut image = DeflateEncoder::new(image, Compression::fast());
        thread::spawn(move || {
            // Errors are ignored: when the burner hangs up, its log says why
            let _ = burner
                .write_all(&size.to_be_bytes())
                .and_then(|()| io::copy(&mut image, &mut burner))
                // Its decompressor only finishes the last block once it sees the end of the stream
                .and_then(|_| burner.shutdown(Shutdown::Write));
        });
        if follow(&lines)? {
            return Ok(());
        }
    }
}

/// The burner's log lines, until it hangs up
fn read_lines(burner: TcpStream) -> Receiver<String> {
    let (sender, lines) = channel();
    thread::spawn(move || {
        for line in BufReader::new(burner).lines() {
            let Ok(line) = line else { return };
            if sender.send(line).is_err() {
                return;
            }
        }
    });
    lines
}

/// Prints the burner's lines until it reboots the target (true) or hangs up (false); silence
/// means it is stuck
fn follow(lines: &Receiver<String>) -> Result<bool> {
    let mut last = String::from("nothing");
    loop {
        let line = match lines.recv_timeout(SILENCE_TIMEOUT) {
            Ok(line) => line,
            Err(RecvTimeoutError::Disconnected) => return Ok(false),
            Err(RecvTimeoutError::Timeout) => {
                bail!("no burner log for {}s, last: {last}", SILENCE_TIMEOUT.as_secs())
            }
        };
        match Message::parse(&line) {
            Some(Message::Log(text)) => {
                println!("{text}");
                last = text;
            }
            Some(Message::Rebooting) => return Ok(true),
            Some(Message::Aborted(reason)) => bail!("the burner aborted: {reason}"),
            None => bail!("unexpected line from the burner: {line}"),
        }
    }
}

fn wait_new_boot(ssh: &Ssh, old_boot: &str) -> Result<()> {
    let deadline = Instant::now() + BOOT_TIMEOUT;
    loop {
        // Fails while the target reboots
        let error = match boot_id(ssh) {
            Ok(boot) if boot != old_boot => return Ok(()),
            Ok(_) => anyhow!("still the old boot"),
            Err(error) => error,
        };
        if Instant::now() > deadline {
            bail!(
                "{} did not boot the new image after {}s: {error:#}",
                ssh.host(),
                BOOT_TIMEOUT.as_secs()
            );
        }
        sleep(Duration::from_secs(3));
    }
}
