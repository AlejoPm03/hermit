<p align="center"><img src="assets/logo.png" width="200" alt="Hermit"></p>

# Hermit

Hermit flashes an image onto a running Linux machine with systemd, with a focus on Raspberry Pis.

It writes the SD card, or whatever disk the machine boots from, over SSH and TCP, so you never have
to take the card out of the device.

It was inspired by balena's [takeover](https://github.com/balena-os/takeover), but it's simpler and
can write any image you give it.

## How it works

1. Hermit connects to the machine over SSH, uploads a small burner into RAM and starts it.
2. The burner replaces systemd as PID 1, stops every other process and moves to a root in RAM,
   which frees the disk.
3. Hermit streams the image to the burner over TCP, compressed. The burner decompresses it straight
   onto the disk, reboots into the new system, and Hermit waits until it's up.

Nothing is written until the disk is free. If anything fails before that, the machine stays on its
old system.

## Requirements

On the machine to flash:

- systemd
- SSH access with `sudo` that doesn't ask for a password, as on Raspberry Pi OS
- TCP port 8099 reachable from your computer
- A wired network: Hermit stops every process, including the one that keeps Wi-Fi connected

On your computer: Rust, `ssh`, and `sshpass` if you log in with a password.

## How to use

Build Hermit:

```sh
cargo build --release
```

Flash a raw `.img` image. Unpack `.xz` or `.zip` images first.

```sh
target/release/hermit pi@192.168.2.2 image.img
```

Without SSH keys, give the password in `SSHPASS`:

```sh
SSHPASS=raspberry target/release/hermit pi@192.168.2.2 image.img
```

Other options:

- `--require-marker` only flashes machines that have `/etc/allow-reflash`, and creates it again on
  the new system. Useful for test labs, so the wrong machine is never flashed.
- `--burner <path>` uses that burner binary instead of the built-in one.
