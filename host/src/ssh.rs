//! Runs commands on the target with the system's ssh, so the user's keys and config just work

use std::fs::File;
use std::path::Path;
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
        self.run_with_input(command, Stdio::null())
    }

    /// Writes local to remote as root
    pub fn put(&self, local: &Path, remote: &str) -> Result<()> {
        let file = File::open(local).with_context(|| format!("cannot open {}", local.display()))?;
        self.run_with_input(&format!("sudo tee {remote} >/dev/null"), file.into())?;
        Ok(())
    }

    fn run_with_input(&self, command: &str, stdin: Stdio) -> Result<String> {
        let output = self
            .ssh()
            .arg(&self.target)
            .arg(command)
            .stdin(stdin)
            .output()
            .context("cannot run ssh")?;
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
