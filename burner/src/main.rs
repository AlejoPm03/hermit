//! Flashes the disk this machine runs from with a raw image the PC sends over TCP, then reboots
//! into it.
//!
//! The running system lives on the disk, so it first becomes PID 1, stops everything else and
//! unmounts the disk.

mod config;
mod flash;
mod log;
mod system;

use std::net::{TcpListener, TcpStream};
use std::time::Duration;

use anyhow::Result;
use config::Config;
use log::log;
use protocol::Message;

const MIB: u64 = 1 << 20;
/// The PC connects right after starting this program and after the takeover
const CONNECT_TIMEOUT: Duration = Duration::from_secs(30);

fn main() {
    log::init();
    if std::process::id() != 1 {
        // First stage: started over SSH, it makes systemd re-execute into this program
        if let Err(error) = run_over_ssh() {
            abort(error)
        }
    } else {
        // Second stage: as PID 1, it frees the disk and writes the image
        let (config, listener, pc) = free_the_disk().unwrap_or_else(|error| abort(error));
        flash::flash(&config, &listener, pc);
        log!("rebooting into the new image");
        log::send(&Message::Rebooting);
        system::reboot()
    }
}

fn run_over_ssh() -> Result<()> {
    log!("start");
    let config = Config::load()?;
    // Checks the PC can reach this port while the old system can still be kept
    let listener = flash::listen()?;
    flash::accept_within(&listener, CONNECT_TIMEOUT)?;
    log!("the PC is connected, disk {}", config.disk.display());
    system::prepare()?;
    system::take_over()
}

fn free_the_disk() -> Result<(Config, TcpListener, TcpStream)> {
    // Read before leaving the old root, where it lives
    let config = Config::load()?;
    // Before freeing the disk, so the PC gets its log lines and nothing is lost if it is gone
    let listener = flash::listen()?;
    let pc = flash::accept_within(&listener, CONNECT_TIMEOUT)?;
    log!("running as PID 1, stopping every process and unmounting the disk");
    system::leave_old_system(&config.disk)?;
    log!("the disk is free: point of no return");
    Ok((config, listener, pc))
}

/// Only before the first write. As PID 1 the way back to the old system is a reboot.
fn abort(error: anyhow::Error) -> ! {
    log!("{error:#}, aborting");
    log::send(&Message::Aborted(format!("{error:#}")));
    if std::process::id() == 1 {
        system::reboot();
    }
    std::process::exit(1)
}
