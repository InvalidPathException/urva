use proc_macro2::Span;
use serde_derive_internals::{Ctxt, attr};
use syn::ext::IdentExt;
use syn::spanned::Spanned;
use syn::{Data, DeriveInput, Fields, Ident, LitStr, Type, Visibility};

use crate::index_attr::{IndexDecl, KeyDecl, KeyKindDecl};

pub(crate) fn same_ident(a: &Ident, b: &Ident) -> bool {
    a.unraw() == b.unraw()
}

pub struct FieldModel {
    pub ident: Ident,
    pub ty: Type,
    pub stored_name: String,
    pub unstorable: Option<&'static str>,
    pub fully_skipped: bool,
    pub flattened: bool,
    pub custom_writer_path: Option<syn::ExprPath>,
}

impl FieldModel {
    pub fn occupies_stored_name(&self) -> bool {
        !self.fully_skipped && !self.flattened
    }
}

pub struct StructModel {
    pub ident: Ident,
    pub vis: Visibility,
    pub fields: Vec<FieldModel>,
}

pub struct EntityModel {
    pub base: StructModel,
    pub collection: String,
    pub id_ty: Type,
    pub version: Option<String>,
    pub indexes: Vec<IndexDecl>,
}

pub fn parse_struct(input: &DeriveInput, derive_name: &str) -> syn::Result<StructModel> {
    if !input.generics.params.is_empty() {
        let kind = if input
            .generics
            .params
            .iter()
            .all(|p| matches!(p, syn::GenericParam::Lifetime(_)))
        {
            "lifetime parameters"
        } else {
            "generic types"
        };
        return Err(syn::Error::new(
            input.generics.span(),
            format!("`#[derive({derive_name})]` does not support {kind}"),
        ));
    }
    let Data::Struct(data) = &input.data else {
        return Err(syn::Error::new(
            input.ident.span(),
            format!("`#[derive({derive_name})]` supports structs only"),
        ));
    };
    let Fields::Named(named) = &data.fields else {
        return Err(syn::Error::new(
            input.ident.span(),
            format!("`#[derive({derive_name})]` requires named fields"),
        ));
    };

    let cx = Ctxt::new();
    let private = Ident::new("_serde", Span::call_site());
    let container = attr::Container::from_ast(&cx, input);
    let mut serde_fields: Vec<attr::Field> = named
        .named
        .iter()
        .enumerate()
        .map(|(i, field)| attr::Field::from_ast(&cx, i, field, None, container.default(), &private))
        .collect();
    cx.check()?;
    for field in &mut serde_fields {
        field.rename_by_rules(container.rename_all_rules());
    }

    let rules = container.rename_all_rules();
    let split_names = rules.serialize != rules.deserialize;
    let mut errors: Vec<syn::Error> = Vec::new();
    if split_names {
        errors.push(syn::Error::new(
            input.ident.span(),
            "split serialize/deserialize `rename_all` writes fields under one set of names and reads them back under another",
        ));
    }
    errors.extend(delegation_errors(&container));

    let mut fields: Vec<FieldModel> = Vec::new();
    for (field, serde) in named.named.iter().zip(&serde_fields) {
        let ident = field.ident.clone().expect("named field");
        match field_model(ident, &field.ty, serde, derive_name, split_names, &fields) {
            Ok(model) => fields.push(model),
            Err(error) => errors.push(error),
        }
    }
    combine_errors(errors)?;

    Ok(StructModel {
        ident: input.ident.clone(),
        vis: input.vis.clone(),
        fields,
    })
}

fn delegation_errors(container: &attr::Container) -> Vec<syn::Error> {
    let mut errors = Vec::new();
    let delegations = [
        ("from", container.type_from().map(Spanned::span)),
        ("try_from", container.type_try_from().map(Spanned::span)),
        ("into", container.type_into().map(Spanned::span)),
        ("remote", container.remote().map(Spanned::span)),
    ];
    for (attr, span) in delegations {
        if let Some(span) = span {
            errors.push(syn::Error::new(
                span,
                format!(
                    "`#[serde({attr})]` stores this struct through another type, so its fields do not describe the stored document"
                ),
            ));
        }
    }
    errors
}

fn field_model(
    ident: Ident,
    ty: &Type,
    serde: &attr::Field,
    derive_name: &str,
    split_names: bool,
    earlier: &[FieldModel],
) -> syn::Result<FieldModel> {
    let name = serde.name();
    if !split_names && name.serialize_name() != name.deserialize_name() {
        return Err(syn::Error::new(
            ident.span(),
            format!(
                "`{ident}` is stored as `{}` but read back as `{}`",
                name.serialize_name(),
                name.deserialize_name()
            ),
        ));
    }
    let stored_name = name.serialize_name().value.clone();

    let flattened = serde.flatten();
    let fully_skipped = serde.skip_serializing() && serde.skip_deserializing();
    let unstorable = if flattened {
        Some("#[serde(flatten)]")
    } else if fully_skipped {
        Some("#[serde(skip)]")
    } else if serde.skip_serializing() {
        Some("#[serde(skip_serializing)]")
    } else {
        None
    };

    if unstorable.is_none() && stored_name.contains('.') {
        return Err(syn::Error::new(
            ident.span(),
            format!(
                "the stored name `{stored_name}` contains a `.`, which query paths read as nested-field access"
            ),
        ));
    }

    if unstorable.is_none() && derive_name == "Entity" && stored_name == "_id" {
        return Err(syn::Error::new(
            ident.span(),
            "the stored name `_id` is reserved",
        ));
    }

    if !fully_skipped
        && !flattened
        && let Some(first) = earlier
            .iter()
            .find(|f| f.occupies_stored_name() && f.stored_name == stored_name)
    {
        let mut err = syn::Error::new(
            ident.span(),
            format!("two fields store under `{stored_name}`"),
        );
        err.combine(syn::Error::new(
            first.ident.span(),
            format!("the first field stored under `{stored_name}`"),
        ));
        return Err(err);
    }

    Ok(FieldModel {
        ident,
        ty: ty.clone(),
        stored_name,
        unstorable,
        fully_skipped,
        flattened,
        custom_writer_path: serde.serialize_with().cloned(),
    })
}

pub fn parse_entity(input: &DeriveInput) -> syn::Result<EntityModel> {
    let base = parse_struct(input, "Entity")?;
    reject_misplaced_attrs(input)?;

    let mut collection: Option<String> = None;
    let mut id_ty: Option<Type> = None;
    let mut version: Option<String> = None;
    for attr in &input.attrs {
        if !attr.path().is_ident("entity") {
            continue;
        }
        attr.parse_nested_meta(|meta| {
            if meta.path.is_ident("collection") {
                let lit: LitStr = meta.value()?.parse()?;
                collection = Some(lit.value());
                Ok(())
            } else if meta.path.is_ident("id") {
                id_ty = Some(meta.value()?.parse()?);
                Ok(())
            } else if meta.path.is_ident("versioned") {
                let name = if meta.input.peek(syn::Token![=]) {
                    let lit: LitStr = meta.value()?.parse()?;
                    if syn::parse_str::<Ident>(&lit.value()).is_err() {
                        return Err(syn::Error::new(
                            lit.span(),
                            "the version field name must be an identifier",
                        ));
                    }
                    lit.value()
                } else {
                    "version".to_string()
                };
                version = Some(name);
                Ok(())
            } else {
                Err(meta
                    .error("unknown `entity` option, expected `collection`, `id`, or `versioned`"))
            }
        })?;
    }
    let collection = collection.ok_or_else(|| {
        syn::Error::new(
            base.ident.span(),
            "missing `#[entity(collection = \"...\")]`",
        )
    })?;
    let id_ty = id_ty.unwrap_or_else(|| syn::parse_quote!(::urva::bson::oid::ObjectId));

    if let Some(version_name) = &version
        && let Some(field) = base.fields.iter().find(|f| {
            f.occupies_stored_name() && f.unstorable.is_none() && &f.stored_name == version_name
        })
    {
        return Err(syn::Error::new(
            field.ident.span(),
            format!(
                "the stored name `{version_name}` is reserved for the version lock. Rename the field, or the lock with `versioned = \"...\"`"
            ),
        ));
    }

    let mut indexes = Vec::new();
    let mut errors: Vec<syn::Error> = Vec::new();
    for attr in &input.attrs {
        if attr.path().is_ident("index") {
            match attr.parse_args::<IndexDecl>() {
                Ok(decl) => indexes.push(decl),
                Err(error) => errors.push(error),
            }
        }
    }
    combine_errors(errors)?;

    let model = EntityModel {
        base,
        collection,
        id_ty,
        version,
        indexes,
    };
    validate_indexes(&model)?;
    Ok(model)
}

fn reject_misplaced_attrs(input: &DeriveInput) -> syn::Result<()> {
    let Data::Struct(data) = &input.data else {
        return Ok(());
    };
    for field in &data.fields {
        for attr in &field.attrs {
            let message = if attr.path().is_ident("entity") {
                "`#[entity]` applies to the struct, not a field"
            } else if attr.path().is_ident("index") {
                "`#[index]` applies to the struct, not a field"
            } else {
                continue;
            };
            return Err(syn::Error::new(attr.span(), message));
        }
    }
    Ok(())
}

fn combine_errors(errors: Vec<syn::Error>) -> syn::Result<()> {
    let mut errors = errors.into_iter();
    match errors.next() {
        None => Ok(()),
        Some(mut first) => {
            for error in errors {
                first.combine(error);
            }
            Err(first)
        }
    }
}

fn validate_indexes(model: &EntityModel) -> syn::Result<()> {
    let errors = model
        .indexes
        .iter()
        .filter_map(|decl| validate_decl(model, decl).err())
        .collect();
    combine_errors(errors)
}

fn validate_decl(model: &EntityModel, decl: &IndexDecl) -> syn::Result<()> {
    for key in &decl.keys {
        if is_id_key(key) {
            continue;
        }
        storable_field(model, &key.segments[0], " and cannot be indexed")?;
    }

    let path = |key: &KeyDecl| {
        let mut path = key.segments[0].unraw().to_string();
        if key.kind == KeyKindDecl::FieldWildcard {
            path.push_str(".$**");
        }
        path
    };
    for (i, key) in decl.keys.iter().enumerate() {
        if decl.keys[..i].iter().any(|prev| path(prev) == path(key)) {
            return Err(syn::Error::new(
                key.span,
                format!("`{}` appears twice in this index's keys", path(key)),
            ));
        }
    }

    if let [key] = decl.keys.as_slice()
        && is_id_key(key)
        && key.kind == KeyKindDecl::Asc
    {
        return Err(syn::Error::new(
            key.span,
            "a one-key `_id` index restates the server's built-in `_id_` index. Remove it",
        ));
    }
    Ok(())
}

fn storable_field<'m>(
    model: &'m EntityModel,
    ident: &Ident,
    purpose: &str,
) -> syn::Result<&'m FieldModel> {
    let Some(field) = model
        .base
        .fields
        .iter()
        .find(|f| same_ident(&f.ident, ident))
    else {
        return Err(syn::Error::new(
            ident.span(),
            format!("`{ident}` is not a field of `{}`", model.base.ident),
        ));
    };
    if let Some(attr) = field.unstorable {
        return Err(syn::Error::new(
            ident.span(),
            format!("`{ident}` carries {attr}, so it is never stored{purpose}"),
        ));
    }
    Ok(field)
}

fn is_id_key(key: &KeyDecl) -> bool {
    matches!(key.segments.as_slice(), [only] if only.unraw() == "_id")
}

pub fn token_type(ty: &Type) -> Type {
    if let Some(inner) = wrapper_inner_type(ty, "Box") {
        return token_type(&inner);
    }
    for wrapper in ["Option", "Vec"] {
        if let Some(inner) = wrapper_inner_type(ty, wrapper) {
            return rebuild_wrapper(ty, token_type(&inner));
        }
    }
    ty.clone()
}

fn rebuild_wrapper(ty: &Type, inner: Type) -> Type {
    let Type::Path(mut path) = peel_type_groups(ty).clone() else {
        return ty.clone();
    };
    if let Some(last) = path.path.segments.last_mut()
        && let syn::PathArguments::AngleBracketed(args) = &mut last.arguments
        && let Some(arg) = args.args.first_mut()
    {
        *arg = syn::GenericArgument::Type(inner);
    }
    Type::Path(path)
}

fn peel_type_groups(ty: &Type) -> &Type {
    match ty {
        Type::Group(group) => peel_type_groups(&group.elem),
        Type::Paren(paren) => peel_type_groups(&paren.elem),
        other => other,
    }
}

fn wrapper_inner_type(ty: &Type, wrapper: &str) -> Option<Type> {
    let Type::Path(path) = peel_type_groups(ty) else {
        return None;
    };
    let last = path.path.segments.last()?;
    if last.ident != wrapper {
        return None;
    }
    let syn::PathArguments::AngleBracketed(args) = &last.arguments else {
        return None;
    };
    if args.args.len() != 1 {
        return None;
    }
    match args.args.first()? {
        syn::GenericArgument::Type(inner) => Some(inner.clone()),
        _ => None,
    }
}
