//! Zero-annotation OpenAPI 3.1 generation for [Axum] 0.8.
//!
//! `axum-auto-openapi` replaces the router registration layer. Handlers stay
//! ordinary axum handlers; the extractor and response types are analyzed when
//! routes are registered, and the document is rendered once with
//! [`Router::finish`].
//!
//! ```
//! use axum::Json;
//! use axum_auto_openapi::routing::{get, post};
//! use axum_auto_openapi::Router;
//! use schemars::JsonSchema;
//! use serde::{Deserialize, Serialize};
//!
//! #[derive(Serialize, JsonSchema)]
//! struct User {
//!     id: u64,
//!     name: String,
//! }
//!
//! #[derive(Deserialize, JsonSchema)]
//! struct CreateUser {
//!     name: String,
//! }
//!
//! async fn get_user(axum::extract::Path(id): axum::extract::Path<u64>) -> Json<User> {
//!     Json(User { id, name: "Ada".into() })
//! }
//!
//! async fn create_user(Json(input): Json<CreateUser>) -> Json<User> {
//!     Json(User { id: 1, name: input.name })
//! }
//!
//! let (app, document) = Router::<()>::new()
//!     .route("/users", post(create_user))
//!     .route("/users/{id}", get(get_user))
//!     .finish();
//!
//! assert_eq!(document.as_value()["openapi"], "3.1.0");
//! assert_eq!(
//!     document.as_value()["paths"]["/users/{id}"]["get"]["parameters"][0]["name"],
//!     "id"
//! );
//! # let _ = app;
//! ```
//!
//! ## What is captured automatically
//!
//! - `Json<T>` request bodies and responses.
//! - `Path<T>` parameters, including scalars, tuples, and structs.
//! - `Query<T>` object fields, optionality, arrays, and nested objects.
//! - `Result<T, E>` success and error responses.
//! - `(StatusCode, T)` responses with runtime-selected status codes.
//! - [`ApiResponse<T, STATUS>`](ApiResponse) for a status code fixed at
//!   registration time; the body is still built by axum.
//! - [`ApiResponseDefault<T>`](ApiResponseDefault) for a runtime-selected
//!   status with a typed body.
//! - `State<T>`, `Extension<T>`, header/request metadata and custom `ApiInput`
//!   implementations.
//!
//! Business-specific status codes, security requirements, summaries and tags
//! can be added fluently on [`routing::ApiMethodRouter`] or through
//! [`OperationDescriptor`]. Tags are derived from the router tree by default
//! ([`Router::without_infer_tags`] turns that off), and a handler that parses
//! its own `Bytes` body can declare the real shape with
//! [`routing::ApiMethodRouter::json_body`].
//!
//! Documentation is published without wiring: the JSON document is served at
//! [`DEFAULT_DOCUMENT_PATH`] and, with the `swagger-ui` feature, the UI at
//! [`DEFAULT_DOCS_PATH`]. Use [`Router::documentation`] to withhold them, or
//! [`Router::openapi_json_route`] / [`Router::swagger_ui`] to change the
//! paths.
//!
//! ## What is not captured
//!
//! - Branch conditions inside a handler.
//! - Concrete status codes inside a `StatusCode` value.
//! - Opaque `impl IntoResponse` return types (these fail to compile instead of
//!   silently producing an incomplete document).
//! - Services registered with `route_service`, and fallback routes.
//!
//! [Axum]: https://docs.rs/axum

mod extract;
mod handler;
mod openapi;
mod operation;
mod params;
mod response;
mod router;
mod schema;

pub mod routing;

pub use extract::ApiInput;
pub use handler::AutoHandler;
pub use openapi::{ApiKeyLocation, Info, OpenApiDocument, SecurityScheme, Server, Tag};
pub use operation::{OperationDescriptor, ResponseKind, StatusKey};
pub use response::{ApiOutput, ApiResponse, ApiResponseDefault};
pub use router::{Router, DEFAULT_DOCS_PATH, DEFAULT_DOCUMENT_PATH};
pub use routing::ApiMethodRouter;

/// Re-exported for convenience so applications can use the same axum version.
pub use axum;
/// Re-exported so derive macros and schema generation stay on one version.
pub use schemars;
pub use schemars::JsonSchema;
