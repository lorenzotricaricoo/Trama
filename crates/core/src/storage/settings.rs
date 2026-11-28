//! Typed library settings, stored in the `setting` key/value table.
//!
//! These are the preferences that belong to the library (they travel with it): the
//! meeting language, audio retention, and the currency and budget used to track what
//! language models cost. Preferences of the app on this Mac (interface language, theme,
//! microphone, library location) live in the app configuration, see [`crate::config`].
//! Secrets are never stored here.

use super::{Library, Result, StorageError};

/// Keys of the library settings in the `setting` table.
pub mod setting_key {
    /// Language meetings are recorded in and summaries written in (BCP 47, for example
    /// `it`): the key the summary job reads with [`super::Library::meeting_language`].
    pub const MEETING_LANGUAGE: &str = super::super::MEETING_LANGUAGE_SETTING;
    /// How long a meeting's audio is kept after a successful transcription
    /// ([`super::AudioRetention`]).
    pub const AUDIO_RETENTION: &str = "audio.retention";
    /// Currency of model prices and of the budget (ISO 4217, for example `USD`).
    pub const USAGE_CURRENCY: &str = "usage.currency";
    /// Monthly budget for language models, in millionths of the currency unit; absent
    /// when there is no budget.
    pub const BUDGET_MONTHLY_MICROS: &str = "budget.monthly_micros";
    /// Share of the budget, in percent, from which the interface warns.
    pub const BUDGET_WARN_PERCENT: &str = "budget.warn_percent";
    /// `true` when calls are refused once the month's spend reaches the budget.
    pub const BUDGET_ENFORCE: &str = "budget.enforce";
}

text_enum! {
    /// How long a meeting's audio is kept after a successful transcription (see the
    /// information architecture, "Audio retention").
    AudioRetention {
        /// Deleted as soon as the transcription succeeds (the default).
        Never = "never",
        Days7 = "7_days",
        Days30 = "30_days",
        /// Kept.
        Always = "always",
    }
}

impl AudioRetention {
    /// Every option, in the order the interface lists them.
    pub const ALL: &[AudioRetention] = &[Self::Never, Self::Days7, Self::Days30, Self::Always];

    /// Days the audio is kept, `None` when it is kept forever.
    pub const fn days(self) -> Option<u32> {
        match self {
            Self::Never => Some(0),
            Self::Days7 => Some(7),
            Self::Days30 => Some(30),
            Self::Always => None,
        }
    }
}

/// Currency and budget used to track what language models cost.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct UsageSettings {
    /// ISO 4217 code of model prices and the budget, for display only: nothing is converted.
    pub currency: String,
    /// Monthly budget in millionths of the currency unit; `None` for no budget.
    pub monthly_budget_micros: Option<u64>,
    /// Share of the budget, in percent (1 to 100), from which the interface warns.
    pub warn_percent: u8,
    /// Refuse calls (`budget_exceeded`) once the month's spend reaches the budget.
    pub enforce_budget: bool,
}

impl Default for UsageSettings {
    fn default() -> Self {
        Self {
            currency: "USD".to_owned(),
            monthly_budget_micros: None,
            warn_percent: 80,
            enforce_budget: false,
        }
    }
}

/// Every library setting, with its default when the user hasn't chosen.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LibrarySettings {
    /// BCP 47 tag, for example `it`.
    pub meeting_language: String,
    pub audio_retention: AudioRetention,
    pub usage: UsageSettings,
}

impl Default for LibrarySettings {
    fn default() -> Self {
        Self {
            meeting_language: super::DEFAULT_MEETING_LANGUAGE.to_owned(),
            audio_retention: AudioRetention::Never,
            usage: UsageSettings::default(),
        }
    }
}

/// Whether `tag` looks like a BCP 47 language tag: a 2 or 3 letter language, then
/// subtags of 1 to 8 letters or digits.
pub fn is_language_tag(tag: &str) -> bool {
    let mut parts = tag.split('-');
    let language = parts.next().unwrap_or_default();
    (2..=3).contains(&language.len())
        && language.chars().all(|c| c.is_ascii_alphabetic())
        && parts.all(|part| {
            (1..=8).contains(&part.len()) && part.chars().all(|c| c.is_ascii_alphanumeric())
        })
}

fn is_currency(code: &str) -> bool {
    code.len() == 3 && code.chars().all(|c| c.is_ascii_uppercase())
}

impl Library {
    /// The library settings. A missing or unreadable value gives its default, so a damaged
    /// setting never stops the app.
    pub fn library_settings(&self) -> Result<LibrarySettings> {
        let defaults = LibrarySettings::default();
        let meeting_language = self
            .setting(setting_key::MEETING_LANGUAGE)?
            .map(|tag| tag.trim().to_owned())
            .filter(|tag| is_language_tag(tag))
            .unwrap_or(defaults.meeting_language);
        let audio_retention = self
            .setting(setting_key::AUDIO_RETENTION)?
            .and_then(|value| value.parse().ok())
            .unwrap_or(defaults.audio_retention);
        let currency = self
            .setting(setting_key::USAGE_CURRENCY)?
            .filter(|code| is_currency(code))
            .unwrap_or(defaults.usage.currency);
        let monthly_budget_micros = self
            .setting(setting_key::BUDGET_MONTHLY_MICROS)?
            .and_then(|value| value.parse().ok());
        let warn_percent = self
            .setting(setting_key::BUDGET_WARN_PERCENT)?
            .and_then(|value| value.parse().ok())
            .filter(|percent| (1..=100).contains(percent))
            .unwrap_or(defaults.usage.warn_percent);
        let enforce_budget = self
            .setting(setting_key::BUDGET_ENFORCE)?
            .is_some_and(|value| value == "true");
        Ok(LibrarySettings {
            meeting_language,
            audio_retention,
            usage: UsageSettings {
                currency,
                monthly_budget_micros,
                warn_percent,
                enforce_budget,
            },
        })
    }

    /// Stores every library setting at once, after checking them. Changing the audio
    /// retention here deletes nothing: applying it to existing meetings, after the user
    /// confirms, is the retention job's work.
    pub fn set_library_settings(&mut self, settings: &LibrarySettings) -> Result<LibrarySettings> {
        let language = settings.meeting_language.trim();
        if !is_language_tag(language) {
            return Err(StorageError::Invalid(
                "the meeting language must be a BCP 47 tag",
            ));
        }
        let usage = &settings.usage;
        if !is_currency(&usage.currency) {
            return Err(StorageError::Invalid(
                "the currency must be an ISO 4217 code",
            ));
        }
        if !(1..=100).contains(&usage.warn_percent) {
            return Err(StorageError::Invalid(
                "the budget warning must be from 1 to 100%",
            ));
        }
        if usage
            .monthly_budget_micros
            .is_some_and(|budget| i64::try_from(budget).is_err())
        {
            return Err(StorageError::Invalid("budget too large"));
        }

        let tx = self.conn.transaction()?;
        let set = |key: &str, value: Option<String>| -> Result<()> {
            match value {
                Some(value) => tx.execute(
                    "INSERT INTO setting (key, value) VALUES (?1, ?2)
                     ON CONFLICT (key) DO UPDATE SET value = excluded.value",
                    [key, value.as_str()],
                ),
                None => tx.execute("DELETE FROM setting WHERE key = ?1", [key]),
            }?;
            Ok(())
        };
        set(setting_key::MEETING_LANGUAGE, Some(language.to_owned()))?;
        set(
            setting_key::AUDIO_RETENTION,
            Some(settings.audio_retention.as_str().to_owned()),
        )?;
        set(setting_key::USAGE_CURRENCY, Some(usage.currency.clone()))?;
        set(
            setting_key::BUDGET_MONTHLY_MICROS,
            usage.monthly_budget_micros.map(|budget| budget.to_string()),
        )?;
        set(
            setting_key::BUDGET_WARN_PERCENT,
            Some(usage.warn_percent.to_string()),
        )?;
        set(
            setting_key::BUDGET_ENFORCE,
            Some(usage.enforce_budget.to_string()),
        )?;
        tx.commit()?;
        self.library_settings()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn language_tags_are_checked() {
        for tag in ["it", "en", "pt-BR", "zh-Hant-TW", "fil"] {
            assert!(is_language_tag(tag), "{tag}");
        }
        for tag in ["", "i", "italian", "it_IT", "it-", "en-toolongsubtag", "1t"] {
            assert!(!is_language_tag(tag), "{tag}");
        }
    }

    #[test]
    fn retention_names_round_trip() {
        for option in AudioRetention::ALL {
            assert_eq!(option.as_str().parse::<AudioRetention>().unwrap(), *option);
        }
        assert_eq!(AudioRetention::Days30.days(), Some(30));
        assert_eq!(AudioRetention::Always.days(), None);
    }
}
