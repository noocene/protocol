use proc_macro2::TokenStream;
use quote::quote;
use synstructure::{Structure, VariantInfo, BindStyle, BindingInfo};

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
        },
    }
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
            let ty = &bindings[0].ast().ty;
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

