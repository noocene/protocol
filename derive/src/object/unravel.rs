use proc_macro2::TokenStream;
use quote::{format_ident, quote};
use syn::{parse_quote, GenericParam, ItemTrait, TraitItem};

pub fn generate(item: ItemTrait) -> TokenStream {
    let ident = &item.ident;
    let mut type_item = item.clone();
    type_item
        .generics
        .params
        .push(parse_quote!(__DERIVE_PROTOCOL_TRANSPORT: ?Sized));
    type_item
        .generics
        .params
        .push(parse_quote!('derive_lifetime_param));
    let mut assoc_idents = vec![];
    for item in &item.items {
        if let TraitItem::Type(item) = item {
            let ident = format_ident!("__DERIVE_ASSOC_{}", item.ident);
            assoc_idents.push((item.ident.clone(), ident.clone()));
            let bounds = &item.bounds;

            type_item
                .generics
                .params
                .push(parse_quote!(#ident: #bounds));
        }
    }
    let (_, _, where_clause) = item.generics.split_for_impl();
    let (impl_generics, _, _) = type_item.generics.split_for_impl();

    let mut type_generics = TokenStream::new();

    for param in &item.generics.params {
        if let GenericParam::Type(ty) = param {
            let ident = &ty.ident;

            type_generics.extend(quote! {
                #ident,
            });
        }
    }

    for (name, ty) in assoc_idents {
        type_generics.extend(quote! {
            #name = #ty,
        });
    }

    if !type_generics.is_empty() {
        type_generics = quote!(<#type_generics>);
    }

    quote! {
        macro_rules! marker_variants {
            ($(
                $($marker:ident)*
            ),+) => {
                $(
                    #[allow(non_camel_case_types)]
                    impl #impl_generics __protocol::Unravel<__DERIVE_PROTOCOL_TRANSPORT> for Box<dyn #ident #type_generics + 'derive_lifetime_param $(+ $marker)*> #where_clause {
                        type Finalize = __protocol::future::Ready<()>;
                        type Target = __protocol::future::Ready<Self::Finalize>;

                        fn unravel(self) -> Self::Target {
                            __protocol::future::ok(__protocol::future::ok(()))
                        }
                    }
                )+
            }
        }

        marker_variants! {
            ,
            Sync,
            Send, Sync Send
        }
    }
}
