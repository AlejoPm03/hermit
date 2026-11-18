use std::fs;
use std::path::PathBuf;

use anyhow::{Context, Result, ensure};

/// Where the binary and its files are uploaded
pub const DIR: &str = "/run/flash";

pub struct Config {
    pub disk: PathBuf,
}

impl Config {
    pub fn load() -> Result<Self> {
        Ok(Self { disk: read("disk")?.into() })
    }
}

fn read(name: &str) -> Result<String> {
    let text = fs::read_to_string(format!("{DIR}/{name}"))
        .with_context(|| format!("cannot read {name}"))?;
    let value = text.trim();
    ensure!(!value.is_empty(), "{name} is empty");
    Ok(value.to_string())
}
