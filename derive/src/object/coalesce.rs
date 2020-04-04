use proc_macro2::TokenStream;
use quote::{format_ident, quote};
use syn::{parse_quote, GenericParam, ItemTrait, TraitItem};

pub fn generate(mut item: ItemTrait) -> TokenStream {
    let ident = &item.ident;
    let vis = &item.vis;
    let mut type_item = item.clone();
    let mut assoc_idents = vec![];
    for item in &mut item.items {
        if let TraitItem::Type(item) = item {
            let ident = format_ident!("__DERIVE_ASSOC_{}", item.ident);
            assoc_idents.push((item.ident.clone(), ident.clone()));
            let bounds = &mut item.bounds;
            bounds.push(parse_quote!('derive_lifetime_param));

            type_item
                .generics
                .params
                .push(parse_quote!(#ident: #bounds));
        }
    }
    let (_, o_type_generics, where_clause) = item.generics.split_for_impl();

    let mut type_generics = TokenStream::new();
    let mut assoc_only = TokenStream::new();

    for param in &item.generics.params {
        if let GenericParam::Type(ty) = param {
            let ident = &ty.ident;

            type_generics.extend(quote! {
                #ident,
            });
        }
    }

    let mut impl_stream = TokenStream::new();

    for (name, ty) in assoc_idents {
        type_generics.extend(quote! {
            #name = #ty,
        });
        assoc_only.extend(quote!(#ty,));
        impl_stream.extend(quote! {
            type #name = #ty;
        });
    }

    if !type_generics.is_empty() {
        type_generics = quote!(<#type_generics>);
    }

    let mut c = type_item.generics.clone();
    c.params.push(parse_quote!('derive_lifetime_param));
    let (o_impl_generics, _, _) = c.split_for_impl();

    type_item
        .generics
        .params
        .push(parse_quote!(__DERIVE_PROTOCOL_TRANSPORT: ?Sized));
    type_item
        .generics
        .params
        .push(parse_quote!('derive_lifetime_param));
    let (impl_generics, _, _) = type_item.generics.split_for_impl();

    let shim = if assoc_only.is_empty() {
        quote!(#vis struct __DERIVE_COALESCE_SHIM(::core::marker::PhantomData<()>);)
    } else {
        #[cfg(feature = "std")]
        quote! {
            #[allow(non_camel_case_types)]
            #vis struct __DERIVE_COALESCE_SHIM<#assoc_only>(::core::marker::PhantomData<*const (#assoc_only)>);

            #[allow(non_camel_case_types)]
            impl<#assoc_only> ::core::marker::Unpin for __DERIVE_COALESCE_SHIM<#assoc_only> {}
            #[allow(non_camel_case_types)]
            unsafe impl<#assoc_only> ::core::marker::Sync for __DERIVE_COALESCE_SHIM<#assoc_only> {}
            #[allow(non_camel_case_types)]
            impl<#assoc_only> ::std::panic::UnwindSafe for __DERIVE_COALESCE_SHIM<#assoc_only> {}
            #[allow(non_camel_case_types)]
            impl<#assoc_only> ::std::panic::RefUnwindSafe for __DERIVE_COALESCE_SHIM<#assoc_only> {}
            #[allow(non_camel_case_types)]
            unsafe impl<#assoc_only> ::core::marker::Send for __DERIVE_COALESCE_SHIM<#assoc_only> {}
        }
        #[cfg(not(feature = "std"))]
        quote! {
            #[allow(non_camel_case_types)]
            #vis struct __DERIVE_COALESCE_SHIM<#assoc_only>(::core::marker::PhantomData<*const (#assoc_only)>);

            #[allow(non_camel_case_types)]
            impl<#assoc_only> ::core::marker::Unpin for __DERIVE_COALESCE_SHIM<#assoc_only> {}
            #[allow(non_camel_case_types)]
            unsafe impl<#assoc_only> ::core::marker::Sync for __DERIVE_COALESCE_SHIM<#assoc_only> {}
            #[allow(non_camel_case_types)]
            unsafe impl<#assoc_only> ::core::marker::Send for __DERIVE_COALESCE_SHIM<#assoc_only> {}
        }
    };

    let assoc_only = if assoc_only.is_empty() {
        quote!()
    } else {
        quote!(<#assoc_only>)
    };

    quote! {
        #shim

        #[allow(non_camel_case_types)]
        impl #o_impl_generics #ident #o_type_generics for __DERIVE_COALESCE_SHIM #assoc_only #where_clause {
            #impl_stream
        }

        macro_rules! marker_variants {
            ($(
                $($marker:ident)*
            ),+) => {
                $(
                    #[allow(non_camel_case_types)]
                    impl #impl_generics __protocol::Coalesce<__DERIVE_PROTOCOL_TRANSPORT> for Box<dyn #ident #type_generics + 'derive_lifetime_param $(+ $marker)*> #where_clause {
                        type Future = __protocol::future::Ready<Self>;

                        fn coalesce() -> Self::Future {
                            __protocol::future::ok(Box::new(__DERIVE_COALESCE_SHIM(::core::marker::PhantomData)))
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
