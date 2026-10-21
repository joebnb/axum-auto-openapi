//! The OpenAPI-aware router.
//!
//! [`Router`] wraps [`axum::Router`] and records an operation for every route
//! registered through [`crate::routing`]. Requests are served by axum itself;
//! the capture work happens only while the application is assembled.

use std::collections::BTreeMap;
use std::convert::Infallible;
use std::marker::PhantomData;
use std::sync::Arc;

use axum::extract::Request;
use axum::handler::Handler;
use axum::response::IntoResponse;
use axum::routing::Route;
use serde_json::Value;
use tower::{Layer, Service};

use crate::openapi::{build_document, Info, OpenApiDocument, SecurityScheme, Server, Tag};
use crate::operation::OperationDescriptor;
use crate::routing::ApiMethodRouter;

#[cfg(feature = "swagger-ui")]
#[derive(Clone, Debug)]
struct SwaggerConfig {
    ui_path: String,
    spec_path: String,
}

/// An axum router that also collects OpenAPI operations.
///
/// `S` follows [`axum::Router<S>`]: it is the state type that still has to be
/// supplied with [`Router::with_state`].
///
/// ```rust,no_run
/// use axum::extract::State;
/// use axum::Json;
/// use axum_openapi::routing::get;
/// use axum_openapi::Router;
///
/// #[derive(Clone)]
/// struct AppState;
///
/// async fn health(State(_): State<AppState>) -> Json<&'static str> {
///     Json("ok")
/// }
///
/// # fn main() {
/// let api = Router::<AppState>::new()
///     .route("/health", get(health))
///     .with_state(AppState);
/// let (app, document): (axum::Router, _) = api.finish();
/// # let _ = (app, document);
/// # }
/// ```
pub struct Router<S = ()> {
    inner: axum::Router<S>,
    paths: BTreeMap<String, BTreeMap<String, OperationDescriptor>>,
    info: Info,
    servers: Vec<Server>,
    tags: Vec<Tag>,
    security_schemes: BTreeMap<String, Value>,
    openapi_route: Option<String>,
    #[cfg(feature = "swagger-ui")]
    swagger: Option<SwaggerConfig>,
    _marker: PhantomData<fn() -> S>,
}

impl<S> Router<S>
where
    S: Clone + Send + Sync + 'static,
{
    /// Create an empty router.
    ///
    /// As with axum, the state type is inferred from the handlers. When the
    /// router has no state, annotate it as `Router::<()>::new()` if nothing
    /// else pins the type down:
    ///
    /// ```rust,no_run
    /// # use axum_openapi::routing::get;
    /// # use axum_openapi::Router;
    /// # async fn handler() {}
    /// let (app, document) = Router::<()>::new().route("/", get(handler)).finish();
    /// # let _ = (app, document);
    /// ```
    pub fn new() -> Self {
        Self {
            inner: axum::Router::new(),
            paths: BTreeMap::new(),
            info: Info::new("Axum OpenAPI", env!("CARGO_PKG_VERSION")),
            servers: Vec::new(),
            tags: Vec::new(),
            security_schemes: BTreeMap::new(),
            openapi_route: None,
            #[cfg(feature = "swagger-ui")]
            swagger: None,
            _marker: PhantomData,
        }
    }

    /// Register a route.
    ///
    /// The path must use axum 0.8 syntax (`/users/{id}`). Registering the same
    /// method twice for one path is a programming error and panics, matching
    /// axum's own behavior.
    pub fn route(mut self, path: &str, method_router: ApiMethodRouter<S>) -> Self {
        assert!(
            path.starts_with('/'),
            "route path must start with '/', got `{path}`"
        );

        let (inner, operations) = method_router.into_parts();
        {
            let path_item = self.paths.entry(path.to_owned()).or_default();
            for (method, operation) in operations {
                if path_item.insert(method.clone(), operation).is_some() {
                    panic!("duplicate OpenAPI operation `{method}` for path `{path}`");
                }
            }
        }

        self.inner = self.inner.route(path, inner);
        self
    }

    /// Nest another router below `path`.
    ///
    /// Documented paths are prefixed exactly like axum prefixed the real
    /// routes, including the special handling of a route at `/`.
    pub fn nest(mut self, path: &str, router: Router<S>) -> Self {
        if path.is_empty() || path == "/" {
            panic!("Nesting at the root is not supported; use merge instead");
        }
        assert!(
            path.starts_with('/'),
            "nest path must start with '/', got `{path}`"
        );

        let Router {
            inner,
            paths,
            tags,
            security_schemes,
            ..
        } = router;

        self.inner = self.inner.nest(path, inner);

        for (inner_path, operations) in paths {
            let full_path = join_nested_path(path, &inner_path);
            let path_item = self.paths.entry(full_path.clone()).or_default();
            for (method, operation) in operations {
                if path_item.insert(method.clone(), operation).is_some() {
                    panic!(
                        "duplicate OpenAPI operation `{method}` for path `{full_path}` after \
                         nesting at `{path}`"
                    );
                }
            }
        }

        self.tags.extend(tags);
        for (name, scheme) in security_schemes {
            self.insert_security_scheme(name, scheme);
        }
        self
    }

    /// Merge another router into this one.
    ///
    /// Duplicate `(path, method)` pairs are rejected with a descriptive panic.
    pub fn merge(mut self, other: Router<S>) -> Self {
        let Router {
            inner: other_inner,
            paths: other_paths,
            tags,
            security_schemes,
            ..
        } = other;

        for (path, operations) in &other_paths {
            if let Some(existing) = self.paths.get(path) {
                for method in operations.keys() {
                    if existing.contains_key(method) {
                        panic!("duplicate OpenAPI operation `{method}` for path `{path}`");
                    }
                }
            }
        }

        self.inner = self.inner.merge(other_inner);
        for (path, operations) in other_paths {
            self.paths.entry(path).or_default().extend(operations);
        }
        self.tags.extend(tags);
        for (name, scheme) in security_schemes {
            self.insert_security_scheme(name, scheme);
        }
        self
    }

    /// Provide the state for `Router<S>` and return the now-stateless router
    /// type (usually `Router<()>`).
    pub fn with_state<S2>(self, state: S) -> Router<S2> {
        Router {
            inner: self.inner.with_state(state),
            paths: self.paths,
            info: self.info,
            servers: self.servers,
            tags: self.tags,
            security_schemes: self.security_schemes,
            openapi_route: self.openapi_route,
            #[cfg(feature = "swagger-ui")]
            swagger: self.swagger,
            _marker: PhantomData,
        }
    }

    /// Apply a tower layer to every route and fallback.
    ///
    /// Metadata is preserved; middleware does not affect the generated
    /// document.
    pub fn layer<L>(self, layer: L) -> Self
    where
        L: Layer<Route> + Clone + Send + Sync + 'static,
        L::Service: Service<Request> + Clone + Send + Sync + 'static,
        <L::Service as Service<Request>>::Response: IntoResponse + 'static,
        <L::Service as Service<Request>>::Error: Into<Infallible> + 'static,
        <L::Service as Service<Request>>::Future: Send + 'static,
    {
        Router {
            inner: self.inner.layer(layer),
            paths: self.paths,
            info: self.info,
            servers: self.servers,
            tags: self.tags,
            security_schemes: self.security_schemes,
            openapi_route: self.openapi_route,
            #[cfg(feature = "swagger-ui")]
            swagger: self.swagger,
            _marker: PhantomData,
        }
    }

    /// Apply a tower layer to routes, without applying it to the fallback.
    ///
    /// Metadata is preserved; middleware does not affect the generated
    /// document.
    pub fn route_layer<L>(self, layer: L) -> Self
    where
        L: Layer<Route> + Clone + Send + Sync + 'static,
        L::Service: Service<Request> + Clone + Send + Sync + 'static,
        <L::Service as Service<Request>>::Response: IntoResponse + 'static,
        <L::Service as Service<Request>>::Error: Into<Infallible> + 'static,
        <L::Service as Service<Request>>::Future: Send + 'static,
    {
        Router {
            inner: self.inner.route_layer(layer),
            paths: self.paths,
            info: self.info,
            servers: self.servers,
            tags: self.tags,
            security_schemes: self.security_schemes,
            openapi_route: self.openapi_route,
            #[cfg(feature = "swagger-ui")]
            swagger: self.swagger,
            _marker: PhantomData,
        }
    }

    /// Set the fallback handler. Fallback routes are not documented
    /// automatically.
    pub fn fallback<H, T>(self, handler: H) -> Self
    where
        H: Handler<T, S>,
        T: 'static,
    {
        Router {
            inner: self.inner.fallback(handler),
            paths: self.paths,
            info: self.info,
            servers: self.servers,
            tags: self.tags,
            security_schemes: self.security_schemes,
            openapi_route: self.openapi_route,
            #[cfg(feature = "swagger-ui")]
            swagger: self.swagger,
            _marker: PhantomData,
        }
    }

    /// Set the fallback for requests whose path exists but whose method does
    /// not. This route is not documented automatically.
    pub fn method_not_allowed_fallback<H, T>(self, handler: H) -> Self
    where
        H: Handler<T, S>,
        T: 'static,
    {
        Router {
            inner: self.inner.method_not_allowed_fallback(handler),
            paths: self.paths,
            info: self.info,
            servers: self.servers,
            tags: self.tags,
            security_schemes: self.security_schemes,
            openapi_route: self.openapi_route,
            #[cfg(feature = "swagger-ui")]
            swagger: self.swagger,
            _marker: PhantomData,
        }
    }

    /// Finish the document and return the underlying axum router.
    ///
    /// Schema generation and path-parameter validation happen here, so
    /// configuration mistakes panic during application startup rather than on
    /// the first request.
    pub fn finish(self) -> (axum::Router<S>, OpenApiDocument) {
        let document = build_document(
            self.info,
            self.servers,
            self.tags,
            self.security_schemes,
            self.paths,
        );

        let mut inner = self.inner;
        #[allow(unused_mut)]
        let mut spec_route = self.openapi_route;

        #[cfg(feature = "swagger-ui")]
        let swagger = self.swagger;
        #[cfg(feature = "swagger-ui")]
        if let Some(swagger) = &swagger {
            if spec_route.is_none() {
                spec_route = Some(swagger.spec_path.clone());
            }
        }

        if let Some(path) = &spec_route {
            assert!(
                path.starts_with('/'),
                "OpenAPI JSON route must start with '/', got `{path}`"
            );
            let spec = Arc::new(document.to_value());
            let handler = move || {
                let spec = Arc::clone(&spec);
                async move { axum::Json(spec.as_ref().clone()) }
            };
            inner = inner.route(path, axum::routing::get(handler));
        }

        #[cfg(feature = "swagger-ui")]
        if let Some(swagger) = swagger {
            let spec_path = spec_route.as_deref().unwrap_or(swagger.spec_path.as_str());
            let html = crate::openapi::swagger_ui_html(spec_path);
            let handler = move || async move { axum::response::Html(html) };
            inner = inner.route(&swagger.ui_path, axum::routing::get(handler));
        }

        (inner, document)
    }

    /// Like [`finish`](Self::finish), but returns the document as a JSON value.
    pub fn finish_json(self) -> (axum::Router<S>, Value) {
        let (router, document) = self.finish();
        (router, document.to_value())
    }
}

impl<S> Router<S>
where
    S: Clone + Send + Sync + 'static,
{
    /// Replace the default `info` object.
    pub fn info(mut self, info: Info) -> Self {
        self.info = info;
        self
    }

    /// Add a server entry.
    pub fn server(mut self, server: Server) -> Self {
        self.servers.push(server);
        self
    }

    /// Add a top-level tag entry.
    pub fn tag(mut self, tag: Tag) -> Self {
        self.tags.push(tag);
        self
    }

    /// Register a security scheme under `components.securitySchemes`.
    pub fn security_scheme(mut self, name: impl Into<String>, scheme: SecurityScheme) -> Self {
        self.insert_security_scheme(name.into(), scheme.into_value());
        self
    }

    /// Serve the generated OpenAPI document as JSON at `path`.
    ///
    /// The document endpoint itself is not added to `paths`.
    pub fn openapi_json_route(mut self, path: impl Into<String>) -> Self {
        self.openapi_route = Some(path.into());
        self
    }

    /// Serve a Swagger UI page at `ui_path`, pointing at the OpenAPI JSON
    /// route `spec_path`.
    ///
    /// If no [`openapi_json_route`](Self::openapi_json_route) was configured,
    /// the document is automatically served at `spec_path`.
    #[cfg(feature = "swagger-ui")]
    pub fn swagger_ui(mut self, ui_path: impl Into<String>, spec_path: impl Into<String>) -> Self {
        self.swagger = Some(SwaggerConfig {
            ui_path: ui_path.into(),
            spec_path: spec_path.into(),
        });
        self
    }

    fn insert_security_scheme(&mut self, name: String, scheme: Value) {
        if let Some(existing) = self.security_schemes.get(&name) {
            if *existing != scheme {
                panic!("conflicting security scheme `{name}`");
            }
        } else {
            self.security_schemes.insert(name, scheme);
        }
    }
}

impl<S> Default for Router<S>
where
    S: Clone + Send + Sync + 'static,
{
    fn default() -> Self {
        Self::new()
    }
}

/// Mirror axum's `path_for_nested_route` behavior so documented paths match
/// the router's actual matching paths.
fn join_nested_path(prefix: &str, path: &str) -> String {
    if prefix.ends_with('/') {
        format!("{prefix}{}", path.trim_start_matches('/'))
    } else if path == "/" {
        prefix.to_owned()
    } else {
        format!("{prefix}{path}")
    }
}

#[cfg(test)]
mod tests {
    use super::join_nested_path;

    #[test]
    fn nested_paths_match_axum() {
        assert_eq!(join_nested_path("/api", "/users"), "/api/users");
        assert_eq!(join_nested_path("/api", "/"), "/api");
        assert_eq!(join_nested_path("/api/", "/users"), "/api/users");
        assert_eq!(join_nested_path("/api/", "/"), "/api/");
    }
}
