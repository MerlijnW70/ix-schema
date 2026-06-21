# ix-schema-serde

The serde (JSON) driver for
[`ix-schema`](https://crates.io/crates/ix-schema): version-tagged encoding and
type-safe migrate/upgrade-on-decode over self-describing JSON.

A value is wrapped in an envelope carrying its schema version straight from the
type's manifest; readers verify that version and can lift older payloads to the
current schema through ix-schema's compile-time migration edges.

See the [`ix-schema`](https://crates.io/crates/ix-schema) crate for the full
picture. A companion crate,
[`ix-schema-postcard`](https://crates.io/crates/ix-schema-postcard), provides the
same over a compact binary format.

Licensed under MIT OR Apache-2.0.
