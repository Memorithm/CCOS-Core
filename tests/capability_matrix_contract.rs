const MANIFEST: &str = include_str!("../Cargo.toml");
const README: &str = include_str!("../README.md");
const MATRIX: &str = include_str!("../docs/CORE_CAPABILITIES.md");

const SHIPPED_FEATURES: &[&str] = &[
    "syn-parser",
    "llm",
    "mimalloc",
    "learned-embed",
    "license",
    "license-pq",
    "signed-sync",
    "slhav2",
    "neural-embed",
];

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
        .split_once("[features]")
        .expect("Cargo.toml must define [features]");
    after_header.split("\n[").next().unwrap_or(after_header)
}

fn defines_feature(section: &str, feature: &str) -> bool {
    section
        .lines()
        .map(str::trim)
        .any(|line| line.starts_with(&format!("{feature} =")))
}

#[test]
fn capability_matrix_matches_the_manifest_and_product_boundary() {
    let features = feature_section();

    for feature in SHIPPED_FEATURES {
        assert!(
            defines_feature(features, feature),
            "matrix names missing Cargo feature {feature}"
        );
        assert!(
            MATRIX.contains(&format!("| `{feature}` |")),
            "matrix is missing shipped feature {feature}"
        );
    }

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
