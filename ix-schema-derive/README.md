# ix-schema-derive

The `#[derive(Ix)]` procedural macro for
[`ix-schema`](https://crates.io/crates/ix-schema).

It lowers a struct or fieldless/data-carrying enum into a compile-time
`const MANIFEST` — layout, fields, variants, and schema evolution — that
`ix-schema` drivers read. Layout figures are emitted as `core::mem` calls, so the
compiler computes them and the manifest cannot drift from the real layout.

You normally depend on [`ix-schema`](https://crates.io/crates/ix-schema), which
re-exports this derive — not on this crate directly. See that crate for full
documentation.

Licensed under MIT OR Apache-2.0.
