use proc_macro2::TokenStream;
use quote::quote;
use synstructure::{Structure, VariantInfo};
use syn::Index;

pub fn generate(item: Structure) -> TokenStream {
    match item.variants().len() {
        0 => generate_never(item),
        1 if item.variants()[0].bindings().len() == 0 => generate_unit(item),
        _ => {
            let ty = super::make_ty(item.clone());
            let conv = make_conv(item.clone());

            item.gen_impl(quote! {
                gen impl<C: ?Sized + __protocol::Read<<C as __protocol::Dispatch<#ty>>::Handle> + __protocol::Join<#ty> + Unpin> __protocol::Coalesce<C> for @Self 
                where
                    <C as __protocol::Join<#ty>>::Future: Unpin,
                    <C as __protocol::Dispatch<#ty>>::Handle: Unpin
                {
                    type Future = __protocol::future::MapOk<__protocol::FlatCoalesce<#ty, C>, fn(#ty) -> Self>;

                    fn coalesce() -> Self::Future {
                        __protocol::FutureExt::map_ok(__protocol::FlatCoalesce::new(), |data| {
                            #conv
                        })
                    }
                }
            })
        }
    }
}

fn generate_never(item: Structure) -> TokenStream {
    let name = format!("{}", item.ast().ident);

    item.gen_impl(quote! {
        gen impl<C: ?Sized> __protocol::Coalesce<C> for @Self {
            type Future = __protocol::future::Ready<Self, __protocol::BottomCoalesce>;

            fn coalesce() -> Self::Future {
                __protocol::future::err(__protocol::BottomCoalesce(#name))
            }
        }
    })
}

fn generate_unit(item: Structure) -> TokenStream {
    let construct = item.variants()[0].construct(|_, _| TokenStream::new());

    item.gen_impl(quote! {
        gen impl<C: ?Sized> __protocol::Coalesce<C> for @Self {
            type Future = __protocol::future::Ready<Self>;

            fn coalesce() -> Self::Future {
                __protocol::future::ok(#construct)
            }
        }
    })
}

fn make_index(len: usize, mut idx: usize) -> TokenStream {
    let mut stream = vec![];
    
    let count = ((len as f32).log2() / 4.0).ceil() as u32;

    for _ in 0..count {
        let index = Index::from(idx % 16);

        stream.push(quote! {
            .#index
        });

        idx /= 16;
    }

    let mut data = quote!(data);

    for item in stream.into_iter().rev() {
        data = quote!((#data)#item);
    }

    data
}

fn conv_bindings(variant: &VariantInfo) -> TokenStream {
    let len = variant.bindings().len();

    match len {
        1 => variant.construct(|_, _| quote!(data)),
        _ => {
            variant.construct(|_, idx| {
                make_index(len, idx)
            })
        },
    }
}

fn make_conv(item: Structure) -> TokenStream {
    match item.variants().len() {
        1 => {
            conv_bindings(&item.variants()[0])
        }
        _ => unimplemented!(),
    }
}
