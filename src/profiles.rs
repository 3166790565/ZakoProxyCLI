use crate::store::Group;
use serde::{Deserialize, Serialize};
use std::sync::OnceLock;

#[derive(Deserialize, Serialize)]
pub struct Profile {
    pub id: String,
    pub name: String,
    pub models: Vec<String>,
    #[serde(default)]
    pub instruction: String,
    #[serde(default)]
    pub model_family: String,
}

pub fn model_family(model: &str) -> Option<&'static str> {
    [
        "gpt-5.6", "gpt-6.1", "gpt-6", "claude", "grok", "gemini", "deepseek", "glm", "kimi",
    ]
    .into_iter()
    .find(|family| {
        model == *family
            || model
                .strip_prefix(*family)
                .is_some_and(|suffix| suffix.starts_with('-'))
    })
}

pub fn all() -> &'static [Profile] {
    static PROFILES: OnceLock<Vec<Profile>> = OnceLock::new();
    PROFILES.get_or_init(|| {
        let mut profiles: Vec<Profile> =
            serde_json::from_str(include_str!("../profiles/index.json"))
                .expect("Embedded profiles must be valid JSON");
        let coldcoffee: Vec<Profile> =
            serde_json::from_str(include_str!("../profiles/coldcoffee/index.json"))
                .expect("Embedded Coldcoffee catalog must be valid JSON");
        for mut profile in coldcoffee {
            profile.instruction = match profile.model_family.as_str() {
                "claude" => include_str!("../profiles/coldcoffee/claude.md"),
                "grok" => include_str!("../profiles/coldcoffee/grok.md"),
                "gemini" => include_str!("../profiles/coldcoffee/gemini.md"),
                "deepseek" => include_str!("../profiles/coldcoffee/deepseek.md"),
                "glm" => include_str!("../profiles/coldcoffee/glm53.md"),
                _ => panic!("Unsupported Coldcoffee model family"),
            }
            .into();
            profiles.push(profile);
        }
        let kimi: Vec<Profile> =
            serde_json::from_str(include_str!("../profiles/kimi-break/index.json"))
                .expect("Embedded kimi-break catalog must be valid JSON");
        for mut profile in kimi {
            profile.instruction = include_str!("../profiles/kimi-break/system.md").into();
            profiles.push(profile);
        }
        let mut ids = std::collections::HashSet::new();
        for profile in &mut profiles {
            assert!(ids.insert(profile.id.clone()), "Profile IDs must be unique");
            let family = profile
                .models
                .first()
                .and_then(|model| model_family(model))
                .expect("Embedded profiles must declare a supported model family");
            assert!(
                profile
                    .models
                    .iter()
                    .all(|model| model_family(model) == Some(family)),
                "A profile must target exactly one model family"
            );
            assert!(!profile.instruction.is_empty());
            profile.model_family = family.into();
        }
        profiles
    })
}

pub fn find(id: &str) -> Option<&'static Profile> {
    all().iter().find(|profile| profile.id == id)
}

// Only exact historical presets migrate; discarded custom fields never become active.
pub(crate) fn legacy_profile(models: &[String], instruction: &str) -> Option<&'static Profile> {
    all().iter().find(|profile| {
        instruction == profile.instruction
            && models.len() == profile.models.len()
            && models.iter().all(|model| profile.models.contains(model))
    })
}

pub fn instruction(group: &Group, model: &str) -> Option<&'static str> {
    group
        .profile_ids
        .iter()
        .filter_map(|id| find(id))
        .find(|profile| profile.models.iter().any(|selected| selected == model))
        .map(|profile| profile.instruction.as_str())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn families_have_exact_or_hyphen_boundaries() {
        for (model, expected) in [
            ("gpt-5.6", Some("gpt-5.6")),
            ("gpt-5.6-sol", Some("gpt-5.6")),
            ("gpt-6", Some("gpt-6")),
            ("gpt-6-astra", Some("gpt-6")),
            ("gpt-6-sol", Some("gpt-6")),
            ("gpt-6.1", Some("gpt-6.1")),
            ("gpt-6.1-sol", Some("gpt-6.1")),
            ("gpt-6.10-sol", None),
            ("gpt-60", None),
            ("gpt-5.60-sol", None),
            ("other-gpt-6", None),
            ("GPT-6", None),
            ("claude-opus-5-5", Some("claude")),
            ("grok-4.7", Some("grok")),
            ("gemini-3.1-pro-preview", Some("gemini")),
            ("deepseek-v4.1-flash", Some("deepseek")),
            ("glm-5.3", Some("glm")),
            ("kimi-k2.7-code", Some("kimi")),
            ("kimilike-k3", None),
            ("claudelike-5", None),
            ("grokish", None),
            ("Gemini-3.1-pro", None),
        ] {
            assert_eq!(model_family(model), expected, "{model}");
        }
    }

    #[test]
    fn coldcoffee_catalog_has_exact_requested_models_and_resolved_skills() {
        let entries: Vec<serde_json::Value> =
            serde_json::from_str(include_str!("../profiles/coldcoffee/index.json")).unwrap();
        assert_eq!(entries.len(), 28);
        assert_eq!(all().len(), 35);
        let mut models = std::collections::HashSet::new();
        for entry in entries {
            let profile = find(entry["id"].as_str().unwrap()).unwrap();
            assert_eq!(profile.models.len(), 1);
            let model = &profile.models[0];
            assert!(!model.contains("svg"));
            assert!(
                models.insert(model.clone()),
                "duplicate exact model {model}"
            );
            assert!(profile.instruction.contains("ZakoAI · 冷咖啡 API 内联适配"));
            assert!(profile.instruction.contains("三拍"));
            assert!(profile.instruction.contains("CHA-ROUTE"));
            assert!(!profile.instruction.contains("{{"));
            assert!(!profile.instruction.contains("127.0.0.1:13337"));
            let group = Group {
                profile_ids: vec![profile.id.clone()],
                ..Default::default()
            };
            assert!(instruction(&group, model).is_some());
            assert!(instruction(&group, &format!("{model}-unlisted")).is_none());
        }
        assert!(models.contains("gemini-3.1-flash-image"));
        assert!(models.contains("claude-fable-5-1"));
        assert!(models.contains("deepseek-v4-pro-0813"));
        assert!(models.contains("glm-5.3"));
    }

    #[test]
    fn legacy_migration_only_recognizes_unchanged_presets() {
        for profile in all() {
            let group: Group = serde_json::from_value(serde_json::json!({
                "name":"Legacy", "models":profile.models, "instruction":profile.instruction
            }))
            .unwrap();
            assert_eq!(
                group.profile_ids.as_slice(),
                std::slice::from_ref(&profile.id)
            );
            let serialized = serde_json::to_value(&group).unwrap();
            assert!(serialized.get("models").is_none());
            assert!(serialized.get("instruction").is_none());

            let custom: Group = serde_json::from_value(serde_json::json!({
                "name":"Custom", "models":["custom-model"], "instruction":"Custom text"
            }))
            .unwrap();
            assert!(custom.profile_ids.is_empty());
            assert!(instruction(&custom, "custom-model").is_none());
        }
    }
}
