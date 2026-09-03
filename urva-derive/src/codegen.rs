use proc_macro2::TokenStream;
use quote::{format_ident, quote};
use syn::Ident;
use syn::ext::IdentExt;

use crate::case::to_snake;
use crate::entity::{EntityModel, FieldModel, StructModel, token_type};

pub fn entity_tokens(model: &EntityModel) -> TokenStream {
    let base = &model.base;
    let ident = &base.ident;
    let fields_mod = format_ident!("{}_fields", to_snake(&ident.unraw().to_string()));
    let fields_module = fields_module(
        base,
        &fields_mod,
        Some((&model.id_ty, model.version.as_deref())),
    );

    let collection = &model.collection;
    let id_ty = &model.id_ty;

    let (lock, version_field, marker) = match &model.version {
        Some(name) => (
            quote! { ::urva::Version },
            name.as_str(),
            quote! { ::urva::Versioned },
        ),
        None => (quote! { () }, "version", quote! { ::urva::Unversioned }),
    };

    quote! {
        #fields_module

        const _: () = {
        #[automatically_derived]
        impl ::urva::__private::Sealed for #ident {}

        #[automatically_derived]
        impl ::urva::Entity for #ident {
            const COLLECTION: &'static str = #collection;
            type Id = #id_ty;
            type Lock = #lock;
            const VERSION_FIELD: &'static str = #version_field;
            const INDEX_SPECS: &'static [&'static ::urva::IndexSpec] = &[];
        }

        #[automatically_derived]
        impl #marker for #ident {}
        };
    }
}

pub fn embedded_tokens(base: &StructModel) -> TokenStream {
    let ident = &base.ident;
    let fields_mod = format_ident!("{}_fields", to_snake(&ident.unraw().to_string()));
    let fields_module = fields_module(base, &fields_mod, None);
    quote! {
        #[automatically_derived]
        impl ::urva::__private::Sealed for #ident {}

        #[automatically_derived]
        impl ::urva::Embedded for #ident {}

        #fields_module
    }
}

fn encoder_ident(base: &StructModel, field: &FieldModel) -> Ident {
    format_ident!(
        "__urva_enc_{}_{}",
        to_snake(&base.ident.unraw().to_string()),
        field.ident.unraw()
    )
}

fn fields_module(
    base: &StructModel,
    fields_mod: &Ident,
    entity: Option<(&syn::Type, Option<&str>)>,
) -> TokenStream {
    let ident = &base.ident;
    let vis = &base.vis;
    let mut encoders = TokenStream::new();
    let mut consts: Vec<TokenStream> = base
        .fields
        .iter()
        .filter(|field| field.unstorable.is_none())
        .map(|field| {
            let fident = &field.ident;
            let stored = &field.stored_name;
            match &field.custom_writer_path {
                Some(writer) => {
                    let ty = &field.ty;
                    let encoder = encoder_ident(base, field);
                    encoders.extend(quote! {
                        #[doc(hidden)]
                        #[allow(non_camel_case_types)]
                        #vis struct #encoder;
                        #[automatically_derived]
                        impl ::urva::Encode<#ty> for #encoder {
                            fn encode(
                                value: &#ty,
                            ) -> ::core::result::Result<::urva::bson::Bson, ::urva::bson::ser::Error> {
                                #writer(value, ::urva::bson::Serializer::new())
                            }
                        }
                    });
                    quote! {
                        pub const #fident: ::urva::Field<#ident, #ty, ::urva::Full, ::urva::Encoded<#encoder>> =
                            ::urva::__private::new_field(#stored);
                    }
                }
                None => {
                    let ty = token_type(&field.ty);
                    quote! {
                        pub const #fident: ::urva::Field<#ident, #ty> =
                            ::urva::__private::new_field(#stored);
                    }
                }
            }
        })
        .collect();
    if let Some((id_ty, version)) = entity {
        consts.push(quote! {
            pub const _id: ::urva::Field<#ident, #id_ty, ::urva::MatchOnly> =
                ::urva::__private::new_field("_id");
        });
        if let Some(version) = version {
            let vident = format_ident!("{version}");
            consts.push(quote! {
                pub const #vident: ::urva::VersionField<#ident> =
                    ::urva::__private::new_version_field(#version);
            });
        }
    }
    quote! {
        #[allow(non_camel_case_types)]
        #vis enum #fields_mod {}
        #[allow(non_upper_case_globals)]
        impl #fields_mod {
            #(#consts)*
        }
        #encoders
    }
}
