//! How extractors contribute to an operation.
//!
//! Implement [`ApiInput`] for your own extractors to teach the generator about
//! them. Built-in extractors with no static API surface (state, headers,
//! extensions, request metadata) deliberately add nothing.

use axum::body::Bytes;
use axum::extract::{ConnectInfo, Extension, Path, Query, RawQuery, Request, State};
use axum::http::{HeaderMap, Method, Uri};
use axum::Json;
use schemars::JsonSchema;

use crate::operation::OperationDescriptor;

/// An extractor whose presence or shape can be documented.
///
/// This trait is implemented for the extractors the generator understands.
/// A custom extractor must provide an implementation when it should appear in
/// the document; there is no blanket fallback, so unsupported types fail
/// loudly at compile time instead of silently producing incomplete docs.
#[diagnostic::on_unimplemented(
    message = "`{Self}` cannot be described in the OpenAPI document",
    label = "this extractor must implement `ApiInput`",
    note = "implement `axum_auto_openapi::ApiInput` for your extractor, or use `Json<T>`, `Path<T>`, `Query<T>` or `State<T>`"
)]
pub trait ApiInput {
    /// Add this extractor's metadata to an operation.
    fn describe(operation: &mut OperationDescriptor);
}

impl<T: JsonSchema + 'static> ApiInput for Json<T> {
    fn describe(operation: &mut OperationDescriptor) {
        operation.describe_json_body::<T>();
    }
}

impl<T: JsonSchema + 'static> ApiInput for Path<T> {
    fn describe(operation: &mut OperationDescriptor) {
        operation.describe_path::<T>();
    }
}

impl<T: JsonSchema + 'static> ApiInput for Query<T> {
    fn describe(operation: &mut OperationDescriptor) {
        operation.describe_query::<T>();
    }
}

impl<T> ApiInput for State<T> {
    fn describe(_: &mut OperationDescriptor) {}
}

impl<T> ApiInput for Extension<T> {
    fn describe(_: &mut OperationDescriptor) {}
}

impl ApiInput for HeaderMap {
    fn describe(_: &mut OperationDescriptor) {}
}

impl ApiInput for Method {
    fn describe(_: &mut OperationDescriptor) {}
}

impl ApiInput for Uri {
    fn describe(_: &mut OperationDescriptor) {}
}

impl ApiInput for RawQuery {
    fn describe(_: &mut OperationDescriptor) {}
}

impl<T> ApiInput for ConnectInfo<T> {
    fn describe(_: &mut OperationDescriptor) {}
}

impl ApiInput for Request {
    fn describe(_: &mut OperationDescriptor) {}
}

impl ApiInput for Bytes {
    fn describe(operation: &mut OperationDescriptor) {
        operation.describe_raw_body(
            "application/octet-stream",
            true,
            serde_json::json!({ "type": "string", "format": "binary" }),
        );
    }
}

impl ApiInput for String {
    fn describe(operation: &mut OperationDescriptor) {
        operation.describe_raw_body("text/plain", true, serde_json::json!({ "type": "string" }));
    }
}

impl<T: ApiInput> ApiInput for Option<T> {
    fn describe(operation: &mut OperationDescriptor) {
        T::describe(operation);
    }
}
