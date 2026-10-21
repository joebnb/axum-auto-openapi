# axum-auto-openapi

Zero-annotation OpenAPI 3.1 generation for [Axum](https://github.com/tokio-rs/axum) 0.8.

Register routes with `axum_auto_openapi::Router` and `axum_auto_openapi::routing::{get, post, ...}` instead of the axum equivalents. The generator inspects each handler's extractors and return type when the route is registered, then renders a single OpenAPI document in `finish()`. Requests are still served by axum itself: there is no reflection over a finished router and no per-request document overhead.

Design rationale and the acceptance results are documented in [`docs/AXUM_AUTOAPI_DESIGN.md`](docs/AXUM_AUTOAPI_DESIGN.md).

## Minimal working example

A complete project: two files, no annotations, no path configuration.

`Cargo.toml`

```toml
[package]
name = "users-api"
version = "0.1.0"
edition = "2021"

[dependencies]
axum = "0.8"
axum-auto-openapi = { git = "https://github.com/joebnb/axum-auto-openapi.git", features = ["swagger-ui"] }
schemars = { version = "1", features = ["derive"] }
serde = { version = "1", features = ["derive"] }
tokio = { version = "1", features = ["macros", "rt-multi-thread", "net"] }
```

`src/main.rs`

```rust
use axum::extract::Path;
use axum::Json;
use axum_auto_openapi::routing::get;
use axum_auto_openapi::Router;
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

#[derive(Serialize, JsonSchema)]
struct User {
    id: u64,
    name: String,
}

#[derive(Deserialize, JsonSchema)]
struct CreateUser {
    name: String,
}

async fn list_users() -> Json<Vec<User>> {
    Json(vec![User { id: 1, name: "Ada".into() }])
}

async fn create_user(Json(input): Json<CreateUser>) -> Json<User> {
    Json(User { id: 2, name: input.name })
}

async fn get_user(Path(id): Path<u64>) -> Json<User> {
    Json(User { id, name: "Ada".into() })
}

#[tokio::main]
async fn main() {
    let (app, _document) = Router::<()>::new()
        .route("/users", get(list_users).post(create_user))
        .route("/users/{id}", get(get_user))
        .finish();

    let listener = tokio::net::TcpListener::bind("127.0.0.1:3000")
        .await
        .unwrap();
    axum::serve(listener, app).await.unwrap();
}
```

Run it:

```bash
cargo run
curl http://127.0.0.1:3000/api/openapi   # OpenAPI 3.1 JSON
open http://127.0.0.1:3000/api/docs      # Swagger UI (needs `swagger-ui`)
```

That is the whole setup. `/api/openapi` and `/api/docs` come from the
defaults, tags are inferred from the router tree, and `finish()` also
returns the document as `_document` for logging or client generation.

See [`examples/basic.rs`](examples/basic.rs) for a longer runnable version
with query parameters, status codes, security schemes, and explicit tags.

## What is captured

| Handler surface | OpenAPI output |
| --- | --- |
| `Json<T>` request body | `requestBody` with an `application/json` schema |
| `Json<T>` response | `200` response with a `$ref` to `T` (or an inline primitive/array schema) |
| `Path<T>` | Path parameters for scalars, tuples, arrays, and structs |
| `Query<T>` | One query parameter per object property, including `required`, arrays, and `deepObject` objects |
| `State<T>`, `Extension<T>`, `HeaderMap`, `Method`, `Uri`, `RawQuery`, `ConnectInfo<T>`, `Request` | Recognized but contribute no parameters |
| `Bytes` / `String` input | Binary / text request body (declare the real shape with `.json_body::<T>()`) |
| `String`, `&'static str`, `Html<_>`, `Bytes`, `Response` output | Text, HTML, binary, or opaque response |
| `Result<T, E>` | Success and error branches (error uses `default` unless you add a status) |
| `StatusCode`, `(StatusCode, T)` | `default` response, because the status is only known at runtime |
| `ApiResponse<T, STATUS>` | `STATUS` response, with the body described by `T` |
| `ApiResponseDefault<T>` | `default` response with the body described by `T` |
| `()` | Empty response |

Schemas come from [`schemars`](https://docs.rs/schemars). Object and polymorphic models are hoisted into `components.schemas`; identical models are deduplicated, and conflicting schemars names receive deterministic `_2`, `_3`, ... suffixes.

### Adding metadata

Metadata applies to the most recently added method in a method router:

```rust
get(list_users)
    .summary("List users")
    .description("Returns every user.")
    .tag("users")
    .security("bearer")
    .response_with::<User>(200, "The users")
    .error_response::<ApiError>(500)
    .post(create_user)
    .summary("Create a user");
```

Document-level metadata is configured on the router:

```rust
Router::<()>::new()
    .info(Info::new("Users API", "0.1.0").description("..."))
    .server(Server::new("https://api.example.com"))
    .tag(Tag::new("users"))
    .security_scheme("bearer", SecurityScheme::bearer());
```

### Tags from the router tree

Tags need no annotation. Every operation is grouped by the routers it was
nested through:

```rust
let auth = Router::<()>::new()
    .route("/login", post(login))
    .route("/me", get(me));

let wx_auth = Router::<()>::new().route("/login", post(wx_login));

let wx = Router::<()>::new()
    .nest("/auth", wx_auth)
    .nest("/device", device);

Router::<()>::new()
    .nest("/api/auth", auth)
    .nest("/api/wx", wx)
    .finish();
// `/api/auth/login`    -> `auth`
// `/api/wx/auth/login` -> `wx/auth`
// `/api/wx/device/…`   -> `wx/device`
```

Call `Router::without_infer_tags()` when path-derived groups do not match the
API's vocabulary and every operation is tagged explicitly.

The prefix shared by every nested router (`/api` above) is treated as the API
root and removed. Two edge cases follow from that rule:

- When every operation shares its whole nest chain, the innermost router name
  is used, so an API that only serves `/api/wx/auth` is tagged `auth` rather
  than one tag per route.
- Routes registered directly on the root router fall back to their first
  static path segment (`/api/health` -> `health`).

An operation that already has a tag — from `.tag("…")` or `.default_tag("…")` —
keeps it.

`Router::default_tag` remains available for cases where the router tree does
not express the grouping: it tags every untagged operation on that router,
including ones registered after the call.

### Bodies your handler parses itself

A handler that takes `Bytes` or `String` and parses the payload is documented as
`application/octet-stream` / `text/plain`, because its extractors cannot say
more. Declare the actual body on the route instead:

```rust
post(login)                                    // fn login(raw: Bytes) -> ...
    .json_body::<LoginBody>()                  // replaces the inferred body
    .response_with::<Envelope>(200, "Signed in");

post(webhook)
    .raw_body("application/cloudevents+json", true, json!({ "type": "object" }));
```

### Responses with a declared status

A bare `Response` knows neither its status code nor its body at registration
time, so it is documented as an empty `200`. When the status is fixed, wrap the
body in `ApiResponse<T, STATUS>`. `T` is whatever the handler would return
anyway, and the wrapper hands response building back to axum:

```rust
use axum::response::{IntoResponse, Redirect, Response};
use axum_auto_openapi::ApiResponse;

async fn create_device(Json(input): Json<NewDevice>) -> ApiResponse<Json<Device>, 201> {
    ApiResponse::json(Device::from(input))
}

async fn login_redirect() -> ApiResponse<Response, 302> {
    ApiResponse::new(Redirect::temporary("/dashboard").into_response())
}
```

`ApiResponse::json(value)` is shorthand for `ApiResponse::new(Json(value))`.
Anything axum can turn into a response works as `T`, so headers, cookies and
redirects are untouched — only the status code is replaced. The status is a
const parameter because the document is built at startup.

Three things the type cannot check for you:

- `T` must be a single response type. `ApiResponse<Result<Json<T>, E>, 200>`
  compiles, but replaces the status of the error branch too.
- A status already present in an adopted `Response` is discarded silently.
- `204` and `304` must not carry a body; declare them as
  `ApiResponse::<(), 204>::new(())`.

When the status is chosen at runtime but the body is still typed, use
`ApiResponseDefault<T>` — the document records it under `default` and axum
keeps whatever status the body produced:

```rust
async fn create(accepted: bool) -> ApiResponseDefault<Json<Device>> {
    let status = if accepted { StatusCode::ACCEPTED } else { StatusCode::CREATED };
    ApiResponseDefault::new((status, Json(Device::from(input))))
}
```

### Response headers

Headers are documented separately from the body, on the status they belong to:

```rust
use axum_auto_openapi::routing::post;

post(login)
    .response_header::<String>(200, "Set-Cookie", "Session cookie", false)
    .raw_response_header(
        429,
        "Retry-After",
        json!({ "type": "integer", "minimum": 0 }),
        "Seconds until the next attempt is allowed",
        true,
    );
```

Both methods create the status entry when it does not exist yet, so they can be
combined with `.response_with::<T>(...)` in any order.

### Publishing the documentation

There is nothing to wire. `finish()` mounts the JSON document at
`/api/openapi` and, with the `swagger-ui` feature, the UI at `/api/docs`.
Turn them off at runtime without changing how the document is built —
`finish()` still returns it, so startup logging, client generation, and tests
keep working:

```rust
let publish_docs = std::env::var("OPENAPI_ENABLED").is_ok_and(|value| value != "false");

let api = Router::<()>::new()
    .route("/users", get(list_users))
    .documentation(publish_docs);
```

The crate never reads the environment itself; how `publish_docs` is derived is
the application's decision. The name `OPENAPI_ENABLED` above is only a
convention — any configuration source works.

`documentation(false)` leaves both routes unmounted; requests to them fall
through to whatever fallback the application installs (usually a 404). The
default paths are exported as `DEFAULT_DOCUMENT_PATH` and `DEFAULT_DOCS_PATH`
and can be replaced with `.openapi_json_route(..)` / `.swagger_ui(..)`.

### Routes outside the document

`merge_undocumented` and `nest_undocumented` mount a plain `axum::Router`
without adding operations. Use them for surfaces OpenAPI cannot describe —
WebSocket upgrades, opaque proxies, static files — or to adopt the generator
one module at a time:

```rust
let api = Router::<()>::new()
    .nest("/api", documented_api)
    .nest_undocumented("/api/iot", websocket_gateway);
```

### Checking the result

`finish()` panics on mistakes that make the document unusable (malformed paths,
unknown path parameters, duplicate routes). Softer problems are reported by
`OpenApiDocument::warnings()` — an operation requiring a security scheme that
was never registered, or duplicated `operationId`s:

```rust
let (app, document) = api.finish();
for warning in document.warnings() {
    tracing::warn!("openapi: {warning}");
}
```

Custom extractors and response wrappers can participate in the document by implementing `ApiInput` and `ApiOutput` respectively. Their `describe` method receives an `OperationDescriptor`, so they can also declare security requirements (`operation.security("bearer")`) or a documented response. Handlers containing an unsupported extractor or return type fail to compile with a targeted diagnostic rather than silently emitting an incomplete document.

## Features

- `swagger-ui` — serve a Swagger UI page backed by the generated OpenAPI JSON.
  Nothing to configure: the page is mounted at `/api/docs` and reads the
  document from `/api/openapi`. Custom paths:

  ```rust
  let api = Router::<()>::new()
      .route("/users/{id}", get(get_user))
      .openapi_json_route("/openapi.json")
      .swagger_ui("/docs", "/openapi.json");
  ```

  The UI HTML loads `swagger-ui-dist` from unpkg, so the browser needs network access. Enabling the feature adds no runtime dependencies.

## Requirements

- Rust 1.82 or newer
- Axum `0.8`
- schemars `1` (re-exported as `axum_auto_openapi::schemars` and `axum_auto_openapi::JsonSchema`)

## Development

```bash
cargo fmt --all
cargo check --all-targets --all-features
cargo test --all-features
cargo clippy --all-targets --all-features -- -D warnings
```

The test suite covers the generated document from several angles:

- `tests/openapi.rs` asserts the document structure and metadata.
- `tests/http.rs` sends real requests through axum with `tower::ServiceExt::oneshot`, so the documented surface is checked against actual routing behavior.
- `tests/validation.rs` parses every generated document with the `oas3` OpenAPI 3.1 implementation and verifies that all internal `$ref` pointers resolve.
- `tests/compile.rs` uses `trybuild` to pin the diagnostics for unsupported extractors and return types.
- `tests/swagger.rs` checks the `swagger-ui` feature: the UI page, the spec route, and reuse of an explicit OpenAPI route.

The document snapshot lives at `tests/fixtures/basic.json`. After an intentional output change, regenerate it and review the diff:

```bash
UPDATE_GOLDEN=1 cargo test --test golden
```

Compile-fail expectations live next to their cases in `tests/ui/*.stderr`. Regenerate them after intentionally changing the diagnostic text:

```bash
TRYBUILD=overwrite cargo test --test compile
```

## Deliberate limitations

- Handler bodies and branch conditions are not analyzed. Use `.response::<T>(status)`, `.empty_response(...)`, or `.raw_response(...)` for business-specific statuses.
- A bare `StatusCode` response documents a `default` response, not its possible concrete codes.
- `route_service`, `fallback`, and `method_not_allowed_fallback` handlers are not documented.
- The documentation routes are mounted by default at `/api/openapi` and `/api/docs`. Registering your own handler at either path panics as a duplicate route; call `documentation(false)` or change the paths with `openapi_json_route` / `swagger_ui` instead.
- Only routes registered through this crate's `Router` are visible; there is no reflection over a finished `axum::Router`.
- Recursive schemas cannot be inlined into path/query parameters (the generator panics with an explanatory message). Use a scalar identifier for those routes.
- `CONNECT` is not supported because OpenAPI path items have no `connect` operation.

## License

BSD-3-Clause
