//! Which operating system's folder layout and tools to use.

use std::ffi::OsStr;
use std::process::Command;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Os {
    Mac,
    Windows,
}

impl Os {
    /// The system Sweep is running on.
    pub fn current() -> Os {
        if cfg!(windows) {
            Os::Windows
        } else {
            Os::Mac
        }
    }

    /// Name the window uses to adapt its look.
    pub fn name(self) -> &'static str {
        match self {
            Os::Mac => "macos",
            Os::Windows => "windows",
        }
    }
}

/// A command for a command-line tool. On Windows it runs without flashing a
/// console window over Sweep.
pub fn command(program: impl AsRef<OsStr>) -> Command {
    #[allow(unused_mut)] // only changed on Windows
    let mut cmd = Command::new(program);
    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;
        const CREATE_NO_WINDOW: u32 = 0x0800_0000;
        cmd.creation_flags(CREATE_NO_WINDOW);
    }
    cmd
}
