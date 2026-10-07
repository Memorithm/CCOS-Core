use std::collections::BTreeSet;

const MANIFEST: &str = include_str!("../Cargo.toml");
const README: &str = include_str!("../README.md");
const MATRIX: &str = include_str!("../docs/CORE_CAPABILITIES.md");

const EXTERNAL_FEATURE_NAMES: &[&str] = &[
    "slhav2-full",
    "octasoma",
    "octacore",
    "rsi",
    "rsi-dgm",
    "rsi-full",
    "pro-default",
    "all-full",
];

fn feature_section() -> &'static str {
    let (_, after_header) = MANIFEST
        .split_once("\n[features]\n")
        .expect("Cargo.toml must define [features]");
    after_header.split("\n[").next().unwrap_or(after_header)
}

fn defines_feature(section: &str, feature: &str) -> bool {
    section
        .lines()
        .map(str::trim)
        .any(|line| line.starts_with(&format!("{feature} =")))
}

fn manifest_features(section: &'static str) -> BTreeSet<&'static str> {
    section
        .lines()
        .filter_map(|line| {
            let line = line.trim();
            let (name, _) = line.split_once(" =")?;
            (name != "default").then_some(name)
        })
        .collect()
}

fn matrix_features() -> BTreeSet<&'static str> {
    MATRIX
        .lines()
        .filter_map(|line| {
            let rest = line.strip_prefix("| `")?;
            let (name, _) = rest.split_once("` |")?;
            Some(name)
        })
        .collect()
}

#[test]
fn capability_matrix_matches_the_manifest_and_product_boundary() {
    let features = feature_section();
    assert_eq!(
        manifest_features(features),
        matrix_features(),
        "capability matrix must enumerate every non-default Cargo feature exactly"
    );

    for feature in EXTERNAL_FEATURE_NAMES {
        assert!(
            !defines_feature(features, feature),
            "external Research feature {feature} leaked into Core"
        );
    }

    assert!(
        README.contains("This repository is **CCOS Core**"),
        "README must state the Core product boundary"
    );
    assert!(
        !README.contains("This repository is **CCOS_EXTENDED**"),
        "README must not claim the retired fused product"
    );
    assert!(
        MATRIX.contains("The workspace contains only `ccos-core`"),
        "matrix must identify the manifest-derived workspace boundary"
    );
}
