//! Move the old node config section when the binary starts successfully.

use std::path::{Path, PathBuf};

use ahjo_core::Error;
use ahjo_node_kit::Config;
use toml_edit::DocumentMut;

use crate::shape::edit::replace_file;

pub struct ConfigRename {
    path: PathBuf,
    original: String,
    updated: String,
}

impl ConfigRename {
    pub fn load(path: &Path) -> Result<(Config, Option<Self>), Error> {
        let bad =
            |message: &str| Error::new("bad_config", format!("{}: {message}", path.display()));
        let original = match std::fs::read_to_string(path) {
            Ok(text) => text,
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
                return Ok((Config::parse_file("", path.to_owned())?, None));
            }
            Err(error) => return Err(bad(&error.to_string())),
        };
        let mut doc = original
            .parse::<DocumentMut>()
            .map_err(|error| bad(&error.to_string()))?;
        let Some(agents) = doc.remove("agents") else {
            return Ok((Config::parse_file(&original, path.to_owned())?, None));
        };
        if doc.contains_key("harness") {
            return Err(bad(
                "both [agents] and [harness] are set. Move the settings into [harness].",
            ));
        }
        doc.insert("harness", agents);
        let updated = doc.to_string();
        let config = Config::parse_file(&updated, path.to_owned())?;
        Ok((
            config,
            Some(Self {
                path: path.to_owned(),
                original,
                updated,
            }),
        ))
    }

    /// Only record the rename after the config has built the node.
    pub fn save(self) -> Result<(), Error> {
        let bad =
            |message: &str| Error::new("bad_config", format!("{}: {message}", self.path.display()));
        let current =
            std::fs::read_to_string(&self.path).map_err(|error| bad(&error.to_string()))?;
        if current != self.original {
            return Err(bad(
                "the config changed while the node started. Start it again.",
            ));
        }
        replace_file(&self.path, self.updated.as_bytes()).map_err(|error| bad(&error.message))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_section_rename_preserves_settings_and_the_config_path() {
        let dir = ahjo_testkit::fresh_dir("harness-config");
        let path = dir.join("custom.toml");
        let before = "# Keep this comment\n[agents]\npath = [\"./tools\"]\n# Leave it off\nenabled = false\n";
        std::fs::write(&path, before).unwrap();

        let (config, rename) = ConfigRename::load(&path).unwrap();
        assert_eq!(config.file(), path);
        assert!(!config.enabled("harness").unwrap());
        assert_eq!(
            config.show(Some("harness"), &["harness"]).unwrap()["sections"]["harness"]["path"][0],
            "./tools"
        );
        assert_eq!(std::fs::read_to_string(&path).unwrap(), before);
        rename.unwrap().save().unwrap();
        let after = std::fs::read_to_string(&path).unwrap();
        assert!(after.contains("[harness]"));
        assert!(after.contains("# Keep this comment") && after.contains("# Leave it off"));
        assert!(ConfigRename::load(&path).unwrap().1.is_none());
        std::fs::remove_dir_all(dir).unwrap();
    }
}
