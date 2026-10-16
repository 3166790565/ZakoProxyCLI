use crate::store::Group;
use serde::{Deserialize, Serialize};
use std::sync::OnceLock;

#[derive(Deserialize, Serialize)]
pub struct Profile {
    pub id: String,
    pub name: String,
    pub models: Vec<String>,
    pub instruction: String,
    #[serde(default)]
    pub model_family: String,
}

pub fn model_family(model: &str) -> Option<&'static str> {
    ["gpt-5.6", "gpt-6.1", "gpt-6"].into_iter().find(|family| {
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
        for profile in &mut profiles {
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

// Only the exact old preset representation is migrated; custom text stays untouched.
pub(crate) fn normalize_legacy_group(group: &mut Group) {
    if !group.profile_ids.is_empty() {
        return;
    }
    if let Some(profile) = all().iter().find(|profile| {
        group.instruction == profile.instruction
            && group.models.len() == profile.models.len()
            && group
                .models
                .iter()
                .all(|model| profile.models.contains(model))
    }) {
        group.profile_ids.push(profile.id.clone());
        group.models.clear();
        group.instruction.clear();
    }
}

pub fn instruction<'a>(group: &'a Group, model: &str) -> Option<&'a str> {
    if let Some(profile) = group
        .profile_ids
        .iter()
        .filter_map(|id| find(id))
        .find(|profile| profile.models.iter().any(|selected| selected == model))
    {
        return Some(&profile.instruction);
    }
    if group.instruction.is_empty() || !group.models.iter().any(|selected| selected == model) {
        return None;
    }
    if all()
        .iter()
        .any(|profile| profile.models.iter().any(|supported| supported == model))
    {
        return None;
    }
    // Built-in text is enabled only through profile_ids, never through a stale legacy copy.
    if all()
        .iter()
        .any(|profile| profile.instruction == group.instruction)
    {
        return None;
    }
    Some(&group.instruction)
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
        ] {
            assert_eq!(model_family(model), expected, "{model}");
        }
    }

    #[test]
    fn legacy_migration_only_recognizes_unchanged_presets() {
        for profile in all() {
            let mut group = Group {
                models: profile.models.clone(),
                instruction: profile.instruction.clone(),
                ..Default::default()
            };
            normalize_legacy_group(&mut group);
            assert_eq!(
                group.profile_ids.as_slice(),
                std::slice::from_ref(&profile.id)
            );
            assert!(group.models.is_empty());
            assert!(group.instruction.is_empty());

            let mut custom = Group {
                models: vec!["custom-model".into()],
                instruction: profile.instruction.clone(),
                ..Default::default()
            };
            normalize_legacy_group(&mut custom);
            assert!(custom.profile_ids.is_empty());
            assert!(instruction(&custom, "custom-model").is_none());
            custom.instruction.push_str(" custom");
            assert_eq!(
                instruction(&custom, "custom-model"),
                Some(custom.instruction.as_str())
            );
        }
    }
}
