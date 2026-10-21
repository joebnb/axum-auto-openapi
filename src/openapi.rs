//! OpenAPI document model and rendering.

use std::collections::BTreeMap;
use std::fmt;

use serde_json::{json, Map, Value};

use crate::operation::{default_response_description, BodySchema, OperationDescriptor};
use crate::params;
use crate::schema::SchemaRegistry;

/// API metadata rendered into the `info` object.
#[derive(Clone, Debug)]
pub struct Info {
    title: String,
    version: String,
    description: Option<String>,
    terms_of_service: Option<String>,
    contact: Option<Value>,
    license: Option<Value>,
}

impl Info {
    /// Create the required OpenAPI `info` fields.
    pub fn new(title: impl Into<String>, version: impl Into<String>) -> Self {
        Self {
            title: title.into(),
            version: version.into(),
            description: None,
            terms_of_service: None,
            contact: None,
            license: None,
        }
    }

    /// Set the API description.
    pub fn description(mut self, description: impl Into<String>) -> Self {
        self.description = Some(description.into());
        self
    }

    /// Set the terms of service URL.
    pub fn terms_of_service(mut self, url: impl Into<String>) -> Self {
        self.terms_of_service = Some(url.into());
        self
    }

    /// Set contact information.
    pub fn contact(
        mut self,
        name: impl Into<String>,
        email: impl Into<String>,
        url: impl Into<String>,
    ) -> Self {
        self.contact = Some(json!({
            "name": name.into(),
            "email": email.into(),
            "url": url.into(),
        }));
        self
    }

    /// Set license information.
    pub fn license(mut self, name: impl Into<String>, url: impl Into<String>) -> Self {
        self.license = Some(json!({
            "name": name.into(),
            "url": url.into(),
        }));
        self
    }

    fn into_value(self) -> Value {
        let mut info = Map::new();
        info.insert("title".to_owned(), json!(self.title));
        info.insert("version".to_owned(), json!(self.version));
        if let Some(description) = self.description {
            info.insert("description".to_owned(), json!(description));
        }
        if let Some(terms) = self.terms_of_service {
            info.insert("termsOfService".to_owned(), json!(terms));
        }
        if let Some(contact) = self.contact {
            info.insert("contact".to_owned(), contact);
        }
        if let Some(license) = self.license {
            info.insert("license".to_owned(), license);
        }
        Value::Object(info)
    }
}

/// A server entry rendered into the document's `servers` array.
#[derive(Clone, Debug)]
pub struct Server {
    url: String,
    description: Option<String>,
}

impl Server {
    /// Create a server entry from its base URL.
    pub fn new(url: impl Into<String>) -> Self {
        Self {
            url: url.into(),
            description: None,
        }
    }

    /// Set a human readable description.
    pub fn description(mut self, description: impl Into<String>) -> Self {
        self.description = Some(description.into());
        self
    }

    fn into_value(self) -> Value {
        let mut server = Map::new();
        server.insert("url".to_owned(), json!(self.url));
        if let Some(description) = self.description {
            server.insert("description".to_owned(), json!(description));
        }
        Value::Object(server)
    }
}

/// A top-level tag entry.
#[derive(Clone, Debug)]
pub struct Tag {
    name: String,
    description: Option<String>,
}

impl Tag {
    /// Create a tag from its name.
    pub fn new(name: impl Into<String>) -> Self {
        Self {
            name: name.into(),
            description: None,
        }
    }

    /// Set a human readable description.
    pub fn description(mut self, description: impl Into<String>) -> Self {
        self.description = Some(description.into());
        self
    }

    fn into_value(self) -> Value {
        let mut tag = Map::new();
        tag.insert("name".to_owned(), json!(self.name));
        if let Some(description) = description_into(self.description) {
            tag.insert("description".to_owned(), description);
        }
        Value::Object(tag)
    }
}

fn description_into(description: Option<String>) -> Option<Value> {
    description.map(|description| json!(description))
}

/// Where an API key is sent.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ApiKeyLocation {
    /// The `Authorization` (or another) header.
    Header,
    /// A query parameter.
    Query,
    /// A cookie.
    Cookie,
}

impl ApiKeyLocation {
    fn as_str(self) -> &'static str {
        match self {
            ApiKeyLocation::Header => "header",
            ApiKeyLocation::Query => "query",
            ApiKeyLocation::Cookie => "cookie",
        }
    }
}

/// A security scheme registered under `components.securitySchemes`.
#[derive(Clone, Debug)]
pub struct SecurityScheme(Value);

impl SecurityScheme {
    /// HTTP bearer authentication.
    pub fn bearer() -> Self {
        Self(json!({ "type": "http", "scheme": "bearer" }))
    }

    /// HTTP bearer authentication with a bearer format hint.
    pub fn bearer_with_format(format: impl Into<String>) -> Self {
        Self(json!({
            "type": "http",
            "scheme": "bearer",
            "bearerFormat": format.into(),
        }))
    }

    /// HTTP basic authentication.
    pub fn basic() -> Self {
        Self(json!({ "type": "http", "scheme": "basic" }))
    }

    /// API key authentication.
    pub fn api_key(name: impl Into<String>, location: ApiKeyLocation) -> Self {
        Self(json!({
            "type": "apiKey",
            "name": name.into(),
            "in": location.as_str(),
        }))
    }

    /// OpenID Connect discovery.
    pub fn open_id_connect(url: impl Into<String>) -> Self {
        Self(json!({
            "type": "openIdConnect",
            "openIdConnectUrl": url.into(),
        }))
    }

    /// Escape hatch for any other security scheme.
    pub fn custom(value: Value) -> Self {
        Self(value)
    }

    pub(crate) fn into_value(self) -> Value {
        self.0
    }
}

/// A finished OpenAPI 3.1 document.
#[derive(Clone, Debug)]
pub struct OpenApiDocument {
    value: Value,
}

impl OpenApiDocument {
    /// Clone the document as a JSON value.
    pub fn to_value(&self) -> Value {
        self.value.clone()
    }

    /// Borrow the document as a JSON value.
    pub fn as_value(&self) -> &Value {
        &self.value
    }

    /// Serialize the document as compact JSON.
    pub fn to_json(&self) -> String {
        serde_json::to_string(&self.value).expect("OpenAPI documents serialize to JSON")
    }

    /// Serialize the document as pretty-printed JSON.
    pub fn to_json_pretty(&self) -> String {
        serde_json::to_string_pretty(&self.value).expect("OpenAPI documents serialize to JSON")
    }
}

impl fmt::Display for OpenApiDocument {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(&self.to_json())
    }
}

pub(crate) fn build_document(
    info: Info,
    servers: Vec<Server>,
    tags: Vec<Tag>,
    security_schemes: BTreeMap<String, Value>,
    paths: BTreeMap<String, BTreeMap<String, OperationDescriptor>>,
) -> OpenApiDocument {
    let mut registry = SchemaRegistry::default();
    let mut rendered_paths = Map::new();

    for (path, operations) in &paths {
        let documented_path = params::documented_path(path);
        let mut path_item = Map::new();
        for (method, operation) in operations {
            path_item.insert(
                method.clone(),
                render_operation(&documented_path, operation, &mut registry),
            );
        }
        rendered_paths.insert(documented_path, Value::Object(path_item));
    }

    let mut document = Map::new();
    document.insert("openapi".to_owned(), json!("3.1.0"));
    document.insert("info".to_owned(), info.into_value());
    document.insert("paths".to_owned(), Value::Object(rendered_paths));

    if !servers.is_empty() {
        document.insert(
            "servers".to_owned(),
            Value::Array(servers.into_iter().map(Server::into_value).collect()),
        );
    }
    if !tags.is_empty() {
        document.insert(
            "tags".to_owned(),
            Value::Array(tags.into_iter().map(Tag::into_value).collect()),
        );
    }

    let schemas = registry.into_components();
    if !schemas.is_empty() || !security_schemes.is_empty() {
        let mut components = Map::new();
        if !schemas.is_empty() {
            components.insert("schemas".to_owned(), json!(schemas));
        }
        if !security_schemes.is_empty() {
            components.insert("securitySchemes".to_owned(), json!(security_schemes));
        }
        document.insert("components".to_owned(), Value::Object(components));
    }

    OpenApiDocument {
        value: Value::Object(document),
    }
}

fn render_operation(
    path: &str,
    operation: &OperationDescriptor,
    registry: &mut SchemaRegistry,
) -> Value {
    let mut rendered = Map::new();

    if let Some(operation_id) = &operation.operation_id {
        rendered.insert("operationId".to_owned(), json!(operation_id));
    }
    if let Some(summary) = &operation.summary {
        rendered.insert("summary".to_owned(), json!(summary));
    }
    if let Some(description) = &operation.description {
        rendered.insert("description".to_owned(), json!(description));
    }
    if !operation.tags.is_empty() {
        rendered.insert("tags".to_owned(), json!(operation.tags));
    }
    if operation.deprecated {
        rendered.insert("deprecated".to_owned(), json!(true));
    }
    if !operation.security.is_empty() {
        rendered.insert("security".to_owned(), json!(operation.security));
    }

    let parameters = params::render_parameters(path, operation);
    if !parameters.is_empty() {
        rendered.insert("parameters".to_owned(), json!(parameters));
    }

    if let Some(body) = &operation.request_body {
        let schema = render_body_schema(&body.schema, registry);
        let mut content = Map::new();
        content.insert(body.content_type.clone(), json!({ "schema": schema }));

        let mut request_body = Map::new();
        request_body.insert("required".to_owned(), json!(body.required));
        request_body.insert("content".to_owned(), Value::Object(content));
        rendered.insert("requestBody".to_owned(), Value::Object(request_body));
    }

    rendered.insert(
        "responses".to_owned(),
        render_responses(&operation.responses, registry),
    );

    Value::Object(rendered)
}

fn render_responses(
    responses: &BTreeMap<String, crate::operation::ResponseSpec>,
    registry: &mut SchemaRegistry,
) -> Value {
    let mut rendered = Map::new();

    for (status, response) in responses {
        let mut value = Map::new();
        let description = response
            .description
            .clone()
            .unwrap_or_else(|| default_response_description(status));
        value.insert("description".to_owned(), json!(description));

        if !response.content.is_empty() {
            let mut content = Map::new();
            for (content_type, schema) in &response.content {
                content.insert(
                    content_type.clone(),
                    json!({ "schema": render_body_schema(schema, registry) }),
                );
            }
            value.insert("content".to_owned(), Value::Object(content));
        }

        rendered.insert(status.clone(), Value::Object(value));
    }

    if rendered.is_empty() {
        rendered.insert(
            "default".to_owned(),
            json!({ "description": "Default response" }),
        );
    }

    Value::Object(rendered)
}

fn render_body_schema(schema: &BodySchema, registry: &mut SchemaRegistry) -> Value {
    match schema {
        BodySchema::Typed(schema) => registry.reference(*schema),
        BodySchema::Raw(schema) => schema.clone(),
    }
}

#[cfg(feature = "swagger-ui")]
pub(crate) fn swagger_ui_html(spec_url: &str) -> String {
    let spec_url = spec_url.replace('\\', "\\\\").replace('"', "\\\"");
    format!(
        r##"<!doctype html>
<html lang="en">
<head>
  <meta charset="utf-8">
  <meta name="viewport" content="width=device-width, initial-scale=1">
  <title>API documentation</title>
  <link rel="stylesheet" href="https://unpkg.com/swagger-ui-dist@5/swagger-ui.css">
</head>
<body>
  <div id="swagger-ui"></div>
  <script src="https://unpkg.com/swagger-ui-dist@5/swagger-ui-bundle.js"></script>
  <script>
    window.addEventListener("load", function () {{
      SwaggerUIBundle({{ url: "{spec_url}", dom_id: "#swagger-ui" }});
    }});
  </script>
</body>
</html>
"##
    )
}
