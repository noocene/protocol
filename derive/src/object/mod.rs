use proc_macro2::TokenStream;
use quote::quote;
use syn::{ItemTrait, TraitItem};

mod coalesce;
mod unravel;

pub fn generate(item: ItemTrait) -> TokenStream {
    if item.supertraits.len() > 0
        || item.items.iter().any(|item| match item {
            TraitItem::Method(_) => true,
            _ => false,
        })
    {
        return quote!(compile_error!(
            "traits with supertraits or methods are currently unsupported"
        ));
    }

    let coalesce = coalesce::generate(item.clone());
    let unravel = unravel::generate(item);

    quote! {
        #coalesce
        #unravel
    }
}
