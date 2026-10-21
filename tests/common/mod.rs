//! Shared handlers and routers for the integration tests.

#![allow(dead_code)]

use axum::extract::{Path, Query, State};
use axum::http::StatusCode;
use axum::Json;
use axum_auto_openapi::routing::get;
use axum_auto_openapi::{Info, OpenApiDocument, Router, SecurityScheme, Server, Tag};
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

#[derive(Clone, Debug)]
pub struct AppState {
    pub label: String,
}

#[derive(Clone, Debug, Serialize, Deserialize, JsonSchema)]
pub struct User {
    pub id: u64,
    pub name: String,
}

#[derive(Clone, Debug, Deserialize, JsonSchema)]
pub struct CreateUser {
    pub name: String,
}

#[derive(Clone, Debug, Deserialize, JsonSchema)]
pub struct Filter {
    pub name: Option<String>,
}

#[derive(Clone, Debug, Deserialize, JsonSchema)]
pub struct ListParams {
    #[serde(default)]
    pub limit: Option<u32>,
    /// Comma-separated tags. axum's `Query` uses `serde_urlencoded`, which does
    /// not support repeated keys, so this is intentionally a string.
    pub tags: String,
    #[serde(default)]
    pub filter: Option<Filter>,
}

#[derive(Clone, Debug, Deserialize, JsonSchema)]
pub struct PathIds {
    pub org: String,
    pub id: u64,
}

#[derive(Clone, Debug, Serialize, JsonSchema)]
pub struct ApiError {
    pub message: String,
}

pub async fn health() -> &'static str {
    "ok"
}

pub async fn list_users(Query(params): Query<ListParams>) -> Json<Vec<User>> {
    let name = params
        .filter
        .as_ref()
        .and_then(|filter| filter.name.clone())
        .unwrap_or_else(|| params.tags.clone());
    let count = params.limit.unwrap_or(1).min(10);
    let users = (0..count)
        .map(|index| User {
            id: u64::from(index),
            name: name.clone(),
        })
        .collect();
    Json(users)
}

pub async fn create_user(Json(input): Json<CreateUser>) -> Json<User> {
    Json(User {
        id: 42,
        name: input.name,
    })
}

pub async fn get_user(Path(id): Path<u64>) -> Json<User> {
    Json(User {
        id,
        name: format!("user-{id}"),
    })
}

pub async fn delete_user(Path(id): Path<u64>) -> StatusCode {
    let _ = id;
    StatusCode::NO_CONTENT
}

pub async fn maybe_user(Path(id): Path<u64>) -> Result<Json<User>, StatusCode> {
    if id == 0 {
        Err(StatusCode::NOT_FOUND)
    } else {
        Ok(Json(User {
            id,
            name: format!("user-{id}"),
        }))
    }
}

pub async fn state_label(State(state): State<AppState>) -> Json<String> {
    Json(state.label)
}

pub async fn path_pair(Path((category, id)): Path<(String, u64)>) -> Json<String> {
    Json(format!("{category}/{id}"))
}

pub async fn path_ids(Path(ids): Path<PathIds>) -> Json<String> {
    Json(format!("{}/{}", ids.org, ids.id))
}

/// A router covering body, path, query, error-response and delete cases.
pub fn app() -> (axum::Router, OpenApiDocument) {
    Router::<()>::new()
        .info(
            Info::new("Test API", "1.0.0")
                .description("Integration test API")
                .contact("API Team", "api@example.com", "https://example.com"),
        )
        .security_scheme("bearer", SecurityScheme::bearer())
        .server(Server::new("https://api.example.com").description("Production"))
        .tag(Tag::new("users").description("User management"))
        .route(
            "/users",
            get(list_users)
                .summary("List users")
                .tag("users")
                .post(create_user)
                .summary("Create a user")
                .description("Creates a user from a JSON body.")
                .operation_id("createUser")
                .tag("users"),
        )
        .route(
            "/users/{id}",
            get(get_user)
                .summary("Fetch a user")
                .security("bearer")
                .delete(delete_user)
                .summary("Delete a user"),
        )
        .route(
            "/failure/{id}",
            get(maybe_user).error_response::<ApiError>(404),
        )
        .finish()
}

/// Two routers combined with `nest` and `merge`.
pub fn nested_app() -> (axum::Router, OpenApiDocument) {
    let v1 = Router::<()>::new()
        .route("/items", get(health))
        .route("/items/{id}", get(get_user));
    Router::<()>::new()
        .nest("/v1", v1)
        .merge(Router::<()>::new().route("/health", get(health)))
        .finish()
}

/// A router that still needs state supplied through `with_state`.
pub fn stateful_app() -> (axum::Router, OpenApiDocument) {
    Router::<AppState>::new()
        .route("/state", get(state_label))
        .with_state(AppState {
            label: "ready".to_owned(),
        })
        .finish()
}
