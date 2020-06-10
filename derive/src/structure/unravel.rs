use proc_macro2::TokenStream;
use quote::quote;
use syn::Type;
use synstructure::{BindStyle, BindingInfo, Structure, VariantInfo};

pub fn generate(mut item: Structure) -> TokenStream {
    match item.variants().len() {
        0 => generate_never(item),
        1 if item.variants()[0].bindings().len() == 0 => generate_unit(item),
        _ => {
            item.bind_with(|_| BindStyle::Move);

            let ty = super::make_ty(item.clone());
            let conv = make_conv(item.clone());

            item.gen_impl(quote! {
                gen impl<C: ?Sized + __protocol::Write<<C as __protocol::Dispatch<#ty>>::Handle> + __protocol::Fork<#ty> + Unpin> __protocol::Unravel<C> for @Self
                where
                    <C as __protocol::Fork<#ty>>::Future: Unpin,
                    <C as __protocol::Fork<#ty>>::Target: Unpin,
                    <C as __protocol::Fork<#ty>>::Finalize: Unpin,
                    <C as __protocol::Dispatch<#ty>>::Handle: Unpin
                {
                    type Target = __protocol::FlatUnravel<#ty, C>;
                    type Finalize = <__protocol::FlatUnravel<#ty, C> as __protocol::Future<C>>::Ok;

                    fn unravel(self) -> Self::Target {
                        __protocol::FlatUnravel::new({
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

        gen impl<C: ?Sized + __protocol::Write<<C as __protocol::Dispatch<<#adapter as __protocol::adapter::Adapt<Self>>::Adapter>>::Handle> + __protocol::Fork<<#adapter as __protocol::adapter::Adapt<Self>>::Adapter> + Unpin> __protocol::Unravel<C> for @Self
        where
            <C as __protocol::Fork<<#adapter as __protocol::adapter::Adapt<Self>>::Adapter>>::Future: Unpin,
            <C as __protocol::Fork<<#adapter as __protocol::adapter::Adapt<Self>>::Adapter>>::Target: Unpin,
            <C as __protocol::Fork<<#adapter as __protocol::adapter::Adapt<Self>>::Adapter>>::Finalize: Unpin,
            <C as __protocol::Dispatch<<#adapter as __protocol::adapter::Adapt<Self>>::Adapter>>::Handle: Unpin
        {
            type Target = __protocol::FlatUnravel<<#adapter as __protocol::adapter::Adapt<Self>>::Adapter, C>;
            type Finalize = <__protocol::FlatUnravel<<#adapter as __protocol::adapter::Adapt<Self>>::Adapter, C> as __protocol::Future<C>>::Ok;

            fn unravel(self) -> Self::Target {
                __protocol::FlatUnravel::new(<#adapter as __protocol::adapter::Adapt<Self>>::wrap(self))
            }
        }
    })
}

fn generate_never(item: Structure) -> TokenStream {
    let message = format!("attempted to unravel bottom type {}", item.ast().ident);

    item.gen_impl(quote! {
        gen impl<C: ?Sized> __protocol::Unravel<C> for @Self {
            type Finalize = __protocol::future::Ready<()>;
            type Target = __protocol::future::Ready<Self::Finalize>;

            fn unravel(self) -> Self::Target {
                panic!(#message)
            }
        }
    })
}

fn generate_unit(item: Structure) -> TokenStream {
    item.gen_impl(quote! {
        gen impl<C: ?Sized> __protocol::Unravel<C> for @Self {
            type Finalize = __protocol::future::Ready<()>;
            type Target = __protocol::future::Ready<Self::Finalize>;

            fn unravel(self) -> Self::Target {
                __protocol::future::ok(__protocol::future::ok(()))
            }
        }
    })
}

fn write_bindings(bindings: &[BindingInfo]) -> TokenStream {
    match bindings.len() {
        1 => {
            let ty = &bindings[0].pat();
            quote! {
                #ty
            }
        }
        n if n <= 16 => {
            let mut stream = TokenStream::new();

            for binding in bindings {
                let ty = binding.pat();

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

fn conv_bindings(variant: &VariantInfo) -> TokenStream {
    match variant.bindings().len() {
        1 => {
            let conv = variant.each(|binding| {
                let pat = binding.pat();
                quote!(#pat)
            });
            quote! {
                match self {
                    #conv
                }
            }
        }
        _ => {
            let conv = write_bindings(variant.bindings());
            let pat = variant.pat();

            quote! {
                match self {
                    #pat => #conv
                }
            }
        }
    }
}

pub fn make_conv(item: Structure) -> TokenStream {
    match item.variants().len() {
        1 => conv_bindings(&item.variants()[0]),
        _ => {
            let mut stream = TokenStream::new();
            let arms = make_table(item.variants());

            for (arm, variant) in arms.iter().zip(item.variants()) {
                let variant = variant.pat();

                stream.extend(quote! {
                    #variant => #arm,
                });
            }

            quote! {
                match self {
                    #stream
                }
            }
        }
    }
}

fn make_table(variants: &[VariantInfo]) -> Vec<TokenStream> {
    let (a, b) = variants.split_at((variants.len() + 1) / 2);
    let _b = b;
    let _a = a;

    let a = if a.len() > 1 {
        Some(make_table(a))
    } else if a.len() == 1 && a[0].bindings().len() == 0 {
        None
    } else {
        Some(vec![write_bindings(a[0].bindings())])
    };
    let b = if b.len() > 1 {
        Some(make_table(b))
    } else if b.len() == 1 && b[0].bindings().len() == 0 {
        None
    } else {
        Some(vec![write_bindings(b[0].bindings())])
    };

    if let Some(a) = a {
        if let Some(b) = b {
            let mut items = vec![];
            for item in a {
                items.push(quote! { Ok(#item) })
            }
            for item in b {
                items.push(quote! { Err(#item) })
            }
            items
        } else {
            let mut items = vec![];
            for item in a {
                items.push(quote! { Some(#item) })
            }
            items.push(quote! { None });
            items
        }
    } else {
        if let Some(b) = b {
            let mut items = vec![];
            items.push(quote! { None });
            for item in b {
                items.push(quote! { Some(#item) })
            }
            items
        } else {
            vec![quote!(true), quote!(false)]
        }
    }
}
