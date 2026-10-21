//! Static analysis of handler signatures.
//!
//! [`AutoHandler`] mirrors axum's own `Handler<T, S>` marker-tuple scheme. The
//! extra signature parameter disambiguates arities so the blanket impls do not
//! overlap, while axum keeps validating that every extractor is usable in its
//! position and that the handler future is `Send`.

use std::future::Future;

use axum::handler::Handler;
use axum::response::IntoResponse;

use crate::operation::{OperationDescriptor, ResponseKind};
use crate::{ApiInput, ApiOutput};

/// A handler whose extractors and return type can be described statically.
///
/// You should not implement this trait manually. [`routing::get`](crate::routing::get)
/// and friends require it automatically.
#[diagnostic::on_unimplemented(
    message = "`{Self}` has an input or output type that cannot be described in OpenAPI",
    label = "unsupported handler signature",
    note = "every extractor must implement `axum_auto_openapi::ApiInput`, and the return type must implement `axum_auto_openapi::ApiOutput`",
    note = "supported return types include `Json<T>`, `Result<T, E>`, `(StatusCode, T)`, `()`, `String`, and `Html<_>`; opaque `impl IntoResponse` return types cannot be inspected",
    note = "implement `ApiInput` / `ApiOutput` for custom extractors and response wrappers"
)]
pub trait AutoHandler<Sig, S>: Handler<Sig, S> {
    /// Add the handler's inputs and outputs to an operation.
    fn describe(operation: &mut OperationDescriptor);
}

impl<F, Fut, R, S> AutoHandler<((),), S> for F
where
    F: Handler<((),), S> + FnOnce() -> Fut + Clone + Send + Sync + 'static,
    Fut: Future<Output = R> + Send,
    S: Send + Sync + 'static,
    R: IntoResponse + ApiOutput,
{
    fn describe(operation: &mut OperationDescriptor) {
        R::describe(operation, ResponseKind::Success);
    }
}

macro_rules! impl_auto_handler {
    ($($extractor:ident),+) => {
        impl<F, Fut, R, S, M, $($extractor,)+> AutoHandler<(M, $($extractor,)+), S> for F
        where
            F: Handler<(M, $($extractor,)+), S>
                + FnOnce($($extractor,)+) -> Fut
                + Clone
                + Send
                + Sync
                + 'static,
            Fut: Future<Output = R> + Send,
            S: Send + Sync + 'static,
            R: IntoResponse + ApiOutput,
            $($extractor: ApiInput + Send + 'static,)+
        {
            fn describe(operation: &mut OperationDescriptor) {
                $(
                    <$extractor as ApiInput>::describe(operation);
                )+
                R::describe(operation, ResponseKind::Success);
            }
        }
    };
}

impl_auto_handler!(A1);
impl_auto_handler!(A1, A2);
impl_auto_handler!(A1, A2, A3);
impl_auto_handler!(A1, A2, A3, A4);
impl_auto_handler!(A1, A2, A3, A4, A5);
impl_auto_handler!(A1, A2, A3, A4, A5, A6);
impl_auto_handler!(A1, A2, A3, A4, A5, A6, A7);
impl_auto_handler!(A1, A2, A3, A4, A5, A6, A7, A8);
impl_auto_handler!(A1, A2, A3, A4, A5, A6, A7, A8, A9);
impl_auto_handler!(A1, A2, A3, A4, A5, A6, A7, A8, A9, A10);
impl_auto_handler!(A1, A2, A3, A4, A5, A6, A7, A8, A9, A10, A11);
impl_auto_handler!(A1, A2, A3, A4, A5, A6, A7, A8, A9, A10, A11, A12);
impl_auto_handler!(A1, A2, A3, A4, A5, A6, A7, A8, A9, A10, A11, A12, A13);
impl_auto_handler!(A1, A2, A3, A4, A5, A6, A7, A8, A9, A10, A11, A12, A13, A14);
impl_auto_handler!(A1, A2, A3, A4, A5, A6, A7, A8, A9, A10, A11, A12, A13, A14, A15);
impl_auto_handler!(A1, A2, A3, A4, A5, A6, A7, A8, A9, A10, A11, A12, A13, A14, A15, A16);
