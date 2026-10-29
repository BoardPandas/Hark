//! One line per speech-to-text provider, and where to get its key, shown at
//! the point the user picks one (spec §2.5).
//!
//! Verified 2026-10-29; re-check every URL and the Gemini data-use line before
//! each release. Both change without a Hark release, and nothing here would
//! fail if they went stale. No prices or free-tier claims: Hark cannot detect
//! when those change. The Gemini line is the exception because it is a privacy
//! disclosure.

use hark_config::{Provider, ProviderKind};

pub struct ProviderHelp {
    /// May contain `{model}`; [`ProviderHelp::line`] fills it in.
    line: &'static str,
    pub key_page: Option<KeyPage>,
}

pub struct KeyPage {
    pub text: &'static str,
    /// A compile-time `https://` constant with no query string.
    pub url: &'static str,
}

impl ProviderHelp {
    /// The line with `{model}` replaced by the model actually in use, so the
    /// copy can never name a default the code no longer has.
    pub fn line(&self, model: &str) -> String {
        self.line.replace("{model}", model)
    }
}

/// The line for `kind`, naming that kind's default model: the one used when
/// the Model field is left blank.
pub fn describe(kind: ProviderKind) -> String {
    let model = Provider {
        kind,
        ..Default::default()
    }
    .resolved_model();
    help(kind).line(&model)
}

/// The Model and endpoint header (R12). Only a custom endpoint has no
/// recommended model to fall back on.
pub fn model_header(custom: bool) -> &'static str {
    if custom {
        "Model and endpoint (required)"
    } else {
        "Model and endpoint (optional)"
    }
}

pub fn help(kind: ProviderKind) -> ProviderHelp {
    let (line, key_page) = match kind {
        ProviderKind::Deepgram => (
            "Hark's default. A speech-to-text service built for dictation. \
             Uses {model} unless you change it below.",
            Some(KeyPage {
                text: "Get a Deepgram key",
                url: "https://console.deepgram.com",
            }),
        ),
        ProviderKind::Openai => (
            "Accurate general-purpose transcription. Uses {model} unless you change it below.",
            Some(KeyPage {
                text: "Get an OpenAI key",
                url: "https://platform.openai.com/api-keys",
            }),
        ),
        ProviderKind::Gemini => (
            "Streams while you hold the key. Smart mode can tidy text in the same call. \
             On Google's free tier, Google may use what you send to improve its products.",
            Some(KeyPage {
                text: "Get a Gemini key",
                url: "https://aistudio.google.com/apikey",
            }),
        ),
        ProviderKind::OpenaiCompatible => (
            "Any server with an OpenAI-style transcription endpoint, including one you run. \
             Enter its URL and model below.",
            None,
        ),
    };
    ProviderHelp { line, key_page }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ui::settings::form::provider_choices;

    const ALL: [ProviderKind; 4] = [
        ProviderKind::Deepgram,
        ProviderKind::Openai,
        ProviderKind::Gemini,
        ProviderKind::OpenaiCompatible,
    ];

    #[test]
    fn every_offered_provider_has_a_line() {
        for (kind, label) in provider_choices() {
            assert!(!help(kind).line("m").trim().is_empty(), "{label}");
        }
    }

    #[test]
    fn key_pages_are_plain_https_links() {
        for kind in ALL {
            if let Some(page) = help(kind).key_page {
                assert!(page.url.starts_with("https://"), "{}", page.url);
                assert!(!page.url.contains('?'), "{}", page.url);
                assert!(page.text.starts_with("Get a"), "{}", page.text);
            }
        }
    }

    #[test]
    fn only_openai_compatible_has_no_key_page() {
        for kind in ALL {
            assert_eq!(
                help(kind).key_page.is_none(),
                kind == ProviderKind::OpenaiCompatible,
                "{}",
                kind.label()
            );
        }
    }

    #[test]
    fn only_a_custom_endpoint_requires_a_model() {
        assert_eq!(model_header(true), "Model and endpoint (required)");
        assert_eq!(model_header(false), "Model and endpoint (optional)");
    }

    #[test]
    fn the_model_placeholder_is_filled_in() {
        let line = help(ProviderKind::Deepgram).line("nova-3");
        assert!(line.contains("Uses nova-3 unless"));
        assert!(describe(ProviderKind::Openai).contains("Uses gpt-transcribe unless"));
        for kind in ALL {
            assert!(!help(kind).line("x").contains('{'), "{}", kind.label());
        }
    }
}
