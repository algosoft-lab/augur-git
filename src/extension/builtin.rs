//! First-party extension packages compiled into the application.

use crate::core::extension::{
    ExtensionManifest, ExtensionPackage, ExtensionSource,
};

use super::manager::ExtensionDefinition;

const SYNC_MANIFEST: &str =
    include_str!("../../extensions/sync-open-tabs/manifest.toml");
const SYNC_SOURCE: &str =
    include_str!("../../extensions/sync-open-tabs/main.lua");

/// Return the read-only, pre-trusted first-party extension definitions.
pub fn bundled_definitions() -> Result<Vec<ExtensionDefinition>, String> {
    let manifest = ExtensionManifest::parse(SYNC_MANIFEST)
        .map_err(|error| error.to_string())?;
    Ok(vec![ExtensionDefinition {
        package: ExtensionPackage {
            manifest,
            root: None,
            source: ExtensionSource::Bundled,
            fingerprint: "bundled-sync-open-tabs-v1".to_string(),
            bundled: true,
        },
        source: SYNC_SOURCE.to_string(),
    }])
}

#[cfg(test)]
mod tests {
    use super::bundled_definitions;

    #[test]
    fn bundled_sync_extension_is_valid() {
        let definitions = bundled_definitions().expect("bundled manifest");
        assert_eq!(definitions.len(), 1);
        assert_eq!(definitions[0].package.manifest.id, "sync-open-tabs");
    }
}
