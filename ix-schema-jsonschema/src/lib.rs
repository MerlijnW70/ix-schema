//! # ix-schema-jsonschema — a (shallow) JSON Schema from the manifest
//!
//! Turns an [`ix_schema::Ix`] type's compile-time manifest into a JSON Schema
//! (Draft 2020-12) document whose shape matches what the **serde driver actually
//! emits**: named structs → objects, tuple structs → arrays, newtype structs →
//! their inner value, unit structs → `null`, fieldless enums → string enums, and
//! data-carrying enums → an externally-tagged `oneOf`.
//!
//! It is deliberately **shallow**. Only primitive scalar field types are mapped
//! precisely (integers carry their `minimum`/`maximum`, `char` its length);
//! `Option<T>` is made nullable and dropped from `required`. Nested `Ix` types,
//! containers (`Vec`, arrays, maps), and any other non-primitive field fall back
//! to a permissive empty schema `{}` — *not* a misleading `"object"` — because
//! the manifest exposes only each field's spelled type *name*, not a nested
//! manifest to recurse into. Where the serde and postcard drivers prove the
//! manifest drives *encoding*, this proves it drives *downstream codegen*.
//!
//! ```
//! use ix_schema::Ix;
//! #[derive(Ix)]
//! #[repr(C)]
//! struct Point {
//!     x: u32,
//!     y: u32,
//! }
//! let schema = ix_schema_jsonschema::to_json_schema::<Point>();
//! assert_eq!(schema["type"], "object");
//! assert_eq!(schema["properties"]["x"]["type"], "integer");
//! ```
#![warn(missing_docs)]
#![deny(rustdoc::broken_intra_doc_links, rustdoc::private_intra_doc_links)]

use ix_schema::{Ix, VariantKind};
use serde_json::{Map, Value, json};

/// The JSON Schema dialect the emitted documents declare.
const DRAFT: &str = "https://json-schema.org/draft/2020-12/schema";

/// JSON Schema for a single spelled Rust type name.
///
/// Primitive scalars map precisely — integers carry their range, `char` its
/// length. `Option<T>` becomes nullable (`anyOf [inner, null]`). Everything else
/// — nested `Ix` types, `Vec`/arrays/maps, anything the manifest names but cannot
/// describe — returns a permissive empty schema `{}`, since the manifest gives
/// only the type's spelled name, not a structure to recurse into.
#[must_use]
pub fn schema_for_type_name(rust_type: &str) -> Value {
    let t = rust_type.trim();
    if let Some(inner) = t.strip_prefix("Option<").and_then(|s| s.strip_suffix('>')) {
        // serde: None -> null, Some(x) -> x
        return json!({ "anyOf": [schema_for_type_name(inner), { "type": "null" }] });
    }
    match t {
        "u8" => json!({ "type": "integer", "minimum": 0, "maximum": 255 }),
        "u16" => json!({ "type": "integer", "minimum": 0, "maximum": 65_535 }),
        "u32" => json!({ "type": "integer", "minimum": 0, "maximum": 4_294_967_295u64 }),
        "u64" | "u128" | "usize" => json!({ "type": "integer", "minimum": 0 }),
        "i8" => json!({ "type": "integer", "minimum": -128, "maximum": 127 }),
        "i16" => json!({ "type": "integer", "minimum": -32_768, "maximum": 32_767 }),
        "i32" | "i64" | "i128" | "isize" => json!({ "type": "integer" }),
        "f32" | "f64" => json!({ "type": "number" }),
        "bool" => json!({ "type": "boolean" }),
        "String" | "&str" | "str" => json!({ "type": "string" }),
        "char" => json!({ "type": "string", "minLength": 1, "maxLength": 1 }),
        // Unknown / complex: stay permissive rather than claim a shape we cannot
        // verify from the manifest.
        _ => json!({}),
    }
}

/// Whether a spelled type is `Option<…>` (optional, hence not `required`).
fn is_optional(type_name: &str) -> bool {
    type_name.trim().starts_with("Option<")
}

/// Attach a `description` to an object schema (no-op for an empty doc).
fn with_doc(mut schema: Value, doc: &str) -> Value {
    if !doc.is_empty()
        && let Value::Object(map) = &mut schema
    {
        map.insert("description".into(), json!(doc));
    }
    schema
}

/// Generate a JSON Schema (Draft 2020-12) [`Value`] for the [`Ix`] type `T`.
///
/// The document's shape mirrors the serde driver's encoding of `T`. See the
/// crate docs for the per-shape mapping and the shallow-fallback caveat.
#[must_use]
pub fn to_json_schema<T: Ix>() -> Value {
    let m = T::MANIFEST;
    let mut body = if m.variants.is_empty() {
        struct_schema(m.fields)
    } else {
        enum_schema(m.variants)
    };
    if let Value::Object(map) = &mut body {
        map.insert("$schema".into(), json!(DRAFT));
        map.insert("title".into(), json!(m.type_name));
        if !m.doc.is_empty() {
            map.insert("description".into(), json!(m.doc));
        }
    }
    body
}

/// Schema for a struct, matching serde's encoding: unit → `null`, newtype →
/// inner value, multi-field tuple → array, named → object.
fn struct_schema(fields: &[ix_schema::FieldSpec]) -> Value {
    if fields.is_empty() {
        return json!({ "type": "null" }); // unit struct -> serde `null`
    }
    // Tuple/newtype structs reflect with positional names "0","1",…
    let is_tuple = fields
        .iter()
        .enumerate()
        .all(|(i, f)| f.name.parse::<usize>() == Ok(i));
    if is_tuple {
        if fields.len() == 1 {
            // newtype struct -> serde encodes transparently as the inner value
            return with_doc(schema_for_type_name(fields[0].type_name), fields[0].doc);
        }
        let items: Vec<Value> = fields
            .iter()
            .map(|f| with_doc(schema_for_type_name(f.type_name), f.doc))
            .collect();
        let n = fields.len();
        return json!({ "type": "array", "prefixItems": items, "minItems": n, "maxItems": n });
    }
    let mut props = Map::new();
    let mut required = Vec::new();
    for f in fields {
        props.insert(
            f.name.to_string(),
            with_doc(schema_for_type_name(f.type_name), f.doc),
        );
        if !is_optional(f.type_name) {
            required.push(json!(f.name));
        }
    }
    json!({ "type": "object", "properties": Value::Object(props), "required": required })
}

/// Schema for an enum: a fieldless enum is a string of variant names; a
/// data-carrying enum is an externally-tagged `oneOf` (serde's default).
fn enum_schema(variants: &[ix_schema::VariantSpec]) -> Value {
    if variants.iter().all(|v| matches!(v.kind, VariantKind::Unit)) {
        let names: Vec<Value> = variants.iter().map(|v| json!(v.name)).collect();
        return json!({ "type": "string", "enum": names });
    }
    let cases: Vec<Value> = variants.iter().map(variant_case).collect();
    json!({ "oneOf": cases })
}

/// One `oneOf` branch for a variant, externally tagged like serde: a unit
/// variant is the bare name (`const`); a data variant is `{ "Name": payload }`.
fn variant_case(v: &ix_schema::VariantSpec) -> Value {
    let case = match v.kind {
        VariantKind::Unit => json!({ "const": v.name }),
        VariantKind::Tuple => {
            let payload = if v.fields.len() == 1 {
                schema_for_type_name(v.fields[0].type_name)
            } else {
                let items: Vec<Value> = v
                    .fields
                    .iter()
                    .map(|f| schema_for_type_name(f.type_name))
                    .collect();
                let n = v.fields.len();
                json!({ "type": "array", "prefixItems": items, "minItems": n, "maxItems": n })
            };
            tagged(v.name, payload)
        }
        VariantKind::Struct => {
            let mut props = Map::new();
            let mut required = Vec::new();
            for f in v.fields {
                let key = f.name.unwrap_or("_");
                props.insert(key.to_string(), schema_for_type_name(f.type_name));
                if !is_optional(f.type_name) {
                    required.push(json!(key));
                }
            }
            let inner = json!({ "type": "object", "properties": Value::Object(props), "required": required });
            tagged(v.name, inner)
        }
    };
    with_doc(case, v.doc)
}

/// `{ "type": "object", "required": [tag], "properties": { tag: payload } }`.
fn tagged(tag: &str, payload: Value) -> Value {
    let mut props = Map::new();
    props.insert(tag.to_string(), payload);
    json!({ "type": "object", "required": [tag], "properties": Value::Object(props) })
}

/// Generate the JSON Schema for `T` as a pretty-printed string.
#[must_use]
pub fn to_json_schema_string<T: Ix>() -> String {
    serde_json::to_string_pretty(&to_json_schema::<T>())
        .expect("a Value built from &str/array/object always serializes")
}

#[cfg(test)]
mod tests {
    use super::*;
    use ix_schema::Ix;

    /// A 2-D point.
    #[derive(Ix)]
    #[repr(C)]
    struct Point {
        /// horizontal position
        x: u32,
        y: f64,
        live: bool,
    }

    #[test]
    fn named_struct_becomes_object_with_bounded_typed_properties() {
        let s = to_json_schema::<Point>();
        assert_eq!(s["$schema"], DRAFT);
        assert_eq!(s["description"], "A 2-D point.");
        assert_eq!(s["type"], "object");
        assert_eq!(s["properties"]["x"]["type"], "integer");
        assert_eq!(s["properties"]["x"]["minimum"], 0);
        assert_eq!(s["properties"]["x"]["maximum"], 4_294_967_295u64);
        assert_eq!(s["properties"]["x"]["description"], "horizontal position");
        assert_eq!(s["properties"]["y"]["type"], "number");
        assert_eq!(s["properties"]["live"]["type"], "boolean");
        assert_eq!(s["required"], json!(["x", "y", "live"]));
    }

    #[derive(Ix)]
    struct Unit;

    #[derive(Ix)]
    #[repr(C)]
    struct Inches(u32);

    #[derive(Ix)]
    #[repr(C)]
    struct Pair(u32, u64);

    #[test]
    fn struct_shapes_match_serde_encoding() {
        // unit struct -> null
        assert_eq!(to_json_schema::<Unit>()["type"], "null");
        // newtype struct -> the inner value, transparently
        let n = to_json_schema::<Inches>();
        assert_eq!(n["type"], "integer");
        assert!(n.get("properties").is_none());
        // multi-field tuple struct -> array
        let p = to_json_schema::<Pair>();
        assert_eq!(p["type"], "array");
        assert_eq!(p["minItems"], 2);
        assert_eq!(p["prefixItems"][1]["type"], "integer");
    }

    #[derive(Ix)]
    #[repr(C)]
    struct WithText {
        name: String,
        initial: char,
    }

    #[test]
    fn string_and_char_are_precise() {
        let s = to_json_schema::<WithText>();
        assert_eq!(s["properties"]["name"]["type"], "string");
        assert_eq!(s["properties"]["initial"]["type"], "string");
        assert_eq!(s["properties"]["initial"]["minLength"], 1);
        assert_eq!(s["properties"]["initial"]["maxLength"], 1);
    }

    #[test]
    fn option_field_is_nullable_and_not_required() {
        // The field type is named "Option<u32>" in the manifest.
        assert_eq!(
            schema_for_type_name("Option<u32>"),
            json!({ "anyOf": [{ "type": "integer", "minimum": 0, "maximum": 4_294_967_295u64 }, { "type": "null" }] })
        );
    }

    #[test]
    fn unknown_and_container_types_are_permissive_not_object() {
        // The risky case: an unsupported type must NOT claim a fake shape.
        assert_eq!(schema_for_type_name("Vec<u32>"), json!({}));
        assert_eq!(schema_for_type_name("MyNestedStruct"), json!({}));
        assert_eq!(schema_for_type_name("[u8; 32]"), json!({}));
    }

    #[derive(Ix)]
    #[repr(u8)]
    enum Mode {
        Idle,
        Active,
    }

    #[derive(Ix)]
    enum Message {
        Quit,
        Text(String),
        Move { x: u32, y: u32 },
    }

    #[test]
    fn fieldless_enum_is_string_enum_but_data_enum_is_tagged_oneof() {
        let m = to_json_schema::<Mode>();
        assert_eq!(m["type"], "string");
        assert_eq!(m["enum"], json!(["Idle", "Active"]));
        let _ = (Mode::Idle, Mode::Active);

        // Data-carrying enum: externally-tagged oneOf, NOT a lossy string enum.
        let msg = to_json_schema::<Message>();
        let cases = msg["oneOf"].as_array().expect("oneOf array");
        assert_eq!(cases.len(), 3);
        assert_eq!(cases[0]["const"], "Quit"); // unit variant
        assert_eq!(cases[1]["properties"]["Text"]["type"], "string"); // newtype payload
        assert_eq!(
            cases[2]["properties"]["Move"]["properties"]["x"]["type"],
            "integer"
        );

        // Construct and read each variant so the fixture's payloads are exercised
        // (the derive inspects them via the manifest, never by value).
        let _ = Message::Quit;
        match Message::Text("hi".to_owned()) {
            Message::Text(s) => assert_eq!(s, "hi"),
            _ => unreachable!(),
        }
        match (Message::Move { x: 1, y: 2 }) {
            Message::Move { x, y } => assert_eq!(x + y, 3),
            _ => unreachable!(),
        }
    }

    #[test]
    fn string_output_is_valid_json() {
        let out = to_json_schema_string::<Point>();
        let parsed: Value = serde_json::from_str(&out).expect("valid JSON");
        assert_eq!(parsed["title"], "ix_schema_jsonschema::tests::Point");
    }
}
