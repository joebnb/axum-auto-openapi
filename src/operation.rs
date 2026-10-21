//! Operation descriptors: the metadata captured from a handler.
//!
//! A descriptor is created when a handler is registered and stays unresolved
//! until the document is finished. Deferring schema generation lets `nest` and
//! `merge` move operations around without rewriting references, and lets path
//! parameters be matched against the final route template.

use std::collections::BTreeMap;

use axum::http::{Method, StatusCode};
use schemars::JsonSchema;
use serde_json::{json, Value};

use crate::schema::SchemaOf;

/// Where a response is being described from.
///
/// [`ApiOutput`](crate::ApiOutput) is used for successful values, error values
/// and response tuples whose status code is only known at runtime.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ResponseKind {
    /// The success branch of a handler, documented as `200`.
    Success,
    /// The error branch of a `Result`, documented as `default`.
    Error,
    /// A response whose status code is chosen at runtime, documented as `default`.
    Dynamic,
    /// A response with a known HTTP status code.
    Status(u16),
}

impl ResponseKind {
    /// The OpenAPI response key for this kind (`"200"`, `"404"`, `"default"`...).
    pub fn status_key(self) -> String {
        match self {
            ResponseKind::Success => "200".to_owned(),
            ResponseKind::Error | ResponseKind::Dynamic => "default".to_owned(),
            ResponseKind::Status(code) => code.to_string(),
        }
    }
}

/// A response key: a numeric status code, a range such as `"2XX"`, or
/// `"default"`.
#[derive(Clone, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct StatusKey(String);

impl StatusKey {
    /// The OpenAPI `default` response.
    pub fn default_response() -> Self {
        Self("default".to_owned())
    }

    /// A numeric status code.
    pub fn code(code: u16) -> Self {
        Self(code.to_string())
    }

    /// The key as it appears in the document.
    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl From<u16> for StatusKey {
    fn from(code: u16) -> Self {
        Self::code(code)
    }
}

impl From<StatusCode> for StatusKey {
    fn from(status: StatusCode) -> Self {
        Self::code(status.as_u16())
    }
}

impl From<&str> for StatusKey {
    fn from(key: &str) -> Self {
        Self(key.to_owned())
    }
}

impl From<String> for StatusKey {
    fn from(key: String) -> Self {
        Self(key)
    }
}

impl std::fmt::Display for StatusKey {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter.write_str(&self.0)
    }
}

pub(crate) fn default_response_description(status: &str) -> String {
    match status {
        "default" => "Default response".to_owned(),
        key if key.len() == 3 && key.ends_with("XX") => format!("{key} response"),
        key => key
            .parse::<u16>()
            .ok()
            .and_then(|code| StatusCode::from_u16(code).ok())
            .and_then(|status| status.canonical_reason())
            .map(str::to_owned)
            .unwrap_or_else(|| "Response".to_owned()),
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum ParameterSource {
    Path,
    Query,
}

#[derive(Clone, Copy)]
pub(crate) struct ParameterGroup {
    pub(crate) source: ParameterSource,
    pub(crate) schema: SchemaOf,
}

#[derive(Clone)]
pub(crate) enum BodySchema {
    Typed(SchemaOf),
    Raw(Value),
}

#[derive(Clone)]
pub(crate) struct RequestBody {
    pub(crate) required: bool,
    pub(crate) content_type: String,
    pub(crate) schema: BodySchema,
}

#[derive(Clone)]
pub(crate) struct ResponseSpec {
    pub(crate) description: Option<String>,
    pub(crate) content: BTreeMap<String, BodySchema>,
}

/// Everything the OpenAPI document needs to know about one handler.
///
/// Custom extractors and response types can extend the generated document by
/// calling the `describe_*` methods on this type.
#[derive(Clone)]
pub struct OperationDescriptor {
    pub(crate) method: Method,
    pub(crate) summary: Option<String>,
    pub(crate) description: Option<String>,
    pub(crate) operation_id: Option<String>,
    pub(crate) tags: Vec<String>,
    /// Statically nested path segments, outermost first, recorded when the
    /// router is nested. Used to infer a tag from the router structure when
    /// no explicit tag was set.
    pub(crate) nest_path: Vec<String>,
    pub(crate) deprecated: bool,
    pub(crate) security: Vec<Value>,
    pub(crate) parameter_groups: Vec<ParameterGroup>,
    pub(crate) request_body: Option<RequestBody>,
    pub(crate) responses: BTreeMap<String, ResponseSpec>,
}

impl OperationDescriptor {
    pub(crate) fn new(method: Method) -> Self {
        Self {
            method,
            summary: None,
            description: None,
            operation_id: None,
            tags: Vec::new(),
            nest_path: Vec::new(),
            deprecated: false,
            security: Vec::new(),
            parameter_groups: Vec::new(),
            request_body: None,
            responses: BTreeMap::new(),
        }
    }

    /// The HTTP method this operation handles.
    pub fn method(&self) -> &Method {
        &self.method
    }

    /// Set a short summary.
    pub fn summary(&mut self, summary: impl Into<String>) {
        self.summary = Some(summary.into());
    }

    /// Set a longer description.
    pub fn description(&mut self, description: impl Into<String>) {
        self.description = Some(description.into());
    }

    /// Set the operation id used by generated clients.
    pub fn operation_id(&mut self, operation_id: impl Into<String>) {
        self.operation_id = Some(operation_id.into());
    }

    /// Add a tag. Duplicate tags are ignored.
    pub fn tag(&mut self, tag: impl Into<String>) {
        let tag = tag.into();
        if !self.tags.contains(&tag) {
            self.tags.push(tag);
        }
    }

    /// Require a security scheme for this operation.
    pub fn security(&mut self, scheme: impl Into<String>) {
        self.security_scopes(scheme, &[]);
    }

    /// Require a security scheme and the given OAuth2/OpenID scopes.
    pub fn security_scopes(&mut self, scheme: impl Into<String>, scopes: &[&str]) {
        let scheme = scheme.into();
        self.security.retain(|requirement| {
            requirement
                .as_object()
                .is_none_or(|object| !object.contains_key(&scheme))
        });
        let mut requirement = serde_json::Map::new();
        requirement.insert(scheme, json!(scopes));
        self.security.push(Value::Object(requirement));
    }

    /// Mark the operation as deprecated.
    pub fn deprecated(&mut self, deprecated: bool) {
        self.deprecated = deprecated;
    }

    /// Describe a `Path<T>` extractor.
    pub fn describe_path<T: JsonSchema + 'static>(&mut self) {
        self.parameter_groups.push(ParameterGroup {
            source: ParameterSource::Path,
            schema: SchemaOf::of::<T>(),
        });
    }

    /// Describe a `Query<T>` extractor.
    pub fn describe_query<T: JsonSchema + 'static>(&mut self) {
        self.parameter_groups.push(ParameterGroup {
            source: ParameterSource::Query,
            schema: SchemaOf::of::<T>(),
        });
    }

    /// Describe a JSON request body.
    pub fn describe_json_body<T: JsonSchema + 'static>(&mut self) {
        self.request_body = Some(RequestBody {
            required: true,
            content_type: "application/json".to_owned(),
            schema: BodySchema::Typed(SchemaOf::of::<T>()),
        });
    }

    /// Describe a request body from a raw JSON Schema value.
    pub fn describe_raw_body(
        &mut self,
        content_type: impl Into<String>,
        required: bool,
        schema: Value,
    ) {
        self.request_body = Some(RequestBody {
            required,
            content_type: content_type.into(),
            schema: BodySchema::Raw(schema),
        });
    }

    /// Add a response with a JSON body.
    pub fn json_response<T: JsonSchema + 'static>(
        &mut self,
        status: impl Into<StatusKey>,
        description: impl Into<String>,
    ) {
        self.insert_response(
            status.into(),
            Some(description.into()),
            "application/json",
            BodySchema::Typed(SchemaOf::of::<T>()),
        );
    }

    /// Add a response without a body.
    pub fn empty_response(&mut self, status: impl Into<StatusKey>, description: impl Into<String>) {
        self.insert_empty_response(status.into(), Some(description.into()));
    }

    /// Add a response with an arbitrary content type and raw schema.
    pub fn raw_response(
        &mut self,
        status: impl Into<StatusKey>,
        description: impl Into<String>,
        content_type: impl Into<String>,
        schema: Value,
    ) {
        self.insert_response(
            status.into(),
            Some(description.into()),
            content_type,
            BodySchema::Raw(schema),
        );
    }

    pub(crate) fn insert_empty_response(&mut self, status: StatusKey, description: Option<String>) {
        self.responses
            .entry(status.0)
            .or_insert_with(|| ResponseSpec {
                description,
                content: BTreeMap::new(),
            });
    }

    pub(crate) fn insert_response(
        &mut self,
        status: StatusKey,
        description: Option<String>,
        content_type: impl Into<String>,
        schema: BodySchema,
    ) {
        let response = self
            .responses
            .entry(status.0)
            .or_insert_with(|| ResponseSpec {
                description: description.clone(),
                content: BTreeMap::new(),
            });
        if response.description.is_none() {
            response.description = description;
        }
        response.content.insert(content_type.into(), schema);
    }
}
