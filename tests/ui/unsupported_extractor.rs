use axum::extract::FromRequestParts;
use axum::http::request::Parts;
use axum_openapi::routing::get;

struct Custom;

impl<S: Send + Sync> FromRequestParts<S> for Custom {
    type Rejection = std::convert::Infallible;

    async fn from_request_parts(_: &mut Parts, _: &S) -> Result<Self, Self::Rejection> {
        Ok(Custom)
    }
}

async fn handler(_: Custom) {}

fn main() {
    let _ = get(handler);
}
