# ix-schema-zerocopy

The zerocopy driver for
[`ix-schema`](https://crates.io/crates/ix-schema): manifest-validated, zero-cost
byte views over `Ix` types.

It cross-checks two independent guarantees at compile time — `zerocopy` proves the
type is sound, and the ix-schema manifest proves the layout is gap-free — so a
byte view can never silently expose padding. Every decision is `const`; nothing
here costs anything at runtime. `#![forbid(unsafe_code)]`.

See the [`ix-schema`](https://crates.io/crates/ix-schema) crate for the full
picture.

Licensed under MIT OR Apache-2.0.
