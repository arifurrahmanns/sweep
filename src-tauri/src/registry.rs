//! Apps whose files commonly stay behind after the app itself is deleted.
//!
//! A folder is only offered for deletion when the app can't be found in any
//! way, so data of installed apps is never listed.

use crate::os::Os;

pub struct KnownApp {
    pub name: &'static str,
    /// Where the app lives once installed. The app counts as installed if any
    /// of them exists. On macOS these are app bundle names; on Windows,
    /// paths relative to the user folder or starting with `%ProgramFiles%`
    /// or another environment variable.
    pub installs: &'static [&'static str],
    /// Command-line tools that use the same files.
    pub clis: &'static [&'static str],
    /// Paths relative to the home folder.
    pub paths: &'static [&'static str],
    /// Short note shown after "Not installed".
    pub note: &'static str,
}

/// The list for the system Sweep looks at.
pub fn known_apps(os: Os) -> &'static [KnownApp] {
    match os {
        Os::Mac => KNOWN_APPS_MAC,
        Os::Windows => KNOWN_APPS_WINDOWS,
    }
}

pub const KNOWN_APPS_MAC: &[KnownApp] = &[
    KnownApp {
        name: "Antigravity",
        installs: &["Antigravity.app", "Antigravity IDE.app"],
        clis: &["antigravity"],
        paths: &[
            ".antigravity",
            ".antigravity-ide",
            ".gemini/antigravity",
            ".gemini/antigravity-ide",
            ".gemini/antigravity-backup",
            ".gemini/antigravity-browser-profile",
            "Library/Application Support/Antigravity",
            "Library/Application Support/Antigravity IDE",
        ],
        note: "includes chat history",
    },
    KnownApp {
        name: "Cursor",
        installs: &["Cursor.app"],
        clis: &["cursor"],
        paths: &[".cursor", "Library/Application Support/Cursor"],
        note: "",
    },
    KnownApp {
        name: "Windsurf",
        installs: &["Windsurf.app"],
        clis: &["windsurf"],
        paths: &[".windsurf", ".codeium/windsurf", "Library/Application Support/Windsurf"],
        note: "",
    },
    KnownApp {
        name: "Visual Studio Code",
        installs: &["Visual Studio Code.app"],
        clis: &[],
        paths: &[".vscode", "Library/Application Support/Code"],
        note: "",
    },
    KnownApp {
        name: "Arduino IDE",
        installs: &["Arduino IDE.app", "Arduino.app"],
        clis: &["arduino-cli"],
        paths: &[".arduinoIDE", "Library/Arduino15"],
        note: "includes board toolchains",
    },
    KnownApp {
        name: "Android Studio",
        installs: &["Android Studio.app"],
        clis: &["adb"],
        paths: &["Library/Android", ".android"],
        note: "includes the Android SDK",
    },
    KnownApp {
        name: "Comet",
        installs: &["Comet.app"],
        clis: &[],
        paths: &["Library/Application Support/Comet"],
        note: "includes your browser profile",
    },
    KnownApp {
        name: "Google Chrome",
        installs: &["Google Chrome.app"],
        clis: &[],
        paths: &["Library/Application Support/Google/Chrome"],
        note: "includes your browser profile",
    },
    KnownApp {
        name: "Brave Browser",
        installs: &["Brave Browser.app"],
        clis: &[],
        paths: &["Library/Application Support/BraveSoftware"],
        note: "includes your browser profile",
    },
    KnownApp {
        name: "Microsoft Edge",
        installs: &["Microsoft Edge.app"],
        clis: &[],
        paths: &["Library/Application Support/Microsoft Edge"],
        note: "includes your browser profile",
    },
    KnownApp {
        name: "Discord",
        installs: &["Discord.app"],
        clis: &[],
        paths: &["Library/Application Support/discord"],
        note: "",
    },
    KnownApp {
        name: "Zoom",
        installs: &["zoom.us.app"],
        clis: &[],
        paths: &["Library/Application Support/zoom.us"],
        note: "",
    },
    KnownApp {
        name: "Slack",
        installs: &["Slack.app"],
        clis: &[],
        paths: &[
            "Library/Application Support/Slack",
            "Library/Containers/com.tinyspeck.slackmacgap",
        ],
        note: "",
    },
    KnownApp {
        name: "Microsoft Teams",
        installs: &["Microsoft Teams.app", "Microsoft Teams (work or school).app"],
        clis: &[],
        paths: &[
            "Library/Containers/com.microsoft.teams2",
            "Library/Application Support/Microsoft/Teams",
        ],
        note: "",
    },
    KnownApp {
        name: "Spotify",
        installs: &["Spotify.app"],
        clis: &[],
        paths: &["Library/Application Support/Spotify"],
        note: "includes downloaded songs",
    },
    KnownApp {
        name: "Figma",
        installs: &["Figma.app"],
        clis: &[],
        paths: &["Library/Application Support/Figma"],
        note: "",
    },
    KnownApp {
        name: "Notion",
        installs: &["Notion.app"],
        clis: &[],
        paths: &["Library/Application Support/Notion"],
        note: "",
    },
    KnownApp {
        name: "Postman",
        installs: &["Postman.app"],
        clis: &[],
        paths: &["Library/Application Support/Postman"],
        note: "may include unsynced collections",
    },
    KnownApp {
        name: "Beekeeper Studio",
        installs: &["Beekeeper Studio.app"],
        clis: &[],
        paths: &["Library/Application Support/beekeeper-studio"],
        note: "includes saved connections",
    },
    KnownApp {
        name: "DBngin",
        installs: &["DBngin.app"],
        clis: &[],
        paths: &["Library/Application Support/com.tinyapp.DBngin"],
        note: "includes database data",
    },
    KnownApp {
        name: "opencode",
        installs: &["OpenCode.app", "opencode.app"],
        clis: &[],
        paths: &["Library/Application Support/ai.opencode.desktop"],
        note: "",
    },
    KnownApp {
        name: "Steam",
        installs: &["Steam.app"],
        clis: &[],
        paths: &["Library/Application Support/Steam"],
        note: "includes installed games",
    },
    KnownApp {
        name: "Telegram",
        installs: &["Telegram.app"],
        clis: &[],
        paths: &["Library/Group Containers/6N38VWS5BX.ru.keepcoder.Telegram"],
        note: "includes downloaded media",
    },
];

pub const KNOWN_APPS_WINDOWS: &[KnownApp] = &[
    KnownApp {
        name: "Antigravity",
        installs: &["AppData/Local/Programs/Antigravity"],
        clis: &["antigravity"],
        paths: &[
            ".antigravity",
            ".gemini/antigravity",
            ".gemini/antigravity-ide",
            ".gemini/antigravity-backup",
            ".gemini/antigravity-browser-profile",
            "AppData/Roaming/Antigravity",
        ],
        note: "includes chat history",
    },
    KnownApp {
        name: "Cursor",
        installs: &["AppData/Local/Programs/cursor", "%ProgramFiles%/cursor"],
        clis: &["cursor"],
        paths: &[".cursor", "AppData/Roaming/Cursor"],
        note: "",
    },
    KnownApp {
        name: "Windsurf",
        installs: &["AppData/Local/Programs/Windsurf", "%ProgramFiles%/Windsurf"],
        clis: &["windsurf"],
        paths: &[".windsurf", ".codeium/windsurf", "AppData/Roaming/Windsurf"],
        note: "",
    },
    KnownApp {
        name: "Visual Studio Code",
        installs: &["AppData/Local/Programs/Microsoft VS Code", "%ProgramFiles%/Microsoft VS Code"],
        clis: &["code"],
        paths: &[".vscode", "AppData/Roaming/Code"],
        note: "",
    },
    KnownApp {
        name: "Arduino IDE",
        installs: &[
            "AppData/Local/Programs/Arduino IDE",
            "%ProgramFiles%/Arduino IDE",
            "%ProgramFiles(x86)%/Arduino",
        ],
        clis: &["arduino-cli"],
        paths: &[".arduinoIDE", "AppData/Roaming/arduino-ide", "AppData/Local/Arduino15"],
        note: "includes board toolchains",
    },
    KnownApp {
        name: "Android Studio",
        installs: &["%ProgramFiles%/Android/Android Studio"],
        clis: &["adb"],
        paths: &["AppData/Local/Android", ".android"],
        note: "includes the Android SDK",
    },
    KnownApp {
        name: "Google Chrome",
        installs: &[
            "%ProgramFiles%/Google/Chrome/Application/chrome.exe",
            "%ProgramFiles(x86)%/Google/Chrome/Application/chrome.exe",
            "AppData/Local/Google/Chrome/Application/chrome.exe",
        ],
        clis: &[],
        paths: &["AppData/Local/Google/Chrome/User Data"],
        note: "includes your browser profile",
    },
    KnownApp {
        name: "Brave Browser",
        installs: &[
            "%ProgramFiles%/BraveSoftware/Brave-Browser/Application/brave.exe",
            "AppData/Local/BraveSoftware/Brave-Browser/Application/brave.exe",
        ],
        clis: &[],
        paths: &["AppData/Local/BraveSoftware/Brave-Browser/User Data"],
        note: "includes your browser profile",
    },
    KnownApp {
        name: "Discord",
        installs: &["AppData/Local/Discord/Update.exe"],
        clis: &[],
        paths: &["AppData/Roaming/discord"],
        note: "",
    },
    KnownApp {
        name: "Zoom",
        installs: &["AppData/Roaming/Zoom/bin/Zoom.exe", "%ProgramFiles%/Zoom/bin/Zoom.exe"],
        clis: &[],
        paths: &["AppData/Roaming/Zoom"],
        note: "",
    },
    KnownApp {
        name: "Slack",
        installs: &["AppData/Local/slack/slack.exe", "%ProgramFiles%/Slack/slack.exe"],
        clis: &[],
        paths: &["AppData/Roaming/Slack"],
        note: "",
    },
    KnownApp {
        name: "Microsoft Teams (classic)",
        installs: &["AppData/Local/Microsoft/Teams/current/Teams.exe"],
        clis: &[],
        paths: &["AppData/Roaming/Microsoft/Teams"],
        note: "",
    },
    KnownApp {
        name: "Spotify",
        installs: &["AppData/Roaming/Spotify/Spotify.exe"],
        clis: &[],
        paths: &["AppData/Local/Spotify"],
        note: "includes downloaded songs",
    },
    KnownApp {
        name: "Figma",
        installs: &["AppData/Local/Figma/Figma.exe"],
        clis: &[],
        paths: &["AppData/Roaming/Figma"],
        note: "",
    },
    KnownApp {
        name: "Notion",
        installs: &["AppData/Local/Programs/Notion"],
        clis: &[],
        paths: &["AppData/Roaming/Notion"],
        note: "",
    },
    KnownApp {
        name: "Postman",
        installs: &["AppData/Local/Postman/Postman.exe"],
        clis: &[],
        paths: &["AppData/Roaming/Postman"],
        note: "may include unsynced collections",
    },
    KnownApp {
        name: "Beekeeper Studio",
        installs: &["AppData/Local/Programs/beekeeper-studio", "%ProgramFiles%/Beekeeper Studio"],
        clis: &[],
        paths: &["AppData/Roaming/beekeeper-studio"],
        note: "includes saved connections",
    },
    KnownApp {
        name: "Telegram",
        installs: &["AppData/Roaming/Telegram Desktop/Telegram.exe"],
        clis: &[],
        paths: &["AppData/Roaming/Telegram Desktop"],
        note: "includes downloaded media",
    },
];
