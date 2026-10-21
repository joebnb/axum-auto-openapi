//! Resolve recorded extractor schemas against the final route template.
//!
//! Resolution happens in [`crate::Router::finish`] because `nest` may prefix a
//! route (possibly adding path parameters) after the operation was registered.

use std::collections::{BTreeMap, BTreeSet};

use serde_json::{json, Map, Value};

use crate::operation::{OperationDescriptor, ParameterSource};
use crate::schema::inline_defs;

pub(crate) fn render_parameters(path: &str, operation: &OperationDescriptor) -> Vec<Value> {
    let captures = path_captures(path);
    let mut path_parameters: BTreeMap<String, Value> = BTreeMap::new();
    let mut query_parameters: BTreeMap<String, Value> = BTreeMap::new();

    for group in &operation.parameter_groups {
        let schema = inline_defs(group.schema.raw());
        match group.source {
            ParameterSource::Path => {
                describe_path(path, operation, &captures, &schema, &mut path_parameters);
            }
            ParameterSource::Query => {
                describe_query(path, operation, &schema, &mut query_parameters);
            }
        }
    }

    // Parameters inherited from a `nest` prefix (or wildcards) do not need a
    // typed extractor, but they still have to be declared.
    for capture in &captures {
        path_parameters.entry(capture.clone()).or_insert_with(|| {
            json!({
                "in": "path",
                "name": capture,
                "required": true,
                "description": "Path parameter without a typed extractor",
                "schema": { "type": "string" }
            })
        });
    }

    let mut parameters = Vec::with_capacity(path_parameters.len() + query_parameters.len());
    for capture in &captures {
        if let Some(parameter) = path_parameters.remove(capture) {
            parameters.push(parameter);
        }
    }
    parameters.extend(path_parameters.into_values());
    parameters.extend(query_parameters.into_values());
    parameters
}

fn describe_path(
    path: &str,
    operation: &OperationDescriptor,
    captures: &[String],
    schema: &Value,
    parameters: &mut BTreeMap<String, Value>,
) {
    if captures.is_empty() {
        panic!(
            "Path extractor on {} `{path}` requires path parameters, but the route has none",
            operation.method
        );
    }

    if let Some((properties, _)) = object_properties(schema) {
        for (name, property) in properties {
            if !captures.contains(&name) {
                panic!(
                    "Path extractor on {} `{path}` describes parameter `{name}`, but the route \
                     only declares {captures:?}",
                    operation.method
                );
            }
            let mut property = property;
            let description = take_description(&mut property);
            insert_path_parameter(
                parameters,
                name.clone(),
                make_path_parameter(&name, property, description),
                path,
                operation,
            );
        }
        return;
    }

    if let Some(items) = schema.get("prefixItems").and_then(Value::as_array) {
        if items.len() != captures.len() {
            panic!(
                "Path tuple on {} `{path}` has {} element(s), but the route declares {} path \
                 parameter(s) {captures:?}",
                operation.method,
                items.len(),
                captures.len()
            );
        }
        for (name, item) in captures.iter().zip(items) {
            let mut item = item.clone();
            let description = take_description(&mut item);
            insert_path_parameter(
                parameters,
                name.clone(),
                make_path_parameter(name, item, description),
                path,
                operation,
            );
        }
        return;
    }

    if schema.get("type").and_then(Value::as_str) == Some("array") {
        let item = schema
            .get("items")
            .cloned()
            .unwrap_or_else(|| json!({ "type": "string" }));
        for name in captures {
            insert_path_parameter(
                parameters,
                name.clone(),
                make_path_parameter(name, item.clone(), None),
                path,
                operation,
            );
        }
        return;
    }

    if captures.len() != 1 {
        panic!(
            "Path extractor on {} `{path}` uses a single value, but the route declares {} path \
             parameters {captures:?}; use `Path<(..)>` or a struct with matching field names",
            operation.method,
            captures.len()
        );
    }

    let mut schema = schema.clone();
    let description = take_description(&mut schema);
    let name = captures[0].clone();
    insert_path_parameter(
        parameters,
        name.clone(),
        make_path_parameter(&name, schema, description),
        path,
        operation,
    );
}

fn insert_path_parameter(
    parameters: &mut BTreeMap<String, Value>,
    name: String,
    parameter: Value,
    path: &str,
    operation: &OperationDescriptor,
) {
    if parameters.insert(name.clone(), parameter).is_some() {
        panic!(
            "duplicate path parameter `{name}` on {} `{path}`: multiple Path extractors describe \
             the same parameter",
            operation.method
        );
    }
}

fn make_path_parameter(name: &str, schema: Value, description: Option<String>) -> Value {
    let mut parameter = Map::new();
    parameter.insert("in".to_owned(), json!("path"));
    parameter.insert("name".to_owned(), json!(name));
    parameter.insert("required".to_owned(), json!(true));
    if let Some(description) = description {
        parameter.insert("description".to_owned(), json!(description));
    }
    parameter.insert("schema".to_owned(), schema);
    Value::Object(parameter)
}

fn describe_query(
    path: &str,
    operation: &OperationDescriptor,
    schema: &Value,
    parameters: &mut BTreeMap<String, Value>,
) {
    let Some((properties, required)) = object_properties(schema) else {
        panic!(
            "Query extractor on {} `{path}` must deserialize from an object, but its schema is \
             not object-shaped; use a struct, a struct with `#[serde(flatten)]`, or a map",
            operation.method
        );
    };

    for (name, property) in properties {
        let mut property = property;
        let description = take_description(&mut property);

        let mut parameter = Map::new();
        parameter.insert("in".to_owned(), json!("query"));
        parameter.insert("name".to_owned(), json!(name));
        if required.contains(&name) {
            parameter.insert("required".to_owned(), json!(true));
        }
        if let Some(description) = description {
            parameter.insert("description".to_owned(), json!(description));
        }
        if is_object_schema(&property) {
            parameter.insert("style".to_owned(), json!("deepObject"));
            parameter.insert("explode".to_owned(), json!(true));
        } else if property.get("type").and_then(Value::as_str) == Some("array") {
            parameter.insert("style".to_owned(), json!("form"));
            parameter.insert("explode".to_owned(), json!(true));
        }
        parameter.insert("schema".to_owned(), property);

        let parameter = Value::Object(parameter);
        match parameters.get(&name) {
            Some(existing) if *existing != parameter => {
                panic!(
                    "conflicting query parameter `{name}` on {} `{path}`: two Query extractors \
                     declare it with different schemas",
                    operation.method
                );
            }
            Some(_) => {}
            None => {
                parameters.insert(name, parameter);
            }
        }
    }
}

/// Extract object properties and required names, merging `allOf` branches.
fn object_properties(schema: &Value) -> Option<(BTreeMap<String, Value>, BTreeSet<String>)> {
    let object = schema.as_object()?;
    let mut properties = BTreeMap::new();
    let mut required = BTreeSet::new();
    let mut saw_object = object.get("type").and_then(Value::as_str) == Some("object")
        || object.contains_key("properties");

    if let Some(map) = object.get("properties").and_then(Value::as_object) {
        saw_object = true;
        for (name, schema) in map {
            properties.insert(name.clone(), schema.clone());
        }
    }

    if let Some(names) = object.get("required").and_then(Value::as_array) {
        for name in names {
            if let Some(name) = name.as_str() {
                required.insert(name.to_owned());
            }
        }
    }

    if let Some(branches) = object.get("allOf").and_then(Value::as_array) {
        for branch in branches {
            if let Some((branch_properties, branch_required)) = object_properties(branch) {
                saw_object = true;
                properties.extend(branch_properties);
                required.extend(branch_required);
            }
        }
    }

    saw_object.then_some((properties, required))
}

fn is_object_schema(schema: &Value) -> bool {
    let Some(object) = schema.as_object() else {
        return false;
    };
    if object.get("type").and_then(Value::as_str) == Some("object")
        || object.contains_key("properties")
        || object.contains_key("additionalProperties")
    {
        return true;
    }
    // `Option<T>` renders as `anyOf: [<T>, null]`. Treat an optional object as
    // an object so it still gets `deepObject` serialization.
    ["anyOf", "oneOf"].iter().any(|keyword| {
        object
            .get(*keyword)
            .and_then(Value::as_array)
            .is_some_and(|branches| branches.iter().any(is_object_schema))
    })
}

fn take_description(schema: &mut Value) -> Option<String> {
    schema
        .as_object_mut()?
        .remove("description")?
        .as_str()
        .map(str::to_owned)
}

/// Axum 0.8 path templates use `{name}` and `{*rest}`.
pub(crate) fn path_captures(path: &str) -> Vec<String> {
    path.split('/')
        .filter_map(|segment| {
            let capture = segment.strip_prefix('{')?.strip_suffix('}')?;
            Some(capture.trim_start_matches('*').to_owned())
        })
        .collect()
}

/// Convert an axum path template into an OpenAPI path template.
///
/// The only difference is the wildcard marker: axum spells a catch-all
/// parameter `{*rest}`, while OpenAPI path templating requires `{rest}`.
pub(crate) fn documented_path(path: &str) -> String {
    path.split('/')
        .map(|segment| {
            match segment
                .strip_prefix("{*")
                .and_then(|segment| segment.strip_suffix('}'))
            {
                Some(name) => format!("{{{name}}}"),
                None => segment.to_owned(),
            }
        })
        .collect::<Vec<_>>()
        .join("/")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_axum_path_captures() {
        assert_eq!(path_captures("/users/{id}"), ["id"]);
        assert_eq!(
            path_captures("/orgs/{org_id}/users/{user_id}"),
            ["org_id", "user_id"]
        );
        assert_eq!(path_captures("/files/{*rest}"), ["rest"]);
        assert!(path_captures("/users").is_empty());
    }

    #[test]
    fn rewrites_wildcards_for_openapi() {
        assert_eq!(documented_path("/files/{*rest}"), "/files/{rest}");
        assert_eq!(documented_path("/users/{id}"), "/users/{id}");
        assert_eq!(documented_path("/users"), "/users");
    }
}
