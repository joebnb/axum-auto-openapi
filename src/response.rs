//! How handler return values contribute to an operation.
//!
//! The important consequence of requiring [`ApiOutput`] is that a handler
//! returning an opaque `impl IntoResponse` cannot silently produce an empty
//! response description. Such handlers fail to compile; return a concrete
//! type or implement `ApiOutput` for your own response type.

use axum::body::Bytes;
use axum::http::StatusCode;
use axum::response::{Html, Response};
use axum::Json;
use schemars::JsonSchema;
use serde_json::json;

use crate::operation::{OperationDescriptor, ResponseKind};

/// A handler return type that can be represented in OpenAPI.
///
/// For `Result<T, E>`, `T` is described with [`ResponseKind::Success`] and `E`
/// with [`ResponseKind::Error`]. A type that represents a runtime-selected
/// status code (such as `StatusCode` or `(StatusCode, Json<T>)`) should ignore
/// the kind it receives and document a `default` response.
#[diagnostic::on_unimplemented(
    message = "`{Self}` cannot be described in the OpenAPI document",
    label = "this handler return type must implement `ApiOutput`",
    note = "return `Json<T>`, `Result<T, E>`, `(StatusCode, Json<T>)`, `()` or implement `axum_auto_openapi::ApiOutput` for your wrapper"
)]
pub trait ApiOutput {
    /// Add this response to the operation.
    fn describe(operation: &mut OperationDescriptor, kind: ResponseKind);
}

impl<T: JsonSchema + 'static> ApiOutput for Json<T> {
    fn describe(operation: &mut OperationDescriptor, kind: ResponseKind) {
        operation.insert_response(
            kind.status_key().into(),
            None,
            "application/json",
            crate::operation::BodySchema::Typed(crate::schema::SchemaOf::of::<T>()),
        );
    }
}

impl ApiOutput for () {
    fn describe(operation: &mut OperationDescriptor, kind: ResponseKind) {
        operation.insert_empty_response(kind.status_key().into(), None);
    }
}

impl ApiOutput for StatusCode {
    fn describe(operation: &mut OperationDescriptor, _: ResponseKind) {
        operation.insert_empty_response(
            "default".into(),
            Some("Response with a runtime-selected status code".to_owned()),
        );
    }
}

impl<T: ApiOutput> ApiOutput for (StatusCode, T) {
    fn describe(operation: &mut OperationDescriptor, _: ResponseKind) {
        T::describe(operation, ResponseKind::Dynamic);
    }
}

impl<T: ApiOutput, E: ApiOutput> ApiOutput for Result<T, E> {
    fn describe(operation: &mut OperationDescriptor, _: ResponseKind) {
        T::describe(operation, ResponseKind::Success);
        E::describe(operation, ResponseKind::Error);
    }
}

impl ApiOutput for String {
    fn describe(operation: &mut OperationDescriptor, kind: ResponseKind) {
        operation.insert_response(
            kind.status_key().into(),
            None,
            "text/plain",
            crate::operation::BodySchema::Raw(json!({ "type": "string" })),
        );
    }
}

impl ApiOutput for &'static str {
    fn describe(operation: &mut OperationDescriptor, kind: ResponseKind) {
        operation.insert_response(
            kind.status_key().into(),
            None,
            "text/plain",
            crate::operation::BodySchema::Raw(json!({ "type": "string" })),
        );
    }
}

impl ApiOutput for Html<String> {
    fn describe(operation: &mut OperationDescriptor, kind: ResponseKind) {
        operation.insert_response(
            kind.status_key().into(),
            None,
            "text/html",
            crate::operation::BodySchema::Raw(json!({ "type": "string" })),
        );
    }
}

impl ApiOutput for Html<&'static str> {
    fn describe(operation: &mut OperationDescriptor, kind: ResponseKind) {
        operation.insert_response(
            kind.status_key().into(),
            None,
            "text/html",
            crate::operation::BodySchema::Raw(json!({ "type": "string" })),
        );
    }
}

impl ApiOutput for Bytes {
    fn describe(operation: &mut OperationDescriptor, kind: ResponseKind) {
        operation.insert_response(
            kind.status_key().into(),
            None,
            "application/octet-stream",
            crate::operation::BodySchema::Raw(json!({ "type": "string", "format": "binary" })),
        );
    }
}

impl ApiOutput for Response {
    fn describe(operation: &mut OperationDescriptor, kind: ResponseKind) {
        operation.insert_empty_response(kind.status_key().into(), None);
    }
}
