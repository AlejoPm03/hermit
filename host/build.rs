//! Builds the burner for each board CPU, for hermit to carry inside its binary

use std::env;
use std::process::Command;

fn main() {
    let out_dir = env::var("OUT_DIR").expect("cargo sets OUT_DIR");
    let status = Command::new(env::var("CARGO").expect("cargo sets CARGO"))
        .args(["build", "--release", "-p", "burner"])
        .args(["--target", "armv7-unknown-linux-musleabihf"])
        .args(["--target", "aarch64-unknown-linux-musl"])
        // Its own target directory, as the one of this build is locked
        .arg("--target-dir")
        .arg(format!("{out_dir}/burner"))
        .status()
        .expect("cannot run cargo");
    assert!(status.success(), "cannot build the burner");
    println!("cargo:rerun-if-changed=../burner");
    println!("cargo:rerun-if-changed=../protocol");
    println!("cargo:rerun-if-changed=../Cargo.lock");
}
