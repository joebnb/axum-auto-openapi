//! Golden-file test for the generated document.
//!
//! Regenerate the fixture after an intentional change:
//!
//! ```text
//! UPDATE_GOLDEN=1 cargo test --test golden
//! ```

mod common;

const GOLDEN: &str = include_str!("fixtures/basic.json");

#[test]
fn generated_document_matches_the_golden_file() {
    let (_, document) = common::app();
    let actual = document.to_json_pretty();

    if std::env::var_os("UPDATE_GOLDEN").is_some() {
        let path = concat!(env!("CARGO_MANIFEST_DIR"), "/tests/fixtures/basic.json");
        std::fs::write(path, format!("{actual}\n")).unwrap();
        return;
    }

    assert_eq!(
        actual.trim_end(),
        GOLDEN.trim_end(),
        "the generated document changed; run `UPDATE_GOLDEN=1 cargo test --test golden` \
         and review the diff"
    );
}
