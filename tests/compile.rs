//! Compile-fail checks for unsupported handler types.
//!
//! Regenerate the expected diagnostics after an intentional change:
//!
//! ```text
//! TRYBUILD=overwrite cargo test --test compile
//! ```

#[test]
fn unsupported_handler_types_fail_with_readable_diagnostics() {
    let tests = trybuild::TestCases::new();
    tests.compile_fail("tests/ui/unsupported_response.rs");
    tests.compile_fail("tests/ui/unsupported_extractor.rs");
}
