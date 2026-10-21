use axum_auto_openapi::routing::get;

async fn handler() -> impl axum::response::IntoResponse {
    "ok"
}

fn main() {
    let _ = get(handler);
}
