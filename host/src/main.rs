//! Flashes a Linux board's boot disk in place over SSH with a raw image.
//!
//! The burner runs on the board, takes it over and writes the image this PC sends.

mod flash;
mod ssh;

use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, Ordering};

use anyhow::Result;
use clap::Parser;
use ssh::Ssh;

#[derive(Parser)]
#[command(about)]
struct Args {
    /// The board to flash, as ssh takes it: host or user@host
    target: String,
    /// The raw .img image to write
    image: PathBuf,
    /// SSH password, sent with sshpass. Without it ssh uses keys
    #[arg(long, env = "SSHPASS")]
    password: Option<String>,
    /// The burner binary, instead of the built-in one for the board's CPU
    #[arg(long)]
    burner: Option<PathBuf>,
    /// Refuse boards without /etc/allow-reflash, and create it again on the new image
    #[arg(long)]
    require_marker: bool,
}

fn main() -> Result<()> {
    let args = Args::parse();
    let warned = AtomicBool::new(false);
    // Stopping while the burner flashes can leave the board unbootable
    ctrlc::set_handler(move || {
        if warned.swap(true, Ordering::SeqCst) {
            std::process::exit(130);
        }
        eprintln!("stopping can leave the board unbootable, press Ctrl+C again to stop anyway");
    })?;
    let ssh = Ssh::new(&args.target, args.password);
    flash::flash(&ssh, &args.image, args.burner.as_deref(), args.require_marker)?;
    println!("{} booted the new image", ssh.host());
    Ok(())
}
