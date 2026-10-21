//! How handler return values contribute to an operation.
//!
//! The important consequence of requiring [`ApiOutput`] is that a handler
//! returning an opaque `impl IntoResponse` cannot silently produce an empty
//! response description. Such handlers fail to compile; return a concrete
//! type or implement `ApiOutput` for your own response type.

use std::marker::PhantomData;

use axum::body::Bytes;
use axum::http::StatusCode;
use axum::response::{Html, IntoResponse, Response};
use axum::Json;
use schemars::JsonSchema;
use serde::Serialize;
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

/// A handler return type that declares the status code in the type and
/// delegates the actual response to axum.
///
/// `T` is whatever the handler would have returned anyway — `Json<T>`,
/// `Html<String>`, `Bytes`, a fully built [`Response`] — so this type adds no
/// response-building logic of its own. It only supplies the two facts the
/// document cannot infer from a bare [`Response`]: the status code, as the
/// const parameter, and the body shape, through `T`'s [`ApiOutput`].
///
/// ```rust
/// # use axum::Json;
/// # use axum::response::{IntoResponse, Response};
/// # use axum_auto_openapi::ApiResponse;
/// # use schemars::JsonSchema;
/// # use serde::Serialize;
/// # #[derive(Serialize, JsonSchema)]
/// # struct Device { id: u64 }
/// async fn create() -> ApiResponse<Json<Device>, 201> {
///     ApiResponse::new(Json(Device { id: 1 }))
/// }
///
/// async fn redirect() -> ApiResponse<Response, 302> {
///     ApiResponse::new(axum::response::Redirect::temporary("/login").into_response())
/// }
/// ```
///
/// The status code is a const parameter because the document is rendered at
/// startup, before any response value exists. A status only known at runtime
/// stays `(StatusCode, T)`, or [`ApiResponseDefault`] when the body is still
/// typed.
///
/// # What to watch
///
/// - The wrapper replaces the status produced by `T`, including the error
///   branch of a `Result`. Pass one response type; never `Result<T, E>`.
/// - Adopting a [`Response`] discards any status it already carries.
/// - `204` and `304` must not carry a body, and the document does not enforce
///   HTTP semantics. Declare those statuses with `()`.
pub struct ApiResponse<T, const STATUS: u16 = 200> {
    body: T,
}

impl<T, const STATUS: u16> ApiResponse<T, STATUS> {
    /// Declare `body` as the `STATUS` response.
    pub fn new(body: T) -> Self {
        Self { body }
    }
}

impl<T, const STATUS: u16> IntoResponse for ApiResponse<T, STATUS>
where
    T: IntoResponse,
{
    fn into_response(self) -> Response {
        let status = StatusCode::from_u16(STATUS)
            .expect("ApiResponse status must be a valid HTTP status code");
        let mut response = self.body.into_response();
        *response.status_mut() = status;
        response
    }
}

impl<T, const STATUS: u16> ApiOutput for ApiResponse<T, STATUS>
where
    T: ApiOutput,
{
    fn describe(operation: &mut OperationDescriptor, _: ResponseKind) {
        debug_assert!(
            StatusCode::from_u16(STATUS).is_ok(),
            "ApiResponse status must be a valid HTTP status code"
        );
        T::describe(operation, ResponseKind::Status(STATUS));
    }
}

impl<T, const STATUS: u16> ApiResponse<Json<T>, STATUS>
where
    T: Serialize,
{
    /// Declare a JSON body without spelling out `Json` twice.
    ///
    /// ```rust
    /// # use axum_auto_openapi::ApiResponse;
    /// # use schemars::JsonSchema;
    /// # use serde::Serialize;
    /// # #[derive(Serialize, JsonSchema)]
    /// # struct Device { id: u64 }
    /// async fn create() -> ApiResponse<axum::Json<Device>, 201> {
    ///     ApiResponse::json(Device { id: 1 })
    /// }
    /// ```
    pub fn json(value: T) -> Self {
        Self::new(Json(value))
    }
}

impl<T, const STATUS: u16> From<T> for ApiResponse<T, STATUS> {
    fn from(body: T) -> Self {
        Self::new(body)
    }
}

/// A response whose status is chosen at runtime while its body stays typed.
///
/// Use this instead of `(StatusCode, T)` when axum itself builds the response
/// (headers, cookies, redirects) but the document should still describe the
/// body. The document records it as the `default` response because the status
/// is only known when the handler runs.
///
/// ```rust
/// # use axum::http::StatusCode;
/// # use axum::Json;
/// # use axum_auto_openapi::ApiResponseDefault;
/// # use schemars::JsonSchema;
/// # use serde::Serialize;
/// # #[derive(Serialize, JsonSchema)]
/// # struct Device { id: u64 }
/// async fn create(accepted: bool) -> ApiResponseDefault<Json<Device>> {
///     let status = if accepted {
///         StatusCode::ACCEPTED
///     } else {
///         StatusCode::CREATED
///     };
///     ApiResponseDefault::new((status, Json(Device { id: 1 })))
/// }
/// ```
pub struct ApiResponseDefault<T> {
    response: Response,
    _body: PhantomData<fn() -> T>,
}

impl<T> ApiResponseDefault<T> {
    /// Document `body` as the `default` response.
    pub fn new(body: impl IntoResponse) -> Self {
        Self {
            response: body.into_response(),
            _body: PhantomData,
        }
    }
}

impl<T> IntoResponse for ApiResponseDefault<T> {
    fn into_response(self) -> Response {
        self.response
    }
}

impl<T: ApiOutput> ApiOutput for ApiResponseDefault<T> {
    fn describe(operation: &mut OperationDescriptor, _: ResponseKind) {
        T::describe(operation, ResponseKind::Dynamic);
    }
}
