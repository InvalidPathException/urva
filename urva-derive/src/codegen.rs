use proc_macro2::TokenStream;
use quote::quote;

use crate::entity::{EntityModel, StructModel};

pub fn entity_tokens(model: &EntityModel) -> TokenStream {
    let ident = &model.base.ident;
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
        const _: () = {
        #[automatically_derived]
        impl ::urva::__private::Sealed for #ident {}

        #[automatically_derived]
        impl ::urva::Entity for #ident {
            const COLLECTION: &'static str = #collection;
            type Id = #id_ty;
            type Lock = #lock;
            const VERSION_FIELD: &'static str = #version_field;
        }

        #[automatically_derived]
        impl #marker for #ident {}
        };
    }
}

pub fn embedded_tokens(base: &StructModel) -> TokenStream {
    let ident = &base.ident;
    quote! {
        #[automatically_derived]
        impl ::urva::__private::Sealed for #ident {}

        #[automatically_derived]
        impl ::urva::Embedded for #ident {}
    }
}
