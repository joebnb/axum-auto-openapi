//! Structural assertions on the generated OpenAPI document.

mod common;

use axum::body::Body;
use axum::extract::{Path, Query};
use axum::http::{Request, StatusCode};
use axum::Json;
use axum_auto_openapi::routing::{get, post};
use axum_auto_openapi::Router;
use serde::Deserialize;
use serde_json::{json, Value};
use tower::ServiceExt;

fn parameter<'a>(operation: &'a Value, name: &str) -> &'a Value {
    operation["parameters"]
        .as_array()
        .unwrap_or_else(|| panic!("operation has no parameters: {operation}"))
        .iter()
        .find(|parameter| parameter["name"].as_str() == Some(name))
        .unwrap_or_else(|| panic!("missing parameter `{name}` in {operation}"))
}

#[test]
fn renders_document_metadata() {
    let (_, document) = common::app();
    let value = document.as_value();

    assert_eq!(value["openapi"], "3.1.0");
    assert_eq!(value["info"]["title"], "Test API");
    assert_eq!(value["info"]["version"], "1.0.0");
    assert_eq!(value["info"]["description"], "Integration test API");
    assert_eq!(value["info"]["contact"]["email"], "api@example.com");
    assert_eq!(
        value["components"]["securitySchemes"]["bearer"]["scheme"],
        "bearer"
    );
    assert_eq!(value["servers"][0]["url"], "https://api.example.com");
    assert_eq!(value["servers"][0]["description"], "Production");
    assert_eq!(value["tags"][0]["name"], "users");
    assert_eq!(value["tags"][0]["description"], "User management");
}

#[test]
fn supports_metadata_overrides_and_raw_bodies() {
    async fn upload(body: String) -> String {
        body
    }

    let (_, document) = Router::<()>::new()
        .route(
            "/upload",
            post(upload)
                .operation_id("uploadFile")
                .summary("Upload a file")
                .description("Stores the request body.")
                .tag("files")
                .security_scopes("oauth", &["files:write"])
                .deprecated(true)
                .response_with::<common::User>(201, "Created")
                .empty_response(202, "Accepted")
                .raw_response(
                    400,
                    "application/problem+json",
                    json!({"type": "object"}),
                    "Problem",
                ),
        )
        .finish();
    let operation = &document.as_value()["paths"]["/upload"]["post"];

    assert_eq!(operation["operationId"], "uploadFile");
    assert_eq!(operation["summary"], "Upload a file");
    assert_eq!(operation["description"], "Stores the request body.");
    assert_eq!(operation["tags"], json!(["files"]));
    assert_eq!(operation["deprecated"], true);
    assert_eq!(operation["security"], json!([{ "oauth": ["files:write"] }]));
    assert_eq!(
        operation["requestBody"]["content"]["text/plain"]["schema"]["type"],
        "string"
    );
    assert_eq!(
        operation["responses"]["200"]["content"]["text/plain"]["schema"]["type"],
        "string"
    );
    assert_eq!(
        operation["responses"]["201"]["content"]["application/json"]["schema"]["$ref"],
        "#/components/schemas/User"
    );
    assert_eq!(operation["responses"]["202"]["description"], "Accepted");
    assert_eq!(
        operation["responses"]["400"]["content"]["application/problem+json"]["schema"],
        json!({"type": "object"})
    );
}

#[test]
fn captures_dynamic_status_tuple_responses() {
    async fn create(Path(id): Path<u64>) -> (StatusCode, Json<common::User>) {
        (
            StatusCode::CREATED,
            Json(common::User {
                id,
                name: "new".to_owned(),
            }),
        )
    }

    let (_, document) = Router::<()>::new()
        .route("/things/{id}", get(create))
        .finish();
    let responses = &document.as_value()["paths"]["/things/{id}"]["get"]["responses"];

    assert_eq!(
        responses["default"]["content"]["application/json"]["schema"]["$ref"],
        "#/components/schemas/User"
    );
}

#[test]
fn captures_flattened_query_parameters() {
    #[derive(Deserialize, schemars::JsonSchema)]
    #[allow(dead_code)]
    struct Paging {
        limit: Option<u32>,
    }

    #[derive(Deserialize, schemars::JsonSchema)]
    #[allow(dead_code)]
    struct Flattened {
        #[serde(flatten)]
        paging: Paging,
        q: String,
    }

    async fn flattened(Query(_): Query<Flattened>) -> Json<String> {
        Json("ok".to_owned())
    }

    let (_, document) = Router::<()>::new()
        .route("/search", get(flattened))
        .finish();
    let operation = &document.as_value()["paths"]["/search"]["get"];

    let limit = parameter(operation, "limit");
    assert!(limit.get("required").is_none());
    let q = parameter(operation, "q");
    assert_eq!(q["required"], true);
    assert_eq!(q["schema"]["type"], "string");
}

#[test]
fn captures_query_parameters() {
    let (_, document) = common::app();
    let operation = &document.as_value()["paths"]["/users"]["get"];

    assert_eq!(operation["summary"], "List users");
    assert_eq!(operation["tags"], json!(["users"]));
    assert!(operation.get("requestBody").is_none());

    let limit = parameter(operation, "limit");
    assert_eq!(limit["in"], "query");
    assert!(limit.get("required").is_none());
    assert_eq!(limit["schema"]["type"], json!(["integer", "null"]));

    let tags = parameter(operation, "tags");
    assert_eq!(tags["required"], true);
    assert_eq!(tags["schema"]["type"], "string");

    let filter = parameter(operation, "filter");
    assert_eq!(filter["style"], "deepObject");
    assert_eq!(filter["explode"], true);
    assert!(filter["schema"]["anyOf"].is_array());
}

#[test]
fn captures_array_query_parameters() {
    #[derive(Deserialize, schemars::JsonSchema)]
    struct ArrayQuery {
        tags: Vec<String>,
    }

    async fn arrays(Query(query): Query<ArrayQuery>) -> Json<Vec<String>> {
        Json(query.tags)
    }

    let (_, document) = Router::<()>::new().route("/arrays", get(arrays)).finish();
    let tags = parameter(&document.as_value()["paths"]["/arrays"]["get"], "tags");

    assert_eq!(tags["required"], true);
    assert_eq!(tags["style"], "form");
    assert_eq!(tags["explode"], true);
    assert_eq!(tags["schema"]["type"], "array");
    assert_eq!(tags["schema"]["items"]["type"], "string");
}

#[test]
fn captures_json_body_and_response() {
    let (_, document) = common::app();
    let operation = &document.as_value()["paths"]["/users"]["post"];

    assert_eq!(operation["operationId"], "createUser");
    assert_eq!(operation["description"], "Creates a user from a JSON body.");
    assert_eq!(operation["requestBody"]["required"], true);
    assert_eq!(
        operation["requestBody"]["content"]["application/json"]["schema"]["$ref"],
        "#/components/schemas/CreateUser"
    );
    assert_eq!(
        operation["responses"]["200"]["content"]["application/json"]["schema"]["$ref"],
        "#/components/schemas/User"
    );
    assert_eq!(operation["responses"]["200"]["description"], "OK");
}

#[test]
fn captures_path_parameters() {
    let (_, document) = common::app();
    let operation = &document.as_value()["paths"]["/users/{id}"]["get"];

    let id = parameter(operation, "id");
    assert_eq!(id["in"], "path");
    assert_eq!(id["required"], true);
    assert_eq!(id["schema"]["type"], "integer");
    assert_eq!(id["schema"]["format"], "uint64");
    assert_eq!(operation["security"], json!([{ "bearer": [] }]));
}

#[test]
fn documents_wildcard_paths() {
    async fn files(Path(rest): Path<String>) -> Json<String> {
        Json(rest)
    }

    let (_, document) = Router::<()>::new()
        .route("/files/{*rest}", get(files))
        .finish();
    let paths = &document.as_value()["paths"];

    assert!(paths.get("/files/{*rest}").is_none());
    let rest = parameter(&paths["/files/{rest}"]["get"], "rest");
    assert_eq!(rest["in"], "path");
    assert_eq!(rest["required"], true);
    assert_eq!(rest["schema"]["type"], "string");
}

#[test]
fn captures_empty_responses() {
    let (_, document) = common::app();
    let operation = &document.as_value()["paths"]["/users/{id}"]["delete"];

    // `StatusCode` carries its value at runtime, so the generated document
    // uses the `default` response instead of guessing 204.
    assert_eq!(
        operation["responses"]["default"]["description"],
        "Response with a runtime-selected status code"
    );
    assert!(operation["responses"]["default"].get("content").is_none());
}

#[test]
fn captures_result_success_and_error_branches() {
    let (_, document) = common::app();
    let operation = &document.as_value()["paths"]["/failure/{id}"]["get"];

    assert_eq!(
        operation["responses"]["200"]["content"]["application/json"]["schema"]["$ref"],
        "#/components/schemas/User"
    );
    assert_eq!(
        operation["responses"]["default"]["description"],
        "Response with a runtime-selected status code"
    );
    assert_eq!(
        operation["responses"]["404"]["content"]["application/json"]["schema"]["$ref"],
        "#/components/schemas/ApiError"
    );
}

#[test]
fn hoists_components_without_titles() {
    let (_, document) = common::app();
    let schemas = &document.as_value()["components"]["schemas"];

    for name in ["User", "CreateUser", "ApiError"] {
        let schema = &schemas[name];
        assert_eq!(schema["type"], "object", "{name} should be an object");
        assert!(schema.get("title").is_none(), "{name} should have no title");
        assert!(
            schema.get("$schema").is_none(),
            "{name} should have no $schema"
        );
        assert!(schema.get("$defs").is_none(), "{name} should have no $defs");
    }

    assert_eq!(
        schemas["CreateUser"]["properties"]["name"]["type"],
        "string"
    );
}

#[test]
fn documents_struct_path_parameters() {
    let (_, document) = Router::<()>::new()
        .route("/orgs/{org}/users/{id}", get(common::path_ids))
        .finish();

    let operation = &document.as_value()["paths"]["/orgs/{org}/users/{id}"]["get"];
    let parameters = operation["parameters"].as_array().unwrap();
    let names: Vec<_> = parameters
        .iter()
        .map(|parameter| parameter["name"].as_str().unwrap())
        .collect();
    assert_eq!(names, ["org", "id"]);
    assert_eq!(parameters[0]["schema"]["type"], "string");
    assert_eq!(parameters[1]["schema"]["type"], "integer");
}

#[test]
fn documents_tuple_path_parameters() {
    let (_, document) = Router::<()>::new()
        .route("/{category}/{id}", get(common::path_pair))
        .finish();

    let operation = &document.as_value()["paths"]["/{category}/{id}"]["get"];
    assert_eq!(
        operation["parameters"],
        json!([
            {
                "in": "path",
                "name": "category",
                "required": true,
                "schema": { "type": "string" }
            },
            {
                "in": "path",
                "name": "id",
                "required": true,
                "schema": { "type": "integer", "format": "uint64", "minimum": 0 }
            }
        ])
    );
}

#[test]
fn nested_and_merged_paths_are_documented() {
    let (_, document) = common::nested_app();
    let paths = &document.as_value()["paths"];

    assert!(paths.get("/v1/items").is_some());
    assert!(paths.get("/v1/items/{id}").is_some());
    assert!(paths.get("/health").is_some());
    assert!(paths.get("/items").is_none());
    assert_eq!(
        paths["/v1/items/{id}"]["get"]["parameters"][0]["name"],
        "id"
    );
}

#[test]
fn with_state_keeps_documentation() {
    let (_, document) = common::stateful_app();
    let operation = &document.as_value()["paths"]["/state"]["get"];

    assert!(operation.get("parameters").is_none());
    assert_eq!(
        operation["responses"]["200"]["content"]["application/json"]["schema"]["type"],
        "string"
    );
}

#[test]
fn primitive_and_array_responses_stay_inline() {
    let (_, document) = Router::<()>::new()
        .route("/health", get(common::health))
        .finish();
    let value = document.as_value();
    assert!(value.get("components").is_none());

    async fn list() -> Json<Vec<common::User>> {
        Json(Vec::new())
    }

    let (_, document) = Router::<()>::new().route("/list", get(list)).finish();
    let schema = &document.as_value()["paths"]["/list"]["get"]["responses"]["200"]["content"]
        ["application/json"]["schema"];
    assert_eq!(schema["type"], "array");
    assert_eq!(schema["items"]["$ref"], "#/components/schemas/User");
}

#[test]
fn document_generation_is_deterministic() {
    let (_, first) = common::app();
    let (_, second) = common::app();
    assert_eq!(first.to_json(), second.to_json());
}

#[tokio::test]
async fn serves_the_openapi_json_route() {
    let (app, _) = Router::<()>::new()
        .route("/health", get(common::health))
        .openapi_json_route("/openapi.json")
        .finish();

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
    let bytes = axum::body::to_bytes(response.into_body(), usize::MAX)
        .await
        .unwrap();
    let value: Value = serde_json::from_slice(&bytes).unwrap();
    assert_eq!(value["openapi"], "3.1.0");
    assert_eq!(
        value["paths"]["/health"]["get"]["responses"]["200"]["description"],
        "OK"
    );
    assert!(value["paths"].get("/openapi.json").is_none());
}

#[test]
fn finish_json_returns_the_same_document() {
    let (_, value) = Router::<()>::new()
        .route("/health", get(common::health))
        .finish_json();

    assert_eq!(value["openapi"], "3.1.0");
    assert_eq!(
        value["paths"]["/health"]["get"]["responses"]["200"]["description"],
        "OK"
    );
}

#[test]
#[should_panic(expected = "duplicate OpenAPI operation `get` for path `/x`")]
fn duplicate_routes_panic() {
    let _ = Router::<()>::new()
        .route("/x", get(common::health))
        .route("/x", get(common::health));
}

#[test]
#[should_panic(expected = "describes parameter `id`")]
fn mismatched_path_extractor_panics() {
    let _ = Router::<()>::new()
        .route("/users/{user_id}", get(common::path_ids))
        .finish();
}

#[test]
#[should_panic(expected = "has 2 element(s), but the route declares 1")]
fn tuple_length_mismatch_panics() {
    let _ = Router::<()>::new()
        .route("/users/{id}", get(common::path_pair))
        .finish();
}

#[test]
#[should_panic(expected = "must deserialize from an object")]
fn scalar_query_extractor_panics() {
    async fn scalar_query(Query(value): Query<u64>) -> Json<u64> {
        Json(value)
    }

    let _ = Router::<()>::new()
        .route("/scalar", get(scalar_query))
        .finish();
}

#[test]
#[should_panic(expected = "conflicting query parameter `limit`")]
fn conflicting_query_parameters_panic() {
    #[derive(Deserialize, schemars::JsonSchema)]
    #[allow(dead_code)]
    struct First {
        limit: u32,
    }

    #[derive(Deserialize, schemars::JsonSchema)]
    #[allow(dead_code)]
    struct Second {
        limit: String,
    }

    async fn conflicting(Query(_): Query<First>, Query(_): Query<Second>) {}

    let _ = Router::<()>::new()
        .route("/conflict", get(conflicting))
        .finish();
}
