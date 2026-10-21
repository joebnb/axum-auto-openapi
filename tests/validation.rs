//! Parse generated documents with an OpenAPI 3.1 implementation.
//!
//! [`oas3`] deserializes the complete OpenAPI 3.1 object model. A successful
//! parse therefore catches missing required fields, invalid operation shapes,
//! and wrongly typed values that structural JSON assertions can miss.

mod common;

use axum::extract::Path;
use axum::http::StatusCode;
use axum::Json;
use axum_auto_openapi::routing::{get, post};
use axum_auto_openapi::{OpenApiDocument, Router};
use serde_json::Value;

fn validate(document: &OpenApiDocument) {
    let spec = oas3::from_json(document.to_json())
        .unwrap_or_else(|error| panic!("document is not valid OpenAPI 3.1: {error}\n{document}"));
    let version = spec
        .validate_version()
        .unwrap_or_else(|error| panic!("invalid OpenAPI version: {error}"));
    assert_eq!((version.major, version.minor), (3, 1));

    assert_internal_refs_resolve(document.as_value(), document.as_value());
}

/// Every `$ref` in the document must point at an existing component.
fn assert_internal_refs_resolve(root: &Value, value: &Value) {
    match value {
        Value::Object(object) => {
            if let Some(reference) = object.get("$ref").and_then(Value::as_str) {
                let pointer = reference
                    .strip_prefix('#')
                    .unwrap_or_else(|| panic!("external $ref is not expected: {reference}"));
                assert!(
                    root.pointer(pointer).is_some(),
                    "unresolved $ref `{reference}`"
                );
            }
            for nested in object.values() {
                assert_internal_refs_resolve(root, nested);
            }
        }
        Value::Array(items) => {
            for nested in items {
                assert_internal_refs_resolve(root, nested);
            }
        }
        _ => {}
    }
}

#[test]
fn basic_document_is_valid_openapi_3_1() {
    let (_, document) = common::app();
    validate(&document);
}

#[test]
fn nested_stateful_and_edge_case_documents_are_valid() {
    let (_, nested) = common::nested_app();
    validate(&nested);

    let (_, stateful) = common::stateful_app();
    validate(&stateful);

    async fn files(Path(rest): Path<String>) -> String {
        rest
    }

    async fn create() -> (StatusCode, Json<common::User>) {
        (
            StatusCode::CREATED,
            Json(common::User {
                id: 1,
                name: "new".to_owned(),
            }),
        )
    }

    let (_, edge_cases) = Router::<()>::new()
        .route("/files/{*rest}", get(files))
        .route("/things", post(create))
        .openapi_json_route("/openapi.json")
        .finish();
    validate(&edge_cases);
}
