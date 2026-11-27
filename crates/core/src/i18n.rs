//! Localization of user-facing strings.
//!
//! Messages are written in [Fluent](https://projectfluent.org/) and live in
//! `locales/<locale>/<catalog>.ftl` at the repository root, shared by the core, the
//! desktop shell and the frontend. Code never hard-codes user-facing text (principle A5):
//! it asks a [`Localizer`] for a message by id.

use fluent_bundle::concurrent::FluentBundle;
use fluent_bundle::{FluentArgs, FluentResource};
use fluent_langneg::{NegotiationStrategy, negotiate_languages};
use unic_langid::{LanguageIdentifier, langid};

/// Locales with a complete set of catalogs, in order of preference for fallback.
pub const SUPPORTED_LOCALES: &[LanguageIdentifier] = &[langid!("en"), langid!("it")];

/// Locale used when none of the requested locales is supported.
pub const DEFAULT_LOCALE: LanguageIdentifier = langid!("en");

/// One Fluent catalog, embedded at build time.
#[derive(Debug, Clone, Copy)]
pub struct Catalog {
    /// Locale the catalog is written in, for example `"it"`.
    pub locale: &'static str,
    /// Catalog name, for example `"menu"` for `locales/it/menu.ftl`.
    pub name: &'static str,
    /// Fluent source.
    pub source: &'static str,
}

/// Errors raised while building a [`Localizer`].
#[derive(Debug, PartialEq, Eq)]
pub enum I18nError {
    /// A catalog has Fluent syntax errors.
    Syntax { locale: String, name: String },
    /// Two catalogs of the same locale define the same message id.
    Duplicate { locale: String, name: String },
}

impl std::fmt::Display for I18nError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Syntax { locale, name } => {
                write!(f, "syntax error in catalog {locale}/{name}.ftl")
            }
            Self::Duplicate { locale, name } => {
                write!(f, "duplicate message id in catalog {locale}/{name}.ftl")
            }
        }
    }
}

impl std::error::Error for I18nError {}

/// Locales preferred by the operating system, most preferred first.
pub fn system_locales() -> Vec<String> {
    sys_locale::get_locales().collect()
}

/// Picks the supported locale that best matches the requested ones (BCP 47 tags,
/// most preferred first). Unknown or malformed tags are ignored.
pub fn negotiate<S: AsRef<str>>(requested: &[S]) -> LanguageIdentifier {
    let requested: Vec<LanguageIdentifier> = requested
        .iter()
        .filter_map(|tag| tag.as_ref().parse().ok())
        .collect();
    negotiate_languages(
        &requested,
        SUPPORTED_LOCALES,
        Some(&DEFAULT_LOCALE),
        NegotiationStrategy::Lookup,
    )
    .first()
    .map(|locale| (*locale).clone())
    .unwrap_or(DEFAULT_LOCALE)
}

/// Formats messages for one locale.
pub struct Localizer {
    locale: LanguageIdentifier,
    bundle: FluentBundle<FluentResource>,
}

impl std::fmt::Debug for Localizer {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Localizer")
            .field("locale", &self.locale)
            .finish_non_exhaustive()
    }
}

impl Localizer {
    /// Builds a localizer for the best match among `requested`, using the catalogs of
    /// that locale.
    pub fn new<S: AsRef<str>>(requested: &[S], catalogs: &[Catalog]) -> Result<Self, I18nError> {
        let locale = negotiate(requested);
        let mut bundle = FluentBundle::new_concurrent(vec![locale.clone()]);
        // Supported locales are all left-to-right; Unicode isolation marks around
        // placeables would only end up in native menus and notifications.
        bundle.set_use_isolating(false);

        let tag = locale.to_string();
        for catalog in catalogs.iter().filter(|c| c.locale == tag) {
            let resource = FluentResource::try_new(catalog.source.to_owned()).map_err(|_| {
                I18nError::Syntax {
                    locale: catalog.locale.to_owned(),
                    name: catalog.name.to_owned(),
                }
            })?;
            bundle
                .add_resource(resource)
                .map_err(|_| I18nError::Duplicate {
                    locale: catalog.locale.to_owned(),
                    name: catalog.name.to_owned(),
                })?;
        }
        Ok(Self { locale, bundle })
    }

    /// The negotiated locale.
    pub fn locale(&self) -> &LanguageIdentifier {
        &self.locale
    }

    /// Formats the message `id`. A missing message returns the id itself, so a gap in
    /// a catalog is visible instead of silently empty; the catalog tests catch it first.
    pub fn message(&self, id: &str, args: Option<&FluentArgs>) -> String {
        let Some(pattern) = self.bundle.get_message(id).and_then(|m| m.value()) else {
            return id.to_owned();
        };
        let mut errors = Vec::new();
        let text = self.bundle.format_pattern(pattern, args, &mut errors);
        if errors.is_empty() {
            text.into_owned()
        } else {
            id.to_owned()
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::{BTreeMap, BTreeSet};
    use std::path::{Path, PathBuf};

    const EN: Catalog = Catalog {
        locale: "en",
        name: "test",
        source: "hello = Hello, { $name }!\nplain = Plain",
    };
    const IT: Catalog = Catalog {
        locale: "it",
        name: "test",
        source: "hello = Ciao, { $name }!\nplain = Semplice",
    };

    #[test]
    fn negotiates_supported_locales() {
        assert_eq!(negotiate(&["it-IT"]), langid!("it"));
        assert_eq!(negotiate(&["en-GB", "it"]), langid!("en"));
        assert_eq!(negotiate(&["de-DE", "it-CH"]), langid!("it"));
    }

    #[test]
    fn falls_back_to_default_locale() {
        assert_eq!(negotiate(&["de-DE"]), DEFAULT_LOCALE);
        assert_eq!(negotiate::<&str>(&[]), DEFAULT_LOCALE);
        assert_eq!(negotiate(&["not a tag"]), DEFAULT_LOCALE);
    }

    #[test]
    fn formats_messages_with_arguments() {
        let localizer = Localizer::new(&["it"], &[EN, IT]).unwrap();
        let mut args = FluentArgs::new();
        args.set("name", "Ada");
        assert_eq!(localizer.message("hello", Some(&args)), "Ciao, Ada!");
        assert_eq!(localizer.message("plain", None), "Semplice");
    }

    #[test]
    fn missing_message_returns_its_id() {
        let localizer = Localizer::new(&["en"], &[EN]).unwrap();
        assert_eq!(localizer.message("nope", None), "nope");
    }

    #[test]
    fn rejects_broken_catalogs() {
        let broken = Catalog {
            locale: "en",
            name: "broken",
            source: "= no id",
        };
        assert_eq!(
            Localizer::new(&["en"], &[broken]).unwrap_err(),
            I18nError::Syntax {
                locale: "en".into(),
                name: "broken".into()
            }
        );
    }

    fn locales_dir() -> PathBuf {
        Path::new(env!("CARGO_MANIFEST_DIR")).join("../../locales")
    }

    /// Message ids per catalog name, for one locale directory.
    fn message_ids(dir: &Path) -> BTreeMap<String, BTreeSet<String>> {
        let mut catalogs = BTreeMap::new();
        for entry in std::fs::read_dir(dir).unwrap() {
            let path = entry.unwrap().path();
            if path.extension().is_none_or(|ext| ext != "ftl") {
                continue;
            }
            let name = path.file_stem().unwrap().to_string_lossy().into_owned();
            let source = std::fs::read_to_string(&path).unwrap();
            let resource = FluentResource::try_new(source)
                .unwrap_or_else(|_| panic!("syntax error in {}", path.display()));
            let ids = resource
                .entries()
                .filter_map(|entry| match entry {
                    fluent_syntax::ast::Entry::Message(m) => Some(m.id.name.to_owned()),
                    fluent_syntax::ast::Entry::Term(t) => Some(format!("-{}", t.id.name)),
                    _ => None,
                })
                .collect();
            catalogs.insert(name, ids);
        }
        catalogs
    }

    #[test]
    fn every_locale_has_the_same_catalogs_and_messages() {
        let reference = message_ids(&locales_dir().join(DEFAULT_LOCALE.to_string()));
        assert!(!reference.is_empty(), "no catalogs found");
        for locale in SUPPORTED_LOCALES {
            let ids = message_ids(&locales_dir().join(locale.to_string()));
            assert_eq!(
                ids, reference,
                "catalogs of {locale} differ from {DEFAULT_LOCALE}"
            );
        }
    }
}
