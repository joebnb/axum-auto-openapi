# axum-openapi

Zero-annotation OpenAPI 3.1 generation for [Axum](https://github.com/tokio-rs/axum) 0.8.

Register routes with `axum_openapi::Router` and `axum_openapi::routing::{get, post, ...}` instead of the axum equivalents. The generator inspects each handler's extractors and return type when the route is registered, then renders a single OpenAPI document in `finish()`. Requests are still served by axum itself: there is no reflection over a finished router and no per-request document overhead.

Design rationale and the acceptance results are documented in [`docs/AXUM_AUTOAPI_DESIGN.md`](docs/AXUM_AUTOAPI_DESIGN.md).

```rust
use axum::extract::Path;
use axum::Json;
use axum_openapi::routing::{get, post};
use axum_openapi::{Info, Router};
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

async fn get_user(Path(id): Path<u64>) -> Json<User> {
    Json(User { id, name: "Ada".into() })
}

async fn create_user(Json(input): Json<CreateUser>) -> Json<User> {
    Json(User { id: 1, name: input.name })
}

let api = Router::<()>::new()
    .info(Info::new("Users API", "0.1.0"))
    .route("/users", post(create_user).summary("Create a user"))
    .route("/users/{id}", get(get_user).summary("Fetch a user"))
    .openapi_json_route("/openapi.json");

let (app, document) = api.finish();

// The same document is served at /openapi.json.
println!("{}", document.to_json_pretty());

let listener = tokio::net::TcpListener::bind("127.0.0.1:3000").await.unwrap();
axum::serve(listener, app).await.unwrap();
```

See [`examples/basic.rs`](examples/basic.rs) for a runnable version:

```bash
cargo run --example basic
curl http://127.0.0.1:3000/openapi.json
```

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

Tags need no annotation. Enable inference once on the root router and every
operation is grouped by the routers it was nested through:

```rust
let auth = Router::<()>::new()
    .route("/login", post(login))
    .route("/me", get(me));

let wx = Router::<()>::new()
    .nest("/auth", auth)
    .nest("/device", device);

Router::<()>::new()
    .nest("/api", wx)
    .infer_tags()
    .finish();
// every operation under `/api/wx/auth` is tagged `wx/auth`
// every operation under `/api/wx/device` is tagged `wx/device`
```

The outermost prefix shared by every nested router (`/api` above) is treated as
the API root and removed. Routes registered directly on the root router fall
back to their first static path segment. An operation that already has a tag —
from `.tag("…")` or `.default_tag("…")` — keeps it.

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

- `swagger-ui` — serve a Swagger UI page backed by the generated OpenAPI JSON:

  ```rust
  let api = Router::<()>::new()
      .route("/users/{id}", get(get_user))
      .openapi_json_route("/openapi.json")
      .swagger_ui("/docs", "/openapi.json");
  ```

  The UI HTML loads `swagger-ui-dist` from unpkg, so the browser needs network access. Enabling the feature adds no runtime dependencies.

## Requirements

- Rust stable
- Axum `0.8`
- schemars `1` (re-exported as `axum_openapi::schemars` and `axum_openapi::JsonSchema`)

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
- Only routes registered through this crate's `Router` are visible; there is no reflection over a finished `axum::Router`.
- Recursive schemas cannot be inlined into path/query parameters (the generator panics with an explanatory message). Use a scalar identifier for those routes.
- `CONNECT` is not supported because OpenAPI path items have no `connect` operation.

## License

MIT
