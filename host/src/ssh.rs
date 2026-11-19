//! Runs commands on the target with the system's ssh, so the user's keys and config just work

use std::io::Write;
use std::process::{Command, Stdio};

use anyhow::{Context, Result, ensure};

pub struct Ssh {
    target: String,
    password: Option<String>,
}

impl Ssh {
    pub fn new(target: &str, password: Option<String>) -> Self {
        Self { target: target.to_string(), password }
    }

    pub fn host(&self) -> &str {
        self.target.rsplit('@').next().unwrap_or(&self.target)
    }

    pub fn run(&self, command: &str) -> Result<String> {
        self.run_with_input(command, &[])
    }

    /// Writes data to remote as root
    pub fn put(&self, data: &[u8], remote: &str) -> Result<()> {
        self.run_with_input(&format!("sudo tee {remote} >/dev/null"), data)?;
        Ok(())
    }

    fn run_with_input(&self, command: &str, input: &[u8]) -> Result<String> {
        let mut ssh = self
            .ssh()
            .arg(&self.target)
            .arg(command)
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .spawn()
            .context("cannot run ssh")?;
        if let Some(mut stdin) = ssh.stdin.take() {
            // Fails when the command exits early, its status says why
            let _ = stdin.write_all(input);
        }
        let output = ssh.wait_with_output().context("cannot run ssh")?;
        let stderr = String::from_utf8_lossy(&output.stderr);
        ensure!(
            output.status.success(),
            "'{command}' failed ({}): {}",
            output.status,
            stderr.trim()
        );
        Ok(String::from_utf8_lossy(&output.stdout).trim().to_string())
    }

    fn ssh(&self) -> Command {
        let mut command = match &self.password {
            Some(password) => {
                // From the environment, so it doesn't show in ps
                let mut command = Command::new("sshpass");
                command.args(["-e", "ssh"]).env("SSHPASS", password);
                command.args([
                    "-o",
                    "PreferredAuthentications=password",
                    "-o",
                    "PubkeyAuthentication=no",
                ]);
                command
            }
            None => Command::new("ssh"),
        };
        // Every flash gives the target a new host key
        command.args(["-o", "StrictHostKeyChecking=no", "-o", "UserKnownHostsFile=/dev/null"]);
        command.args(["-o", "LogLevel=ERROR", "-o", "ConnectTimeout=3"]);
        command
    }
}
