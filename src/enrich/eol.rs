use super::{RegistryDb, VersionLifecycle};

/// Build a built-in registry database with known EOL dates for common runtimes.
/// This serves as a fallback when no cache file is present.
pub fn builtin_registry() -> RegistryDb {
    let mut db = RegistryDb::default();

    db.families.insert(
        "node".into(),
        vec![
            VersionLifecycle {
                cycle: "16".into(),
                release_date: Some("2021-04-20".into()),
                eol_date: Some("2023-09-11".into()),
                lts: true,
                latest_patch: Some("16.20.2".into()),
            },
            VersionLifecycle {
                cycle: "18".into(),
                release_date: Some("2022-04-19".into()),
                eol_date: Some("2025-04-30".into()),
                lts: true,
                latest_patch: Some("18.20.4".into()),
            },
            VersionLifecycle {
                cycle: "20".into(),
                release_date: Some("2023-04-18".into()),
                eol_date: Some("2026-04-30".into()),
                lts: true,
                latest_patch: Some("20.18.0".into()),
            },
            VersionLifecycle {
                cycle: "22".into(),
                release_date: Some("2024-04-24".into()),
                eol_date: Some("2027-04-30".into()),
                lts: true,
                latest_patch: Some("22.11.0".into()),
            },
        ],
    );

    db.families.insert(
        "python".into(),
        vec![
            VersionLifecycle {
                cycle: "3.8".into(),
                release_date: Some("2019-10-14".into()),
                eol_date: Some("2024-10-07".into()),
                lts: false,
                latest_patch: Some("3.8.20".into()),
            },
            VersionLifecycle {
                cycle: "3.9".into(),
                release_date: Some("2020-10-05".into()),
                eol_date: Some("2025-10-05".into()),
                lts: false,
                latest_patch: Some("3.9.21".into()),
            },
            VersionLifecycle {
                cycle: "3.10".into(),
                release_date: Some("2021-10-04".into()),
                eol_date: Some("2026-10-04".into()),
                lts: false,
                latest_patch: Some("3.10.16".into()),
            },
            VersionLifecycle {
                cycle: "3.11".into(),
                release_date: Some("2022-10-24".into()),
                eol_date: Some("2027-10-24".into()),
                lts: false,
                latest_patch: Some("3.11.11".into()),
            },
            VersionLifecycle {
                cycle: "3.12".into(),
                release_date: Some("2023-10-02".into()),
                eol_date: Some("2028-10-02".into()),
                lts: false,
                latest_patch: Some("3.12.8".into()),
            },
            VersionLifecycle {
                cycle: "3.13".into(),
                release_date: Some("2024-10-07".into()),
                eol_date: Some("2029-10-07".into()),
                lts: false,
                latest_patch: Some("3.13.1".into()),
            },
        ],
    );

    db.updated_at = Some("2025-01-01T00:00:00Z".into());
    db
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_builtin_registry_has_node() {
        let db = builtin_registry();
        assert!(db.families.contains_key("node"));
        let node = &db.families["node"];
        assert!(node.iter().any(|lc| lc.cycle == "22"));
    }

    #[test]
    fn test_builtin_registry_has_python() {
        let db = builtin_registry();
        assert!(db.families.contains_key("python"));
        let python = &db.families["python"];
        assert!(python.iter().any(|lc| lc.cycle == "3.12"));
    }
}
