# ix-schema-toml

The TOML driver for [`ix-schema`](https://crates.io/crates/ix-schema): the same
version-tagging and type-safe migrate/upgrade-on-decode as
[`ix-schema-serde`](https://crates.io/crates/ix-schema-serde) and
[`ix-schema-postcard`](https://crates.io/crates/ix-schema-postcard), but over
[`toml`](https://crates.io/crates/toml)'s config-shaped, table-oriented format.

Three structurally different data models — self-describing JSON, compact binary,
and config-style TOML — driven by the one manifest is the orchestrator claim made
concrete: only the codec changes; the version and migration logic is shared.

See the [`ix-schema`](https://crates.io/crates/ix-schema) crate for the full
picture.

Licensed under MIT OR Apache-2.0.
