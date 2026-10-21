//! JSON Schema generation and the OpenAPI component registry.
//!
//! `schemars` produces a self contained JSON Schema document with local
//! `$defs`. OpenAPI expects reusable models under `components.schemas`, so this
//! module hoists `$defs` into that namespace and rewrites every internal
//! reference. Generation is deferred until [`crate::Router::finish`] so that
//! routers can be nested and merged without having to rewrite already rendered
//! schemas.

use std::collections::BTreeMap;

use schemars::{generate::SchemaSettings, JsonSchema};
use serde_json::{json, Value};

/// A type-erased schema producer.
///
/// Operations only need to remember *how* to generate a schema; the actual
/// generation happens once, when the document is finished. This keeps routing
/// assembly cheap and lets all operations share a single component registry.
#[derive(Clone, Copy)]
pub(crate) struct SchemaOf {
    raw: fn() -> Value,
    name: fn() -> String,
}

impl SchemaOf {
    pub(crate) fn of<T: JsonSchema + 'static>() -> Self {
        Self {
            raw: raw_schema::<T>,
            name: type_name::<T>,
        }
    }

    pub(crate) fn raw(self) -> Value {
        (self.raw)()
    }

    pub(crate) fn name(self) -> String {
        (self.name)()
    }
}

fn raw_schema<T: JsonSchema>() -> Value {
    let settings = SchemaSettings::draft2020_12().with(|settings| {
        // `$schema` is an annotation that does not belong inside an OpenAPI
        // component; OpenAPI 3.1 already implies JSON Schema 2020-12.
        settings.meta_schema = None;
    });
    let schema = settings.into_generator().into_root_schema_for::<T>();
    serde_json::to_value(schema).expect("a JSON Schema must serialize to JSON")
}

fn type_name<T: JsonSchema>() -> String {
    T::schema_name().into_owned()
}

/// Hoist schemars `$defs` into `#/components/schemas` and deduplicate models.
#[derive(Default)]
pub(crate) struct SchemaRegistry {
    components: BTreeMap<String, Component>,
    /// Names reserved during a registration pass, mapped to the canonical
    /// fingerprint that justified the reservation.
    claimed: BTreeMap<String, u64>,
}

struct Component {
    schema: Value,
}

impl SchemaRegistry {
    /// Generate a schema for a handler body/response type.
    ///
    /// Object-like and polymorphic roots become named components and are
    /// returned as a `$ref`. Primitive and array roots stay inline but any
    /// nested models still become components.
    pub(crate) fn reference(&mut self, schema: SchemaOf) -> Value {
        self.register(schema.raw(), sanitize_name(&schema.name()))
    }

    /// Whether no components were registered.
    #[cfg(test)]
    pub(crate) fn is_empty(&self) -> bool {
        self.components.is_empty()
    }

    pub(crate) fn into_components(self) -> BTreeMap<String, Value> {
        self.components
            .into_iter()
            .map(|(name, component)| (name, component.schema))
            .collect()
    }

    fn register(&mut self, mut root: Value, root_name: String) -> Value {
        let defs = take_defs(&mut root);
        let needs_root_component = defs.values().any(contains_root_ref)
            || contains_root_ref(&root)
            || !should_inline(&root);

        if !needs_root_component {
            let def_names = self.hoist_defs(defs, &root_name);
            normalize_refs(&mut root, &def_names, &root_name);
            strip_top_level_metadata(&mut root);
            return root;
        }

        let root_fingerprint = fingerprint(&root, &root_name);
        let root_component = self.claim(root_name, root_fingerprint);
        let def_names = self.hoist_defs(defs, &root_component);
        normalize_refs(&mut root, &def_names, &root_component);
        strip_top_level_metadata(&mut root);
        self.store(&root_component, root);

        json!({ "$ref": format!("#/components/schemas/{root_component}") })
    }

    /// Register every `$defs` entry and return a map from the original local
    /// name to the final component name.
    fn hoist_defs(
        &mut self,
        defs: BTreeMap<String, Value>,
        root_name: &str,
    ) -> BTreeMap<String, String> {
        let mut names = BTreeMap::new();

        // Reserve names first. Fingerprints are computed from the unmodified
        // local schema so identical definitions generated from different roots
        // (or at the root directly) map to the same component.
        for (local_name, def) in &defs {
            let fingerprint = fingerprint(def, root_name);
            let component = self.claim(sanitize_name(local_name), fingerprint);
            names.insert(local_name.clone(), component);
        }

        for (local_name, mut def) in defs {
            let component = names[&local_name].clone();
            normalize_refs(&mut def, &names, root_name);
            strip_top_level_metadata(&mut def);
            self.store(&component, def);
        }

        names
    }

    fn claim(&mut self, base: String, fingerprint: u64) -> String {
        let base = sanitize_name(&base);
        let mut candidate = base.clone();
        let mut suffix = 1u32;

        loop {
            match self.claimed.get(&candidate) {
                None => {
                    self.claimed.insert(candidate.clone(), fingerprint);
                    return candidate;
                }
                Some(existing) if *existing == fingerprint => return candidate,
                Some(_) => {
                    suffix += 1;
                    candidate = format!("{base}_{suffix}");
                }
            }
        }
    }

    fn store(&mut self, name: &str, schema: Value) {
        self.components
            .entry(name.to_owned())
            .or_insert(Component { schema });
    }
}

/// Turn a schemars schema into a fully inline schema, suitable for path and
/// query parameters where a `$ref` would be awkward to inspect.
pub(crate) fn inline_defs(mut root: Value) -> Value {
    let defs = take_defs(&mut root);
    strip_schema_metadata(&mut root);
    let mut stack = Vec::new();
    inline_refs(&mut root, &defs, &mut stack);
    root
}

/// Remove annotations that are redundant or misleading once a schema is
/// inlined into a parameter.
fn strip_schema_metadata(value: &mut Value) {
    if let Value::Object(map) = value {
        map.remove("$schema");
        map.remove("$id");
        map.remove("title");
    }
}

fn inline_refs(value: &mut Value, defs: &BTreeMap<String, Value>, stack: &mut Vec<String>) {
    match value {
        Value::Object(map) => {
            if let Some(reference) = map.get("$ref").and_then(Value::as_str).map(str::to_owned) {
                if reference == "#" {
                    panic!(
                        "recursive schemas are not supported for path/query parameters; \
                         extract the identifier as a scalar instead"
                    );
                }
                if let Some(local_name) = reference.strip_prefix("#/$defs/") {
                    if stack.iter().any(|name| name == local_name) {
                        panic!(
                            "recursive schema `{local_name}` is not supported for path/query parameters"
                        );
                    }
                    let mut resolved = defs
                        .get(local_name)
                        .unwrap_or_else(|| {
                            panic!("unresolved schema reference `{reference}` while inlining")
                        })
                        .clone();
                    stack.push(local_name.to_owned());
                    strip_schema_metadata(&mut resolved);
                    inline_refs(&mut resolved, defs, stack);
                    stack.pop();
                    *value = resolved;
                    return;
                }
            }

            for nested in map.values_mut() {
                inline_refs(nested, defs, stack);
            }
        }
        Value::Array(items) => {
            for nested in items {
                inline_refs(nested, defs, stack);
            }
        }
        _ => {}
    }
}

fn take_defs(root: &mut Value) -> BTreeMap<String, Value> {
    let Some(object) = root.as_object_mut() else {
        return BTreeMap::new();
    };
    let Some(Value::Object(defs)) = object.remove("$defs") else {
        return BTreeMap::new();
    };
    defs.into_iter().collect()
}

/// A schema with only primitive/array structure does not deserve a component.
fn should_inline(root: &Value) -> bool {
    let Some(object) = root.as_object() else {
        return true;
    };

    if object.contains_key("oneOf")
        || object.contains_key("anyOf")
        || object.contains_key("allOf")
        || object.contains_key("$ref")
    {
        return false;
    }

    match object.get("type") {
        Some(Value::String(kind)) => match kind.as_str() {
            "object" => {
                !object.contains_key("properties")
                    && !object.contains_key("additionalProperties")
                    && !object.contains_key("patternProperties")
            }
            "array" => true,
            _ => true,
        },
        Some(Value::Array(_)) => true,
        _ => true,
    }
}

fn contains_root_ref(value: &Value) -> bool {
    match value {
        Value::Object(map) => {
            if map.get("$ref").and_then(Value::as_str) == Some("#") {
                return true;
            }
            map.values().any(contains_root_ref)
        }
        Value::Array(items) => items.iter().any(contains_root_ref),
        _ => false,
    }
}

fn normalize_refs(value: &mut Value, defs: &BTreeMap<String, String>, root_name: &str) {
    match value {
        Value::Object(map) => {
            if let Some(Value::String(reference)) = map.get_mut("$ref") {
                if reference == "#" {
                    *reference = format!("#/components/schemas/{root_name}");
                } else if let Some(local_name) = reference.strip_prefix("#/$defs/") {
                    if let Some(component) = defs.get(local_name) {
                        *reference = format!("#/components/schemas/{component}");
                    } else {
                        panic!(
                            "unresolved `$defs/{local_name}` reference while building OpenAPI components"
                        );
                    }
                }
            }

            map.remove("$defs");
            map.remove("$schema");
            map.remove("$id");

            for nested in map.values_mut() {
                normalize_refs(nested, defs, root_name);
            }
        }
        Value::Array(items) => {
            for nested in items {
                normalize_refs(nested, defs, root_name);
            }
        }
        _ => {}
    }
}

fn strip_top_level_metadata(value: &mut Value) {
    if let Value::Object(map) = value {
        map.remove("$schema");
        map.remove("$id");
        map.remove("title");
        map.remove("$defs");
    }
}

/// Fingerprint used for deduplication.
///
/// Refs are canonicalized relative to local `$defs` names so the same model
/// looks identical whether schemars emitted it as a root schema or as a
/// definition of another schema.
fn fingerprint(value: &Value, root_name: &str) -> u64 {
    let mut canonical = value.clone();
    canonicalize_for_fingerprint(&mut canonical, root_name);
    let serialized = serde_json::to_string(&canonical).expect("schemas serialize to JSON");
    fnv1a(serialized.as_bytes())
}

fn canonicalize_for_fingerprint(value: &mut Value, root_name: &str) {
    match value {
        Value::Object(map) => {
            map.remove("$schema");
            map.remove("$id");
            map.remove("title");
            map.remove("$defs");

            if let Some(Value::String(reference)) = map.get_mut("$ref") {
                if reference == "#" {
                    *reference = format!("#/$defs/{root_name}");
                }
            }

            for nested in map.values_mut() {
                canonicalize_for_fingerprint(nested, root_name);
            }
        }
        Value::Array(items) => {
            for nested in items {
                canonicalize_for_fingerprint(nested, root_name);
            }
        }
        _ => {}
    }
}

fn sanitize_name(name: &str) -> String {
    let mut sanitized = String::with_capacity(name.len());
    for character in name.chars() {
        if character.is_ascii_alphanumeric() || matches!(character, '.' | '_' | '-') {
            sanitized.push(character);
        } else {
            sanitized.push('_');
        }
    }
    if sanitized.is_empty() {
        "Schema".to_owned()
    } else {
        sanitized
    }
}

fn fnv1a(bytes: &[u8]) -> u64 {
    let mut hash = 0xcbf2_9ce4_8422_2325u64;
    for byte in bytes {
        hash ^= u64::from(*byte);
        hash = hash.wrapping_mul(0x0000_0100_0000_01b3);
    }
    hash
}

#[cfg(test)]
mod tests {
    use super::*;
    use schemars::JsonSchema;
    use serde::Serialize;
    use std::collections::HashMap;

    #[derive(Serialize, JsonSchema)]
    struct User {
        id: u64,
    }

    #[derive(Serialize, JsonSchema)]
    struct Nested {
        user: User,
    }

    #[derive(Serialize, JsonSchema)]
    struct Generic<T> {
        value: T,
    }

    #[derive(Serialize, JsonSchema)]
    struct Other {
        name: String,
    }

    #[test]
    fn registers_objects_and_rewrites_refs() {
        let mut registry = SchemaRegistry::default();
        assert_eq!(
            registry.reference(SchemaOf::of::<User>()),
            json!({ "$ref": "#/components/schemas/User" })
        );

        let components = registry.into_components();
        assert!(components.contains_key("User"));
        assert_eq!(components["User"]["type"], "object");
        assert!(components["User"].get("title").is_none());
    }

    #[test]
    fn hoists_nested_definitions() {
        let mut registry = SchemaRegistry::default();
        registry.reference(SchemaOf::of::<Nested>());
        let components = registry.into_components();

        assert!(components.contains_key("Nested"));
        assert!(components.contains_key("User"));
        assert_eq!(
            components["Nested"]["properties"]["user"]["$ref"],
            "#/components/schemas/User"
        );
    }

    #[test]
    fn arrays_stay_inline_but_items_are_components() {
        let mut registry = SchemaRegistry::default();
        let schema = registry.reference(SchemaOf::of::<Vec<User>>());

        assert_eq!(schema["type"], "array");
        assert_eq!(schema["items"]["$ref"], "#/components/schemas/User");
        let components = registry.into_components();
        assert!(components.contains_key("User"));
        assert!(!components.contains_key("Array_of_User"));
    }

    #[test]
    fn primitive_schemas_stay_inline() {
        let mut registry = SchemaRegistry::default();
        let schema = registry.reference(SchemaOf::of::<u64>());
        assert_eq!(schema["type"], "integer");
        assert!(registry.is_empty());
    }

    #[test]
    fn maps_become_components() {
        let mut registry = SchemaRegistry::default();
        let schema = registry.reference(SchemaOf::of::<HashMap<String, User>>());
        assert!(schema["$ref"].is_string());
        let components = registry.into_components();
        assert!(components.contains_key("Map_of_User"));
    }

    #[test]
    fn identical_models_are_deduplicated() {
        let mut registry = SchemaRegistry::default();
        let first = registry.reference(SchemaOf::of::<Nested>());
        let second = registry.reference(SchemaOf::of::<Nested>());
        assert_eq!(first, second);

        let components = registry.into_components();
        assert_eq!(components.len(), 2, "Nested and User");
    }

    #[test]
    fn conflicting_generic_instantiations_get_distinct_names() {
        let mut registry = SchemaRegistry::default();
        let first = registry.reference(SchemaOf::of::<Generic<User>>());
        let second = registry.reference(SchemaOf::of::<Generic<Other>>());
        assert_ne!(first, second);

        let components = registry.into_components();
        assert_eq!(
            components
                .keys()
                .filter(|name| name.starts_with("Generic"))
                .count(),
            2
        );
    }

    #[test]
    fn inline_defs_resolves_nested_refs() {
        let schema = inline_defs(raw_schema::<Nested>());
        assert_eq!(schema["properties"]["user"]["type"], "object");
        assert!(schema.get("$defs").is_none());
    }
}
