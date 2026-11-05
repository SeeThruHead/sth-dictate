use crate::hotkey::KEY_F18;
use serde::Deserialize;
use std::path::{Path, PathBuf};

#[derive(Clone, Debug, Deserialize, PartialEq)]
#[serde(default, deny_unknown_fields)]
pub struct Settings {
    pub endpoint: String,
    pub model: Option<String>,
    pub hotkey: u16,
    pub hotkey_label: String,
    pub microphone: Option<String>,
    pub sounds: Option<PathBuf>,
}

impl Default for Settings {
    fn default() -> Settings {
        Settings {
            endpoint: "http://192.168.0.149:8000/v1".into(),
            model: None,
            hotkey: KEY_F18,
            hotkey_label: "Right ⌘".into(),
            microphone: None,
            sounds: None,
        }
    }
}

impl Settings {
    pub fn file(home: &Path) -> PathBuf {
        home.join(".config/sth-dictate/settings.json")
    }

    pub fn parse(text: &str, file: &Path) -> Result<Settings, String> {
        serde_json::from_str(text).map_err(|error| format!("{} is not valid: {error}", file.display()))
    }

    pub fn load(home: &Path) -> Result<Settings, String> {
        let file = Settings::file(home);
        std::fs::read_to_string(&file).map_or_else(|_| Ok(Settings::default()), |text| Settings::parse(&text, &file))
    }

    pub fn sounds_dir(&self, home: &Path) -> PathBuf {
        self.sounds.clone().unwrap_or_else(|| home.join(".local/share/sounds/sth-dictate"))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn missing_fields_take_the_defaults() {
        let settings = Settings::parse(r#"{"model":"turbo"}"#, Path::new("s.json")).unwrap();
        assert_eq!(settings, Settings { model: Some("turbo".into()), ..Settings::default() });
    }

    #[test]
    fn a_misspelt_field_is_an_error_naming_the_file() {
        let error = Settings::parse(r#"{"endpont":"x"}"#, Path::new("/home/s/settings.json")).unwrap_err();
        assert!(error.starts_with("/home/s/settings.json is not valid"));
    }

    #[test]
    fn sounds_default_to_the_shared_folder() {
        assert_eq!(
            Settings::default().sounds_dir(Path::new("/h")),
            PathBuf::from("/h/.local/share/sounds/sth-dictate")
        );
    }
}
