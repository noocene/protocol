use proc_macro2::TokenStream;
use quote::quote;
use syn::{Index, Type};
use synstructure::{Structure, VariantInfo};

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

pub fn generate_adapted(item: Structure, adapter: Type) -> TokenStream {
    item.gen_impl(quote! {
        compile_error!("adapters are currently broken due to https://github.com/rust-lang/rust/issues/70756");

        gen impl<C: ?Sized + __protocol::Read<<C as __protocol::Dispatch<<#adapter as __protocol::adapter::Adapt<Self>>::Adapter>>::Handle> + __protocol::Join<<#adapter as __protocol::adapter::Adapt<Self>>::Adapter> + Unpin> __protocol::Coalesce<C> for @Self
        where
            <C as __protocol::Join<<#adapter as __protocol::adapter::Adapt<Self>>::Adapter>>::Future: Unpin,
            <C as __protocol::Dispatch<<#adapter as __protocol::adapter::Adapt<Self>>::Adapter>>::Handle: Unpin
        {
            type Future = __protocol::future::MapOk<__protocol::FlatCoalesce<<#adapter as __protocol::adapter::Adapt<Self>>::Adapter, C>, fn(<#adapter as __protocol::adapter::Adapt<Self>>::Adapter) -> Self>;

            fn coalesce() -> Self::Future {
                __protocol::FutureExt::map_ok(__protocol::FlatCoalesce::new(), __protocol::adapter::Adapt::unwrap)
            }
        }
    })
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
        _ => variant.construct(|_, idx| make_index(len, idx)),
    }
}

fn make_conv(item: Structure) -> TokenStream {
    match item.variants().len() {
        1 => conv_bindings(&item.variants()[0]),
        _ => write_conv(item.variants()),
    }
}

pub fn write_conv(variants: &[VariantInfo]) -> TokenStream {
    let (a, b) = variants.split_at((variants.len() + 1) / 2);
    let _b = b;
    let _a = a;

    let a = if a.len() > 1 {
        Some(write_conv(a))
    } else if a.len() == 1 && a[0].bindings().len() == 0 {
        None
    } else {
        Some(conv_bindings(&a[0]))
    };
    let b = if b.len() > 1 {
        Some(write_conv(b))
    } else if b.len() == 1 && b[0].bindings().len() == 0 {
        None
    } else {
        Some(conv_bindings(&b[0]))
    };

    if let Some(a) = a {
        if let Some(b) = b {
            quote! {
                match data {
                    Ok(data) => #a,
                    Err(data) => #b
                }
            }
        } else {
            let b = conv_bindings(&_b[0]);
            quote!(match data {
                Some(data) => #a,
                None => #b
            })
        }
    } else {
        if let Some(b) = b {
            let a = conv_bindings(&_a[0]);
            quote!(match data {
                Some(data) => #b,
                None => #a
            })
        } else {
            let a = conv_bindings(&_a[0]);
            let b = conv_bindings(&_b[0]);

            quote! {
                match data {
                    true => #a,
                    false => #b
                }
            }
        }
    }
}
