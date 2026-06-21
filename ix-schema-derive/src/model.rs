//! The compile-time intermediate representation of a `#[derive(Ix)]` type.
//!
//! This IR exists only while the macro runs. It owns `syn` types and their
//! spans so diagnostics can point at the exact offending line, and it is the
//! single structure every later phase (validation, codegen, migration) reads.

use proc_macro_error2::{ResultExt as _, abort};
use proc_macro2::{Span, TokenStream};

use crate::{abort_unsupported, parse_repr, reject_duplicate};

/// A field of the struct, with the ix metadata attached to it.
pub struct FieldModel {
    /// Field accessor: a named field (`Member::Named`) or a tuple position
    /// (`Member::Unnamed`). Works for both `offset_of!` and `stringify!`.
    pub member: syn::Member,
    /// The field's doc comment (joined `///` lines), or `""`.
    pub doc: String,
    /// Declared field type.
    pub ty: syn::Type,
    /// Schema version the field was introduced in.
    pub since: u32,
    /// Initialiser for a field absent from the previous version.
    pub default: Option<syn::Expr>,
    /// Conversion function applied to the predecessor's field.
    pub with: Option<syn::Path>,
    /// The field's name in the previous version, if renamed.
    pub rename_from: Option<String>,
    /// Span of the field, for diagnostics.
    pub span: Span,
}

/// The shape of an enum variant's payload.
pub enum VariantShape {
    /// `Variant` — no payload.
    Unit,
    /// `Variant(A, B)` — positional payload.
    Tuple,
    /// `Variant { a: A }` — named payload.
    Struct,
}

/// A single payload field of a data-carrying variant.
pub struct VariantFieldModel {
    /// Field name for a struct-like variant; `None` for a tuple position.
    pub name: Option<syn::Ident>,
    /// Declared field type.
    pub ty: syn::Type,
}

/// A single enum variant in ix's IR.
pub struct VariantModel {
    /// Variant identifier.
    pub ident: syn::Ident,
    /// The variant's doc comment (joined `///` lines), or `""`.
    pub doc: String,
    /// Whether the variant is unit, tuple, or struct shaped.
    pub kind: VariantShape,
    /// Payload fields (empty for a unit variant).
    pub fields: Vec<VariantFieldModel>,
}

/// The whole struct, parsed and validated into ix's IR.
pub struct StructModel {
    /// Struct identifier.
    pub ident: syn::Ident,
    /// The type's doc comment (joined `///` lines), or `""`.
    pub doc: String,
    /// Generics, forwarded verbatim to the generated impl.
    pub generics: syn::Generics,
    /// Schema version (`1` unless overridden).
    pub version: u32,
    /// The predecessor *type* this version migrates from, if any. The macro
    /// reads its version from its own `MANIFEST`, so no number is duplicated.
    pub migrate_from: Option<syn::Type>,
    /// The `::ix_schema::Repr` expression derived from `#[repr(..)]`.
    pub repr: TokenStream,
    /// Fields in declaration order (named-field struct; empty for an enum).
    pub fields: Vec<FieldModel>,
    /// Variants in declaration order (enum; empty for a struct).
    pub variants: Vec<VariantModel>,
    /// Names of fields present in the predecessor but dropped in this version.
    pub removed: Vec<String>,
}

impl StructModel {
    /// Parse and validate a `DeriveInput` into the IR, aborting with a precise
    /// diagnostic on anything ix cannot represent.
    pub fn analyze(input: &syn::DeriveInput) -> Self {
        match &input.data {
            syn::Data::Struct(data) => Self::analyze_struct(input, &data.fields),
            syn::Data::Enum(data) => Self::analyze_enum(input, data),
            syn::Data::Union(_) => abort_unsupported(input),
        }
    }

    /// Analyze a struct — named, tuple, newtype, or unit — into the IR. Tuple
    /// positions become `Member::Unnamed`, so `offset_of!`/`stringify!` work the
    /// same as for named fields.
    fn analyze_struct(input: &syn::DeriveInput, struct_fields: &syn::Fields) -> Self {
        let (version, migrate_from, removed) = parse_struct_attrs(&input.attrs);
        let repr = parse_repr(&input.attrs);

        let fields: Vec<FieldModel> = struct_fields
            .iter()
            .enumerate()
            .map(|(i, field)| FieldModel::analyze(field, i, version))
            .collect();

        if !removed.is_empty() && migrate_from.is_none() {
            abort!(
                input.ident,
                "`removed(..)` records fields dropped from a predecessor, but no \
                 `migrate_from` is declared";
                help = "add `#[ix(version = .., migrate_from = PrevType)]`"
            );
        }
        for name in &removed {
            if fields.iter().any(|f| f.name_string() == *name) {
                abort!(
                    input.ident,
                    "`removed(\"{}\")` names a field that is still present",
                    name;
                    help = "delete the field, or drop it from `removed(..)`"
                );
            }
        }

        StructModel {
            ident: input.ident.clone(),
            doc: doc_of(&input.attrs),
            generics: input.generics.clone(),
            version,
            migrate_from,
            repr,
            fields,
            variants: Vec::new(),
            removed,
        }
    }

    /// Analyze an enum into the IR. Both fieldless and data-carrying variants are
    /// modelled; only schema migration on enums is rejected (not yet supported).
    fn analyze_enum(input: &syn::DeriveInput, data: &syn::DataEnum) -> Self {
        let (version, migrate_from, removed) = parse_struct_attrs(&input.attrs);
        if migrate_from.is_some() || !removed.is_empty() {
            abort!(
                input.ident,
                "ix-schema: schema migration is not yet supported on enums";
                help = "derive `Ix` on an enum without `migrate_from`/`removed`"
            );
        }
        let repr = parse_repr(&input.attrs);

        let variants: Vec<VariantModel> = data
            .variants
            .iter()
            .map(|variant| {
                let (kind, fields) = match &variant.fields {
                    syn::Fields::Unit => (VariantShape::Unit, Vec::new()),
                    syn::Fields::Unnamed(unnamed) => (
                        VariantShape::Tuple,
                        unnamed
                            .unnamed
                            .iter()
                            .map(|f| VariantFieldModel {
                                name: None,
                                ty: f.ty.clone(),
                            })
                            .collect(),
                    ),
                    syn::Fields::Named(named) => (
                        VariantShape::Struct,
                        named
                            .named
                            .iter()
                            .map(|f| VariantFieldModel {
                                name: f.ident.clone(),
                                ty: f.ty.clone(),
                            })
                            .collect(),
                    ),
                };
                VariantModel {
                    ident: variant.ident.clone(),
                    doc: doc_of(&variant.attrs),
                    kind,
                    fields,
                }
            })
            .collect();

        StructModel {
            ident: input.ident.clone(),
            doc: doc_of(&input.attrs),
            generics: input.generics.clone(),
            version,
            migrate_from: None,
            repr,
            fields: Vec::new(),
            variants,
            removed: Vec::new(),
        }
    }
}

/// Join a type/field/variant's `///` doc comments (`#[doc = "..."]`) into one
/// string, one line per `///`, trimmed. Returns `""` when undocumented.
fn doc_of(attrs: &[syn::Attribute]) -> String {
    let mut lines = Vec::new();
    for attr in attrs {
        if !attr.path().is_ident("doc") {
            continue;
        }
        if let syn::Meta::NameValue(nv) = &attr.meta
            && let syn::Expr::Lit(syn::ExprLit {
                lit: syn::Lit::Str(s),
                ..
            }) = &nv.value
        {
            lines.push(s.value().trim().to_string());
        }
    }
    lines.join("\n")
}

impl FieldModel {
    /// The field's name as a string: the identifier, or the tuple index.
    pub fn name_string(&self) -> String {
        match &self.member {
            syn::Member::Named(id) => id.to_string(),
            syn::Member::Unnamed(idx) => idx.index.to_string(),
        }
    }

    fn analyze(field: &syn::Field, index: usize, struct_version: u32) -> Self {
        let member = match &field.ident {
            Some(ident) => syn::Member::Named(ident.clone()),
            None => syn::Member::Unnamed(syn::Index {
                index: index as u32,
                span: Span::call_site(),
            }),
        };
        let span = field
            .ident
            .as_ref()
            .map_or_else(Span::call_site, syn::Ident::span);

        let mut since: Option<u32> = None;
        let mut default: Option<syn::Expr> = None;
        let mut with: Option<syn::Path> = None;
        let mut rename_from: Option<String> = None;

        for attr in &field.attrs {
            if !attr.path().is_ident("ix") {
                continue;
            }
            attr.parse_nested_meta(|meta| {
                if meta.path.is_ident("since") {
                    reject_duplicate(since.is_some(), &meta)?;
                    let lit: syn::LitInt = meta.value()?.parse()?;
                    since = Some(lit.base10_parse()?);
                } else if meta.path.is_ident("default") {
                    reject_duplicate(default.is_some(), &meta)?;
                    default = Some(meta.value()?.parse()?);
                } else if meta.path.is_ident("with") {
                    reject_duplicate(with.is_some(), &meta)?;
                    with = Some(meta.value()?.parse()?);
                } else if meta.path.is_ident("rename_from") {
                    reject_duplicate(rename_from.is_some(), &meta)?;
                    let lit: syn::LitStr = meta.value()?.parse()?;
                    rename_from = Some(lit.value());
                } else {
                    return Err(meta.error(
                        "unknown field attribute; expected `since`, `default`, `with` or `rename_from`",
                    ));
                }
                Ok(())
            })
            .unwrap_or_abort();
        }

        let since = since.unwrap_or(1);
        if since > struct_version {
            abort!(
                span,
                "field declares `since = {}` but the struct is only version {}",
                since,
                struct_version;
                help = "raise the struct's `#[ix(version = ..)]` or lower `since`"
            );
        }

        FieldModel {
            member,
            doc: doc_of(&field.attrs),
            ty: field.ty.clone(),
            since,
            default,
            with,
            rename_from,
            span,
        }
    }
}

/// Parse the struct-level `#[ix(version = .., migrate_from = .., removed(..))]`.
fn parse_struct_attrs(attrs: &[syn::Attribute]) -> (u32, Option<syn::Type>, Vec<String>) {
    let mut version: Option<u32> = None;
    let mut migrate_from: Option<syn::Type> = None;
    let mut removed: Vec<String> = Vec::new();

    for attr in attrs {
        if !attr.path().is_ident("ix") {
            continue;
        }
        attr.parse_nested_meta(|meta| {
            if meta.path.is_ident("version") {
                reject_duplicate(version.is_some(), &meta)?;
                let lit: syn::LitInt = meta.value()?.parse()?;
                version = Some(lit.base10_parse()?);
            } else if meta.path.is_ident("migrate_from") {
                reject_duplicate(migrate_from.is_some(), &meta)?;
                migrate_from = Some(meta.value()?.parse()?);
            } else if meta.path.is_ident("removed") {
                let content;
                syn::parenthesized!(content in meta.input);
                let names = content
                    .parse_terminated(<syn::LitStr as syn::parse::Parse>::parse, syn::Token![,])?;
                for name in names {
                    removed.push(name.value());
                }
            } else {
                return Err(meta.error(
                    "unknown struct attribute; expected `version`, `migrate_from` or `removed`",
                ));
            }
            Ok(())
        })
        .unwrap_or_abort();
    }

    (version.unwrap_or(1), migrate_from, removed)
}
