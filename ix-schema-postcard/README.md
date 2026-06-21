# ix-schema-postcard

The postcard (compact binary) driver for
[`ix-schema`](https://crates.io/crates/ix-schema): the same version-tagging and
type-safe migrate/upgrade-on-decode as
[`ix-schema-serde`](https://crates.io/crates/ix-schema-serde), but over
[`postcard`](https://crates.io/crates/postcard)'s binary format instead of JSON.

Two structurally different codecs driven by the one manifest is the orchestrator
claim made concrete: the version and migration logic is format-agnostic; only the
encode/decode calls differ.

See the [`ix-schema`](https://crates.io/crates/ix-schema) crate for the full
picture.

Licensed under MIT OR Apache-2.0.
