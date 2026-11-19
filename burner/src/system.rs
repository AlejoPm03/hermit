//! Getting the old system off the disk, and rebooting

use std::fs::{self, File, OpenOptions};
use std::io;
use std::os::unix::fs::OpenOptionsExt;
use std::path::Path;
use std::process::Command;
use std::thread::sleep;
use std::time::{Duration, Instant};

use anyhow::{Context, Result, bail, ensure};
use nix::fcntl::OFlag;
use nix::mount::{MntFlags, MsFlags, mount, umount2};
use nix::sys::reboot::RebootMode;
use nix::sys::signal::{Signal, kill};
use nix::sys::wait::{WaitPidFlag, WaitStatus, waitpid};
use nix::unistd::{Pid, pivot_root, sync};

use crate::config::DIR;
use crate::log::log;

const NONE: Option<&str> = None;

pub fn prepare() -> Result<()> {
    log!("preparing the system");

    // If PID 1 dies the kernel panics: reboot instead of hanging
    fs::write("/proc/sys/kernel/panic", "10").context("cannot set kernel.panic")?;

    // Small write cache, so the progress is realistic
    fs::write("/proc/sys/vm/dirty_background_bytes", "16777216")
        .context("cannot set vm.dirty_background_bytes")?;
    fs::write("/proc/sys/vm/dirty_bytes", "67108864").context("cannot set vm.dirty_bytes")?;

    // The swap file is on the disk
    let status = Command::new("swapoff").arg("-a").status().context("cannot run swapoff")?;
    ensure!(status.success(), "swapoff -a failed: {status}");

    sync();
    Ok(())
}

/// Mounts this binary over systemd's and makes systemd re-execute, so this program runs
/// again as PID 1. The trick comes from balena-os/takeover.
pub fn take_over() -> Result<()> {
    let init = fs::read_link("/proc/1/exe").context("cannot find the init binary")?;
    let me = std::env::current_exe().context("cannot find this binary")?;
    mount(Some(&me), &init, NONE, MsFlags::MS_BIND, NONE)
        .context("cannot replace the init binary")?;
    log!("handing over to PID 1, the SSH session will end");
    kill(Pid::from_raw(1), Signal::SIGTERM).context("cannot signal PID 1")
}

/// Kills every process, moves to a root in RAM and drops the old one, freeing the disk
pub fn leave_old_system(disk: &Path) -> Result<()> {
    // As PID 1 every orphan is our child to reap
    for signal in [Signal::SIGTERM, Signal::SIGKILL] {
        let _ = kill(Pid::from_raw(-1), signal);
        reap_children(Duration::from_secs(5));
    }

    // pivot_root refuses shared mounts
    let flags = MsFlags::MS_REC | MsFlags::MS_PRIVATE;
    mount(NONE, "/", NONE, flags, NONE).context("cannot make / private")?;
    let root = Path::new(DIR).join("root");
    fs::create_dir_all(&root)?;
    mount(Some("tmpfs"), &root, Some("tmpfs"), MsFlags::empty(), NONE)
        .context("cannot mount tmpfs")?;
    // Only /dev is needed: the disk and /dev/kmsg
    let dev = root.join("dev");
    fs::create_dir(&dev)?;
    let flags = MsFlags::MS_BIND | MsFlags::MS_REC;
    mount(Some("/dev"), &dev, NONE, flags, NONE).context("cannot bind /dev")?;

    // Stacks the old root on top of the new one, then drops it
    std::env::set_current_dir(&root)?;
    pivot_root(".", ".").context("pivot_root failed")?;
    umount2(".", MntFlags::MNT_DETACH).context("cannot drop the old root")?;
    std::env::set_current_dir("/")?;

    // Killed processes may still be exiting and holding the old filesystems
    for _ in 0..10 {
        if open_disk(disk).is_ok() {
            return Ok(());
        }
        reap_children(Duration::from_secs(1));
    }
    bail!("the disk is still in use")
}

/// Opens the disk for writing; `O_EXCL` fails while anything has it mounted
pub fn open_disk(disk: &Path) -> io::Result<File> {
    OpenOptions::new().write(true).custom_flags(OFlag::O_EXCL.bits()).open(disk)
}

pub fn reboot() -> ! {
    sync();
    let Err(error) = nix::sys::reboot::reboot(RebootMode::RB_AUTOBOOT);
    // PID 1 must never exit
    log!("rebooting failed: {error}");
    loop {
        sleep(Duration::from_secs(60));
    }
}

fn reap_children(timeout: Duration) {
    let deadline = Instant::now() + timeout;
    while Instant::now() < deadline {
        match waitpid(Pid::from_raw(-1), Some(WaitPidFlag::WNOHANG)) {
            Ok(WaitStatus::StillAlive) => sleep(Duration::from_millis(100)),
            Ok(_) => {}
            // No children left
            Err(_) => return,
        }
    }
}
