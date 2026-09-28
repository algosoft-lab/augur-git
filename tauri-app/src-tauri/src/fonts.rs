//! System font family discovery for the appearance settings.

use std::collections::BTreeSet;

/// Families available on this machine, sorted and de-duplicated.
pub fn font_families() -> Vec<String> {
    let mut database = fontdb::Database::new();
    database.load_system_fonts();
    unique_families(
        database
            .faces()
            .flat_map(|face| face.families.iter().map(|(name, _)| name.as_str())),
    )
}

fn unique_families<'a>(names: impl IntoIterator<Item = &'a str>) -> Vec<String> {
    names
        .into_iter()
        .map(str::trim)
        .filter(|name| !name.is_empty())
        .map(str::to_owned)
        .collect::<BTreeSet<_>>()
        .into_iter()
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn family_names_preserve_spaces_and_deduplicate_variants() {
        let names = unique_families([
            "Source Sans 3",
            "Fira Code",
            "Source Sans 3",
            "  Fira Code  ",
            "",
        ]);
        assert_eq!(names, ["Fira Code", "Source Sans 3"]);
    }

    #[test]
    fn system_families_are_sorted_and_unique() {
        let families = font_families();
        let mut sorted = families.clone();
        sorted.sort();
        sorted.dedup();
        assert_eq!(families, sorted);
        assert!(families.iter().all(|name| !name.is_empty()));
    }
}
