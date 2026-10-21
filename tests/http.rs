//! Serve the documented routers through axum and exercise the real handlers.

mod common;

use axum::body::Body;
use axum::http::{header, Method, Request, StatusCode};
use axum_auto_openapi::routing::{get, post};
use axum_auto_openapi::Router;
use serde_json::Value;
use tower::ServiceExt;

async fn json_body(response: axum::response::Response) -> Value {
    let bytes = axum::body::to_bytes(response.into_body(), usize::MAX)
        .await
        .unwrap();
    serde_json::from_slice(&bytes).unwrap()
}

async fn text_body(response: axum::response::Response) -> String {
    let bytes = axum::body::to_bytes(response.into_body(), usize::MAX)
        .await
        .unwrap();
    String::from_utf8(bytes.to_vec()).unwrap()
}

#[tokio::test]
async fn serves_path_handler() {
    let (app, _) = common::app();
    let response = app
        .oneshot(
            Request::builder()
                .uri("/users/7")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();

    assert_eq!(response.status(), StatusCode::OK);
    assert_eq!(
        json_body(response).await,
        serde_json::json!({"id": 7, "name": "user-7"})
    );
}

#[tokio::test]
async fn serves_query_handler() {
    let (app, _) = common::app();
    let response = app
        .oneshot(
            Request::builder()
                .uri("/users?limit=2&tags=a")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();

    assert_eq!(response.status(), StatusCode::OK);
    let users = json_body(response).await;
    assert_eq!(users.as_array().unwrap().len(), 2);
    assert_eq!(users[0]["name"], "a");
}

#[tokio::test]
async fn serves_json_body_handler() {
    let (app, _) = common::app();
    let response = app
        .oneshot(
            Request::builder()
                .method(Method::POST)
                .uri("/users")
                .header(header::CONTENT_TYPE, "application/json")
                .body(Body::from(r#"{"name":"Grace"}"#))
                .unwrap(),
        )
        .await
        .unwrap();

    assert_eq!(response.status(), StatusCode::OK);
    assert_eq!(
        json_body(response).await,
        serde_json::json!({"id": 42, "name": "Grace"})
    );
}

#[tokio::test]
async fn serves_empty_delete_response() {
    let (app, _) = common::app();
    let response = app
        .oneshot(
            Request::builder()
                .method(Method::DELETE)
                .uri("/users/1")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();

    assert_eq!(response.status(), StatusCode::NO_CONTENT);
    let bytes = axum::body::to_bytes(response.into_body(), usize::MAX)
        .await
        .unwrap();
    assert!(bytes.is_empty());
}

#[tokio::test]
async fn serves_error_branch() {
    let (app, _) = common::app();
    let response = app
        .clone()
        .oneshot(
            Request::builder()
                .uri("/failure/0")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::NOT_FOUND);

    let response = app
        .oneshot(
            Request::builder()
                .uri("/failure/3")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::OK);
    assert_eq!(json_body(response).await["id"], 3);
}

#[tokio::test]
async fn serves_stateful_router() {
    let (app, _) = common::stateful_app();
    let response = app
        .oneshot(
            Request::builder()
                .uri("/state")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();

    assert_eq!(response.status(), StatusCode::OK);
    assert_eq!(json_body(response).await, serde_json::json!("ready"));
}

#[tokio::test]
async fn serves_nested_and_merged_routes() {
    let (app, _) = common::nested_app();

    let response = app
        .clone()
        .oneshot(
            Request::builder()
                .uri("/v1/items/5")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::OK);
    assert_eq!(json_body(response).await["id"], 5);

    let response = app
        .clone()
        .oneshot(
            Request::builder()
                .uri("/v1/items")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::OK);
    assert_eq!(text_body(response).await, "ok");

    let response = app
        .oneshot(
            Request::builder()
                .uri("/health")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::OK);
}

#[tokio::test]
async fn serves_complex_path_extractors() {
    let (app, _) = Router::<()>::new()
        .route("/{category}/{id}", get(common::path_pair))
        .route("/orgs/{org}/users/{id}", get(common::path_ids))
        .finish();

    let response = app
        .clone()
        .oneshot(
            Request::builder()
                .uri("/books/9")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(json_body(response).await, serde_json::json!("books/9"));

    let response = app
        .oneshot(
            Request::builder()
                .uri("/orgs/acme/users/9")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(json_body(response).await, serde_json::json!("acme/9"));
}

#[tokio::test]
async fn serves_text_body_and_response() {
    async fn echo(body: String) -> String {
        body
    }

    let (app, _) = Router::<()>::new().route("/echo", post(echo)).finish();
    let response = app
        .oneshot(
            Request::builder()
                .method(Method::POST)
                .uri("/echo")
                .header(header::CONTENT_TYPE, "text/plain")
                .body(Body::from("hello"))
                .unwrap(),
        )
        .await
        .unwrap();

    assert_eq!(response.status(), StatusCode::OK);
    assert_eq!(text_body(response).await, "hello");
}

#[tokio::test]
async fn serves_dynamic_status_tuple_response() {
    async fn create() -> (StatusCode, axum::Json<common::User>) {
        (
            StatusCode::CREATED,
            axum::Json(common::User {
                id: 9,
                name: "new".to_owned(),
            }),
        )
    }

    let (app, _) = Router::<()>::new().route("/things", post(create)).finish();
    let response = app
        .oneshot(
            Request::builder()
                .method(Method::POST)
                .uri("/things")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();

    assert_eq!(response.status(), StatusCode::CREATED);
    assert_eq!(json_body(response).await["id"], 9);
}

#[tokio::test]
async fn unknown_routes_are_not_invented() {
    let (app, _) = common::app();
    let response = app
        .oneshot(
            Request::builder()
                .uri("/missing")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::NOT_FOUND);
}
