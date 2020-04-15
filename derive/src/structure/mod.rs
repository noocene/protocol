use proc_macro2::TokenStream;
use quote::quote;
use syn::{parse_quote, Type};
use synstructure::{AddBounds, BindingInfo, Structure, VariantInfo};

mod coalesce;
mod unravel;

pub use coalesce::write_conv as write_coalesce_conv;
pub use unravel::make_conv as write_unravel_conv;

pub fn generate_adapted(mut item: Structure, adapter: Type) -> TokenStream {
    item.add_bounds(AddBounds::None);

    let coalesce = coalesce::generate_adapted(item.clone(), adapter.clone());
    let unravel = unravel::generate_adapted(item, adapter);

    quote! {
        #coalesce
        #unravel
    }
}

pub fn generate(mut item: Structure) -> TokenStream {
    item.add_bounds(AddBounds::None);

    let coalesce = coalesce::generate(item.clone());
    let unravel = unravel::generate(item.clone());

    quote! {
        #coalesce
        #unravel
    }
}

pub fn make_ty(item: Structure) -> Type {
    match item.variants().len() {
        1 => {
            let ty = write_bindings(item.variants()[0].bindings());
            parse_quote!(#ty)
        }
        _ => {
            let ty = write_variants(item.variants());
            parse_quote!(#ty)
        }
    }
}

fn write_variants(variants: &[VariantInfo]) -> TokenStream {
    let (a, b) = variants.split_at((variants.len() + 1) / 2);

    let a = if a.len() > 1 {
        Some(write_variants(a))
    } else if a.len() == 1 && a[0].bindings().len() == 0 {
        None
    } else {
        Some(write_bindings(a[0].bindings()))
    };
    let b = if b.len() > 1 {
        Some(write_variants(b))
    } else if b.len() == 1 && b[0].bindings().len() == 0 {
        None
    } else {
        Some(write_bindings(b[0].bindings()))
    };

    if let Some(a) = a {
        if let Some(b) = b {
            quote!(Result<#a, #b>)
        } else {
            quote!(Option<#a>)
        }
    } else {
        if let Some(b) = b {
            quote!(Option<#b>)
        } else {
            quote!(bool)
        }
    }
}

fn write_bindings(bindings: &[BindingInfo]) -> TokenStream {
    match bindings.len() {
        1 => {
            let ty = &bindings[0].ast().ty;
            quote! {
                #ty
            }
        }
        n if n <= 16 => {
            let mut stream = TokenStream::new();

            for binding in bindings {
                let ty = &binding.ast().ty;

                stream.extend(quote! {
                    #ty,
                })
            }

            quote!((#stream))
        }
        _ => {
            let mut stream = TokenStream::new();

            for binding in bindings.chunks(16) {
                let ty = write_bindings(binding);

                stream.extend(quote! {
                    #ty,
                })
            }

            quote!((#stream))
        }
    }
}
