//! Apps whose files commonly stay behind after the app itself is deleted.
//!
//! A folder is only offered for deletion when none of the app's bundles or
//! command-line tools can be found, so data of installed apps is never listed.

pub struct KnownApp {
    pub name: &'static str,
    /// App bundle names. The app counts as installed if any of them exists.
    pub bundles: &'static [&'static str],
    /// Command-line tools that use the same files.
    pub clis: &'static [&'static str],
    /// Paths relative to the home folder.
    pub paths: &'static [&'static str],
    /// Short note shown after "Not installed".
    pub note: &'static str,
}

pub const KNOWN_APPS: &[KnownApp] = &[
    KnownApp {
        name: "Antigravity",
        bundles: &["Antigravity.app", "Antigravity IDE.app"],
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
        bundles: &["Cursor.app"],
        clis: &["cursor"],
        paths: &[".cursor", "Library/Application Support/Cursor"],
        note: "",
    },
    KnownApp {
        name: "Windsurf",
        bundles: &["Windsurf.app"],
        clis: &["windsurf"],
        paths: &[".windsurf", ".codeium/windsurf", "Library/Application Support/Windsurf"],
        note: "",
    },
    KnownApp {
        name: "Visual Studio Code",
        bundles: &["Visual Studio Code.app"],
        clis: &[],
        paths: &[".vscode", "Library/Application Support/Code"],
        note: "",
    },
    KnownApp {
        name: "Arduino IDE",
        bundles: &["Arduino IDE.app", "Arduino.app"],
        clis: &["arduino-cli"],
        paths: &[".arduinoIDE", "Library/Arduino15"],
        note: "includes board toolchains",
    },
    KnownApp {
        name: "Android Studio",
        bundles: &["Android Studio.app"],
        clis: &["adb"],
        paths: &["Library/Android", ".android"],
        note: "includes the Android SDK",
    },
    KnownApp {
        name: "Comet",
        bundles: &["Comet.app"],
        clis: &[],
        paths: &["Library/Application Support/Comet"],
        note: "includes your browser profile",
    },
    KnownApp {
        name: "Google Chrome",
        bundles: &["Google Chrome.app"],
        clis: &[],
        paths: &["Library/Application Support/Google/Chrome"],
        note: "includes your browser profile",
    },
    KnownApp {
        name: "Brave Browser",
        bundles: &["Brave Browser.app"],
        clis: &[],
        paths: &["Library/Application Support/BraveSoftware"],
        note: "includes your browser profile",
    },
    KnownApp {
        name: "Microsoft Edge",
        bundles: &["Microsoft Edge.app"],
        clis: &[],
        paths: &["Library/Application Support/Microsoft Edge"],
        note: "includes your browser profile",
    },
    KnownApp {
        name: "Discord",
        bundles: &["Discord.app"],
        clis: &[],
        paths: &["Library/Application Support/discord"],
        note: "",
    },
    KnownApp {
        name: "Zoom",
        bundles: &["zoom.us.app"],
        clis: &[],
        paths: &["Library/Application Support/zoom.us"],
        note: "",
    },
    KnownApp {
        name: "Slack",
        bundles: &["Slack.app"],
        clis: &[],
        paths: &[
            "Library/Application Support/Slack",
            "Library/Containers/com.tinyspeck.slackmacgap",
        ],
        note: "",
    },
    KnownApp {
        name: "Microsoft Teams",
        bundles: &["Microsoft Teams.app", "Microsoft Teams (work or school).app"],
        clis: &[],
        paths: &[
            "Library/Containers/com.microsoft.teams2",
            "Library/Application Support/Microsoft/Teams",
        ],
        note: "",
    },
    KnownApp {
        name: "Spotify",
        bundles: &["Spotify.app"],
        clis: &[],
        paths: &["Library/Application Support/Spotify"],
        note: "includes downloaded songs",
    },
    KnownApp {
        name: "Figma",
        bundles: &["Figma.app"],
        clis: &[],
        paths: &["Library/Application Support/Figma"],
        note: "",
    },
    KnownApp {
        name: "Notion",
        bundles: &["Notion.app"],
        clis: &[],
        paths: &["Library/Application Support/Notion"],
        note: "",
    },
    KnownApp {
        name: "Postman",
        bundles: &["Postman.app"],
        clis: &[],
        paths: &["Library/Application Support/Postman"],
        note: "may include unsynced collections",
    },
    KnownApp {
        name: "Beekeeper Studio",
        bundles: &["Beekeeper Studio.app"],
        clis: &[],
        paths: &["Library/Application Support/beekeeper-studio"],
        note: "includes saved connections",
    },
    KnownApp {
        name: "DBngin",
        bundles: &["DBngin.app"],
        clis: &[],
        paths: &["Library/Application Support/com.tinyapp.DBngin"],
        note: "includes database data",
    },
    KnownApp {
        name: "opencode",
        bundles: &["OpenCode.app", "opencode.app"],
        clis: &[],
        paths: &["Library/Application Support/ai.opencode.desktop"],
        note: "",
    },
    KnownApp {
        name: "Steam",
        bundles: &["Steam.app"],
        clis: &[],
        paths: &["Library/Application Support/Steam"],
        note: "includes installed games",
    },
    KnownApp {
        name: "Telegram",
        bundles: &["Telegram.app"],
        clis: &[],
        paths: &["Library/Group Containers/6N38VWS5BX.ru.keepcoder.Telegram"],
        note: "includes downloaded media",
    },
];
