//! Tests for the optional Swagger UI integration.

#![cfg(feature = "swagger-ui")]

mod common;

use axum::body::Body;
use axum::http::{header, Request, StatusCode};
use axum_openapi::routing::get;
use axum_openapi::Router;
use tower::ServiceExt;

#[tokio::test]
async fn serves_swagger_ui_and_spec() {
    let (app, _) = Router::<()>::new()
        .route("/health", get(common::health))
        .swagger_ui("/docs", "/openapi.json")
        .finish();

    let response = app
        .clone()
        .oneshot(Request::builder().uri("/docs").body(Body::empty()).unwrap())
        .await
        .unwrap();

    assert_eq!(response.status(), StatusCode::OK);
    assert_eq!(
        response.headers()[header::CONTENT_TYPE],
        "text/html; charset=utf-8"
    );
    let body = axum::body::to_bytes(response.into_body(), usize::MAX)
        .await
        .unwrap();
    let html = String::from_utf8(body.to_vec()).unwrap();
    assert!(html.contains("swagger-ui"));
    assert!(html.contains(r#"url: "/openapi.json""#));

    let response = app
        .oneshot(
            Request::builder()
                .uri("/openapi.json")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::OK);
}

#[tokio::test]
async fn reuses_an_explicit_openapi_route() {
    let (app, _) = Router::<()>::new()
        .route("/health", get(common::health))
        .openapi_json_route("/spec")
        .swagger_ui("/docs", "/openapi.json")
        .finish();

    let response = app
        .clone()
        .oneshot(Request::builder().uri("/docs").body(Body::empty()).unwrap())
        .await
        .unwrap();
    let body = axum::body::to_bytes(response.into_body(), usize::MAX)
        .await
        .unwrap();
    assert!(String::from_utf8(body.to_vec())
        .unwrap()
        .contains(r#"url: "/spec""#));

    let response = app
        .oneshot(Request::builder().uri("/spec").body(Body::empty()).unwrap())
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::OK);
}
