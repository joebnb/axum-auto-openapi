//! Method routers aware of OpenAPI metadata.
//!
//! These functions mirror [`axum::routing`] but additionally analyze the
//! handler signature. Replacing `use axum::routing::{get, post}` with
//! `use axum_openapi::routing::{get, post}` is the only change most handlers
//! need.

use axum::http::Method;
use axum::routing::MethodRouter as AxumMethodRouter;
use schemars::JsonSchema;

use crate::operation::{default_response_description, OperationDescriptor, StatusKey};
use crate::AutoHandler;

/// A method router that carries the OpenAPI operations registered on it.
///
/// Metadata setters such as [`summary`](Self::summary) apply to the most
/// recently added method. This allows fluent construction:
///
/// ```rust,no_run
/// # use axum_openapi::routing::get;
/// # async fn list() {}
/// # async fn create() {}
/// let router: axum_openapi::ApiMethodRouter = get(list)
///     .summary("List items")
///     .post(create)
///     .summary("Create an item");
/// # let _ = router;
/// ```
pub struct ApiMethodRouter<S = ()> {
    inner: AxumMethodRouter<S>,
    operations: Vec<(String, OperationDescriptor)>,
    last: usize,
}

impl<S> ApiMethodRouter<S> {
    fn current(&mut self) -> &mut OperationDescriptor {
        &mut self.operations[self.last].1
    }

    /// Set the summary of the most recently added operation.
    pub fn summary(mut self, summary: impl Into<String>) -> Self {
        self.current().summary(summary);
        self
    }

    /// Set the description of the most recently added operation.
    pub fn description(mut self, description: impl Into<String>) -> Self {
        self.current().description(description);
        self
    }

    /// Set the operation id of the most recently added operation.
    pub fn operation_id(mut self, operation_id: impl Into<String>) -> Self {
        self.current().operation_id(operation_id);
        self
    }

    /// Add a tag to the most recently added operation.
    pub fn tag(mut self, tag: impl Into<String>) -> Self {
        self.current().tag(tag);
        self
    }

    /// Require a registered security scheme for the most recently added operation.
    pub fn security(mut self, scheme: impl Into<String>) -> Self {
        self.current().security(scheme);
        self
    }

    /// Require a security scheme with explicit scopes.
    pub fn security_scopes(mut self, scheme: impl Into<String>, scopes: &[&str]) -> Self {
        self.current().security_scopes(scheme, scopes);
        self
    }

    /// Mark the most recently added operation as deprecated.
    pub fn deprecated(mut self, deprecated: bool) -> Self {
        self.current().deprecated(deprecated);
        self
    }

    /// Add a documented JSON response for the most recently added operation.
    pub fn response<T: JsonSchema + 'static>(mut self, status: impl Into<StatusKey>) -> Self {
        let status = status.into();
        let description = default_response_description(status.as_str());
        self.current().json_response::<T>(status, description);
        self
    }

    /// Add a documented JSON response with an explicit description.
    pub fn response_with<T: JsonSchema + 'static>(
        mut self,
        status: impl Into<StatusKey>,
        description: impl Into<String>,
    ) -> Self {
        self.current().json_response::<T>(status, description);
        self
    }

    /// Add a documented error response. Identical to [`response`](Self::response)
    /// but clearer at the call site.
    pub fn error_response<T: JsonSchema + 'static>(self, status: impl Into<StatusKey>) -> Self {
        self.response::<T>(status)
    }

    /// Add an empty response.
    pub fn empty_response(
        mut self,
        status: impl Into<StatusKey>,
        description: impl Into<String>,
    ) -> Self {
        self.current().empty_response(status, description);
        self
    }

    /// Add a response with an arbitrary content type and raw schema.
    pub fn raw_response(
        mut self,
        status: impl Into<StatusKey>,
        content_type: impl Into<String>,
        schema: serde_json::Value,
        description: impl Into<String>,
    ) -> Self {
        self.current()
            .raw_response(status, description, content_type, schema);
        self
    }

    /// Document a JSON request body with an explicit type.
    ///
    /// Handlers that read the body themselves (for example a `Bytes` body
    /// parsed after route-specific validation) are otherwise documented as
    /// `application/octet-stream`. Declaring the type here documents what the
    /// handler actually accepts and replaces any body inferred from the
    /// extractors.
    ///
    /// ```rust,no_run
    /// # use axum_openapi::routing::post;
    /// # use schemars::JsonSchema;
    /// # use serde::Deserialize;
    /// # #[derive(Deserialize, JsonSchema)]
    /// # struct Login { email: String }
    /// # async fn login(_: axum::body::Bytes) {}
    /// let route: axum_openapi::ApiMethodRouter = post(login).json_body::<Login>();
    /// # let _ = route;
    /// ```
    pub fn json_body<T: JsonSchema + 'static>(mut self) -> Self {
        self.current().describe_json_body::<T>();
        self
    }

    /// Replace the documented request body with an arbitrary content type and
    /// JSON Schema.
    ///
    /// Use this for webhooks and other bodies whose wire format is not
    /// expressed by the handler's extractors.
    pub fn raw_body(
        mut self,
        content_type: impl Into<String>,
        required: bool,
        schema: serde_json::Value,
    ) -> Self {
        self.current()
            .describe_raw_body(content_type, required, schema);
        self
    }

    pub(crate) fn into_parts(self) -> (AxumMethodRouter<S>, Vec<(String, OperationDescriptor)>) {
        (self.inner, self.operations)
    }
}

macro_rules! chain_method {
    ($name:ident, $method:expr, $key:literal) => {
        #[doc = concat!("Add a `", stringify!($name), "` handler to this method router.")]
        pub fn $name<H, T>(mut self, handler: H) -> Self
        where
            H: AutoHandler<T, S>,
            T: 'static,
            S: Clone + Send + Sync + 'static,
        {
            let mut operation = OperationDescriptor::new($method);
            <H as AutoHandler<T, S>>::describe(&mut operation);
            self.operations.push(($key.to_owned(), operation));
            self.last = self.operations.len() - 1;
            self.inner = self.inner.$name(handler);
            self
        }
    };
}

impl<S> ApiMethodRouter<S>
where
    S: Clone + Send + Sync + 'static,
{
    chain_method!(delete, Method::DELETE, "delete");
    chain_method!(get, Method::GET, "get");
    chain_method!(head, Method::HEAD, "head");
    chain_method!(options, Method::OPTIONS, "options");
    chain_method!(patch, Method::PATCH, "patch");
    chain_method!(post, Method::POST, "post");
    chain_method!(put, Method::PUT, "put");
    chain_method!(trace, Method::TRACE, "trace");
}

macro_rules! top_level_fn {
    ($name:ident, $method:expr, $key:literal) => {
        #[doc = concat!("Route `", stringify!($name), "` requests to the given handler.")]
        pub fn $name<H, T, S>(handler: H) -> ApiMethodRouter<S>
        where
            H: AutoHandler<T, S>,
            T: 'static,
            S: Clone + Send + Sync + 'static,
        {
            let mut operation = OperationDescriptor::new($method);
            <H as AutoHandler<T, S>>::describe(&mut operation);
            ApiMethodRouter {
                inner: axum::routing::$name(handler),
                operations: vec![($key.to_owned(), operation)],
                last: 0,
            }
        }
    };
}

top_level_fn!(delete, Method::DELETE, "delete");
top_level_fn!(get, Method::GET, "get");
top_level_fn!(head, Method::HEAD, "head");
top_level_fn!(options, Method::OPTIONS, "options");
top_level_fn!(patch, Method::PATCH, "patch");
top_level_fn!(post, Method::POST, "post");
top_level_fn!(put, Method::PUT, "put");
top_level_fn!(trace, Method::TRACE, "trace");
