# ix-schema-jsonschema

A JSON Schema (Draft 2020-12) generator for
[`ix-schema`](https://crates.io/crates/ix-schema): turn any `Ix` type's
compile-time manifest into a JSON Schema document — field names, types, and doc
comments included.

Where the serde and postcard drivers prove the manifest drives *encoding*, this
proves it drives *downstream codegen*: one type description, many tools.

```rust
use ix_schema::Ix;

#[derive(Ix)]
#[repr(C)]
struct Point {
    x: u32,
    y: u32,
}

let schema = ix_schema_jsonschema::to_json_schema::<Point>();
assert_eq!(schema["properties"]["x"]["type"], "integer");
```

A struct becomes an `object` with one property per field; a fieldless enum
becomes a `string` with an `enum` of variant names. Doc comments are emitted as
`description`.

See the [`ix-schema`](https://crates.io/crates/ix-schema) crate for the full
picture.

Licensed under MIT OR Apache-2.0.
