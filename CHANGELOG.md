# Changelog

## 0.2.1

### Changed
- Crate `description` metadata now refers to `ix-schema` instead of the
  pre-rename `ix` name. Metadata only — no code or API changes.

## 0.2.0

### Added
- **Doc-comment capture** — `Manifest`, `FieldSpec`, and `VariantSpec` now carry a
  `doc` field; the derive harvests `///` comments for the type, each field, and
  each variant.
- **Tuple, newtype, and unit struct** support in `#[derive(Ix)]` (tuple positions
  reflect as fields `"0"`, `"1"`, … with real `offset_of!` offsets).
- New crate **`ix-schema-toml`** — a TOML (config-shaped) format driver.
- New crate **`ix-schema-jsonschema`** — JSON Schema (Draft 2020-12) generation
  from a type's manifest.
- CI (and the project's checks) now verify the core crate builds `no_std` on a
  bare-metal target.

### Changed (breaking)
- `Manifest`, `FieldSpec`, and `VariantSpec` gained a public `doc` field. Code
  that constructs these structs literally must add it. (Pre-1.0 breaking change,
  hence the `0.1 → 0.2` bump.)

## 0.1.0
- Initial release: core manifest + derive, serde and postcard drivers, zerocopy
  driver, and type-safe schema migration.
