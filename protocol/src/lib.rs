//! What the burner tells the PC, one message per line.
//!
//! The PC sends the image's size, 8 bytes big-endian, then the image compressed with deflate.

/// The PC connects here to follow the flash and send the image
pub const PORT: u16 = 8099;

#[derive(Debug, PartialEq)]
pub enum Message {
    Log(String),
    Rebooting,
    /// The burner gave up before the point of no return, and the board reboots into the old system
    Aborted(String),
}

impl Message {
    pub fn to_line(&self) -> String {
        match self {
            Self::Log(text) => format!("log {text}\n"),
            Self::Rebooting => "rebooting\n".to_string(),
            Self::Aborted(reason) => format!("aborted {reason}\n"),
        }
    }

    pub fn parse(line: &str) -> Option<Self> {
        let (kind, text) = line.split_once(' ').unwrap_or((line, ""));
        match kind {
            "log" => Some(Self::Log(text.to_string())),
            "rebooting" => Some(Self::Rebooting),
            "aborted" => Some(Self::Aborted(text.to_string())),
            _ => None,
        }
    }
}
