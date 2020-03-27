use proc_macro2::TokenStream;
use quote::quote;
use syn::{parse_quote, Type};
use synstructure::{BindingInfo, Structure};

mod coalesce;
mod unravel;

pub fn generate(item: Structure) -> TokenStream {
    let coalesce = coalesce::generate(item.clone());
    let unravel = unravel::generate(item.clone());

    quote! {
        #coalesce
        #unravel
    }
}

fn make_ty(item: Structure) -> Type {
    match item.variants().len() {
        1 => {
            let ty = write_bindings(item.variants()[0].bindings());
            parse_quote!(#ty)
        }
        _ => unimplemented!(),
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
