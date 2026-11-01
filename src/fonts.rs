//! Catalog-only CJK fallback fonts, embedded independently of Windows language packs.
use crate::i18n::Language;
use eframe::egui::{FontData, FontDefinitions, FontFamily};

struct FontPair {
    region: &'static str,
    regular: &'static [u8],
    light: &'static [u8],
}

fn selected(language: Language) -> Option<FontPair> {
    match language {
        Language::ZhHans => Some(FontPair {
            region: "sc",
            regular: include_bytes!("../assets/fonts/nanodns-ui-sc-regular.otf"),
            light: include_bytes!("../assets/fonts/nanodns-ui-sc-light.otf"),
        }),
        Language::ZhHant => Some(FontPair {
            region: "tc",
            regular: include_bytes!("../assets/fonts/nanodns-ui-tc-regular.otf"),
            light: include_bytes!("../assets/fonts/nanodns-ui-tc-light.otf"),
        }),
        Language::Ja => Some(FontPair {
            region: "jp",
            regular: include_bytes!("../assets/fonts/nanodns-ui-jp-regular.otf"),
            light: include_bytes!("../assets/fonts/nanodns-ui-jp-light.otf"),
        }),
        Language::Ko => Some(FontPair {
            region: "kr",
            regular: include_bytes!("../assets/fonts/nanodns-ui-kr-regular.otf"),
            light: include_bytes!("../assets/fonts/nanodns-ui-kr-light.otf"),
        }),
        _ => None,
    }
}

fn append(fonts: &mut FontDefinitions, family: FontFamily, name: &str) {
    let names = fonts.families.entry(family).or_default();
    if !names.iter().any(|existing| existing == name) {
        names.push(name.to_owned());
    }
}

/// Add the selected region's Regular/Light fallbacks after the existing fonts.
/// Call after creating console-display and before Context::set_fonts.
pub fn add_cjk(fonts: &mut FontDefinitions, language: Language) {
    if !language.is_cjk() {
        return;
    }
    let Some(pair) = selected(language) else {
        return;
    };
    let regular = format!("nanodns-cjk-{}-regular", pair.region);
    let light = format!("nanodns-cjk-{}-light", pair.region);
    fonts
        .font_data
        .insert(regular.clone(), FontData::from_static(pair.regular).into());
    fonts
        .font_data
        .insert(light.clone(), FontData::from_static(pair.light).into());
    append(fonts, FontFamily::Proportional, &regular);
    append(fonts, FontFamily::Monospace, &regular);
    append(fonts, FontFamily::Name("console-display".into()), &light);
}

#[cfg(test)]
mod tests {
    use super::{Language, add_cjk, selected};
    use eframe::egui;
    use std::collections::{HashMap, HashSet};

    #[test]
    fn non_cjk_definitions_are_unchanged_and_selected_families_are_idempotent() {
        for language in Language::ALL {
            let mut fonts = egui::FontDefinitions::default();
            let previous_data_len = fonts.font_data.len();
            let previous_families = fonts.families.clone();
            add_cjk(&mut fonts, language);
            if !language.is_cjk() {
                assert_eq!(fonts.font_data.len(), previous_data_len);
                assert_eq!(fonts.families, previous_families);
                continue;
            }
            let pair = selected(language).expect("CJK language must select a regional font pair");
            assert_eq!(fonts.font_data.len(), previous_data_len + 2);
            let regular = format!("nanodns-cjk-{}-regular", pair.region);
            let light = format!("nanodns-cjk-{}-light", pair.region);
            assert!(fonts.font_data.contains_key(&regular));
            assert!(fonts.font_data.contains_key(&light));
            let previous_families = fonts.families.clone();
            add_cjk(&mut fonts, language);
            assert_eq!(fonts.font_data.len(), previous_data_len + 2);
            assert_eq!(fonts.families, previous_families);
            for family in [egui::FontFamily::Proportional, egui::FontFamily::Monospace] {
                assert_eq!(
                    fonts.families.get(&family).and_then(|names| names.last()),
                    Some(&regular)
                );
            }
            assert_eq!(
                fonts
                    .families
                    .get(&egui::FontFamily::Name("console-display".into()))
                    .and_then(|names| names.last()),
                Some(&light)
            );
        }
    }

    #[test]
    fn egui_parses_embedded_fonts_and_renders_every_catalog_character_without_os_fonts()
    -> Result<(), serde_json::Error> {
        for (language, json) in [
            (Language::ZhHans, include_str!("../locales/zh-Hans.json")),
            (Language::ZhHant, include_str!("../locales/zh-Hant.json")),
            (Language::Ja, include_str!("../locales/ja.json")),
            (Language::Ko, include_str!("../locales/ko.json")),
        ] {
            let catalog: HashMap<String, String> = serde_json::from_str(json)?;
            let mut fonts = egui::FontDefinitions::default();
            // No Segoe/system fonts: this proves the embedded fallback works
            // when the selected Windows CJK language pack is not installed.
            add_cjk(&mut fonts, language);
            let ctx = egui::Context::default();
            ctx.set_fonts(fonts);
            let _ = ctx.run(egui::RawInput::default(), |ctx| {
                ctx.fonts_mut(|fonts| {
                    for family in [
                        egui::FontFamily::Proportional,
                        egui::FontFamily::Monospace,
                        egui::FontFamily::Name("console-display".into()),
                    ] {
                        let font_id = egui::FontId::new(16.0, family);
                        // has_glyph compares against the family's replacement
                        // glyph and reports false for '?' when it is the chosen
                        // replacement. The parsed character map includes that
                        // glyph and tests actual coverage without skipping it.
                        let supported: HashSet<_> = fonts
                            .fonts
                            .font(&font_id.family)
                            .characters()
                            .keys()
                            .copied()
                            .collect();
                        for character in catalog
                            .values()
                            .flat_map(|text| text.chars())
                            .filter(|character| !character.is_control())
                        {
                            assert!(
                                supported.contains(&character),
                                "{} missing U+{:04X} in {:?}",
                                language.tag(),
                                u32::from(character),
                                font_id.family
                            );
                        }
                        for text in catalog.values() {
                            let galley = fonts.layout_no_wrap(
                                text.clone(),
                                font_id.clone(),
                                egui::Color32::WHITE,
                            );
                            assert!(
                                galley.size().x > 0.0 && galley.size().y > 0.0,
                                "{} text must produce a visible layout in {:?}",
                                language.tag(),
                                font_id.family
                            );
                        }
                    }
                });
            });
        }
        Ok(())
    }
}
