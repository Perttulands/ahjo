//! The values shared by mission documents, edits and command schemas.

use std::fmt;

macro_rules! mission_enum {
    ($(#[$attr:meta])* pub enum $name:ident { $($(#[$variant_attr:meta])* $variant:ident => $wire:literal),+ $(,)? }) => {
        #[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, serde::Deserialize, serde::Serialize, schemars::JsonSchema)]
        $(#[$attr])*
        pub enum $name {
            $($(#[$variant_attr])* #[serde(rename = $wire)] $variant),+
        }

        impl $name {
            pub const ALL: &'static [Self] = &[$(Self::$variant),+];

            pub const fn as_str(self) -> &'static str {
                match self {
                    $(Self::$variant => $wire),+
                }
            }
        }

        impl fmt::Display for $name {
            fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
                f.write_str(self.as_str())
            }
        }

        impl std::str::FromStr for $name {
            type Err = String;

            fn from_str(value: &str) -> Result<Self, Self::Err> {
                match value {
                    $($wire => Ok(Self::$variant),)+
                    _ => Err(format!("unknown {} {value:?}; expected {}", stringify!($name), [$($wire),+].join(", "))),
                }
            }
        }
    };
}

mission_enum! {
    #[derive(Default)]
    pub enum Formation {
        #[default] Unset => "",
        Solo => "solo",
        Peer => "peer",
        Orchestrated => "orchestrated",
    }
}

mission_enum! {
    pub enum GateKind {
        Code => "code",
        Step => "step",
        Human => "human",
    }
}

mission_enum! {
    #[derive(Default)]
    pub enum EndOutcome {
        #[default] Unset => "",
        Done => "done",
        Rejected => "rejected",
    }
}

mission_enum! {
    #[derive(Default)]
    pub enum PortKind {
        #[default] Unset => "",
        Text => "text",
        File => "file",
        Folder => "folder",
    }
}

impl PortKind {
    pub const fn effective(self) -> Self {
        match self {
            Self::Unset => Self::Text,
            kind => kind,
        }
    }

    pub const fn is_unset(&self) -> bool {
        matches!(self, Self::Unset)
    }
}

mission_enum! {
    #[derive(Default)]
    pub enum HumanChannel {
        #[default] Unset => "",
        Notify => "notify",
        Session => "session",
    }
}

impl HumanChannel {
    pub const fn is_unset(&self) -> bool {
        matches!(self, Self::Unset)
    }
}

mission_enum! {
    #[derive(Default)]
    pub enum WhenSpent {
        #[default] Unset => "",
        Stop => "stop",
        Best => "best",
        Fail => "fail",
    }
}

impl WhenSpent {
    pub const fn is_unset(&self) -> bool {
        matches!(self, Self::Unset)
    }
}

mission_enum! {
    #[derive(Default)]
    pub enum Effort {
        #[default] Unset => "",
        Low => "low",
        Medium => "medium",
        High => "high",
        Xhigh => "xhigh",
        Max => "max",
        Ultra => "ultra",
    }
}

impl Effort {
    pub const fn is_unset(&self) -> bool {
        matches!(self, Self::Unset)
    }
}

mission_enum! {
    #[derive(Default)]
    pub enum Harness {
        #[default] Unset => "",
        ClaudeCode => "claude-code",
        OpenAiCodex => "openai-codex",
        Pi => "pi",
    }
}

impl Harness {
    pub const fn is_unset(&self) -> bool {
        matches!(self, Self::Unset)
    }
}
