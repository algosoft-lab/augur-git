use super::RemoteEntry;

/// Read raw config values so insteadOf rewrites never become editable URLs.
pub(super) fn parse(text: &str) -> Vec<RemoteEntry> {
    let mut remotes: Vec<RemoteEntry> = Vec::new();
    let mut seen_urls = std::collections::HashSet::new();
    for record in text.split('\0') {
        let Some((key, value)) = record.split_once('\n') else {
            continue;
        };
        let Some(key) = key.strip_prefix("remote.") else {
            continue;
        };
        let (name, push) = if let Some(name) = key.strip_suffix(".pushurl") {
            (name, true)
        } else if let Some(name) = key.strip_suffix(".url") {
            (name, false)
        } else {
            continue;
        };
        let index = remotes.iter().position(|remote| remote.name == name);
        let remote = if let Some(index) = index {
            &mut remotes[index]
        } else {
            remotes.push(RemoteEntry {
                name: name.into(),
                ..RemoteEntry::default()
            });
            remotes.last_mut().unwrap()
        };
        if push {
            remote.has_push_url = true;
        } else if seen_urls.insert(name.to_string()) {
            remote.url = value.into();
        }
    }
    remotes
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn remote_config_preserves_raw_first_url_and_push_presence() {
        let text = "remote.origin.url\nshort:Org/repo.git\0remote.origin.url\nextra\0remote.origin.pushurl\nssh://host/repo\0remote.team.upstream.url\n/path with spaces\0";
        assert_eq!(
            parse(text),
            vec![
                RemoteEntry {
                    name: "origin".into(),
                    url: "short:Org/repo.git".into(),
                    has_push_url: true
                },
                RemoteEntry {
                    name: "team.upstream".into(),
                    url: "/path with spaces".into(),
                    has_push_url: false
                },
            ]
        );
    }
}
