use super::rewrite_ty;
use proc_macro2::TokenStream;
use quote::{format_ident, quote, quote_spanned};
use syn::{parse_quote, spanned::Spanned, FnArg, GenericParam, ItemTrait, ReturnType, TraitItem};

pub fn generate(mut item: ItemTrait) -> TokenStream {
    let mut has_methods = false;

    let mut arms = quote!();
    let mut state = quote!();
    let mut state_arms = quote!();
    let mut errors = quote!();
    let mut pending_arms = quote!();
    let mut error_params = quote!();
    let mut error_arg_tys = quote!();
    let mut error_param_names = quote!();
    let mut finalize_items = quote!();

    let target: FnArg = parse_quote!(self: Box<Self>);
    let mut r_generics = quote!();

    let (_, o_type_generics, _) = item.generics.split_for_impl();
    let ident = &item.ident;

    let mut some_by_ref = false;

    let self_ty = quote!(#ident #o_type_generics);

    for t in &item.items {
        if let TraitItem::Method(method) = t {
            has_methods = true;

            let m_ident = &method.sig.ident;

            let moves = method
                .sig
                .inputs
                .first()
                .map(|item| item == &target)
                .unwrap_or(false);

            let mut bindings = quote!();
            let mut args = quote!();

            let ret = match &method.sig.output {
                ReturnType::Default => quote!(()),
                ReturnType::Type(_, ty) => {
                    let span = ty.span();

                    let ty = if let Some(ty) = rewrite_ty(*ty.clone(), &self_ty) {
                        ty
                    } else {
                        return quote_spanned!(span => const __DERIVE_ERROR: () = { compile_error!(
                            "object-safe traits cannot use the `Self` type"
                        ); };);
                    };
                    quote!(#ty)
                }
            };

            for ty in method
                .sig
                .inputs
                .iter()
                .skip(1)
                .filter_map(|item| match item {
                    FnArg::Typed(ty) => Some(ty),
                    _ => None,
                })
            {
                let span = ty.span();
                let pat = &ty.pat;
                let ty = if let Some(ty) = rewrite_ty((*ty.ty).clone(), &self_ty) {
                    ty
                } else {
                    return quote_spanned!(span => const __DERIVE_ERROR: () = { compile_error!(
                        "object-safe traits cannot use the `Self` type"
                    ); };);
                };
                if moves {
                    r_generics.extend(quote!(#ty,));
                }
                bindings.extend(quote!(#ty,));
                args.extend(quote!(#pat,));
            }

            if moves {
                item.generics.where_clause.get_or_insert_with(|| parse_quote!(where)).predicates.push(parse_quote!(__protocol::RefContextTarget<__DERIVE_PROTOCOL_TRANSPORT>: __protocol::Notify<(#bindings)> + __protocol::Fork<#ret> + __protocol::Write<<__protocol::RefContextTarget<__DERIVE_PROTOCOL_TRANSPORT> as __protocol::Dispatch<#ret>>::Handle>));
                item.generics.where_clause.as_mut().unwrap().predicates.push(parse_quote!(<__protocol::RefContextTarget<__DERIVE_PROTOCOL_TRANSPORT> as __protocol::Join<__protocol::Notification<__protocol::RefContextTarget<__DERIVE_PROTOCOL_TRANSPORT>, (#bindings)>>>::Future: ::core::marker::Unpin));
                item.generics.where_clause.as_mut().unwrap().predicates.push(parse_quote!(<__protocol::RefContextTarget<__DERIVE_PROTOCOL_TRANSPORT> as __protocol::Notify<(#bindings)>>::Unwrap: ::core::marker::Unpin));
                item.generics.where_clause.as_mut().unwrap().predicates.push(parse_quote!(<__protocol::RefContextTarget<__DERIVE_PROTOCOL_TRANSPORT> as __protocol::Fork<#ret>>::Target: ::core::marker::Unpin));
                item.generics.where_clause.as_mut().unwrap().predicates.push(parse_quote!(<__protocol::RefContextTarget<__DERIVE_PROTOCOL_TRANSPORT> as __protocol::Fork<#ret>>::Future: ::core::marker::Unpin));
                item.generics.where_clause.as_mut().unwrap().predicates.push(parse_quote!(<__protocol::RefContextTarget<__DERIVE_PROTOCOL_TRANSPORT> as __protocol::Fork<#ret>>::Finalize: ::core::marker::Unpin));
                item.generics.where_clause.as_mut().unwrap().predicates.push(parse_quote!(<__protocol::RefContextTarget<__DERIVE_PROTOCOL_TRANSPORT> as __protocol::Dispatch<#ret>>::Handle: ::core::marker::Unpin));
                let s_ident = format_ident!("__DERIVE_JOIN_{}", m_ident);
                let e_ident = format_ident!("JOIN_{}", m_ident);
                let message = format!(
                    "failed to join arguments by move for method `{}`: {{0}}",
                    m_ident
                );
                arms.extend(quote!(__DERIVE_CALL::#m_ident(handle) => {
                    this.state = __DERIVE_UNRAVEL_STATE::#s_ident(__protocol::Join::<__protocol::Notification<__protocol::RefContextTarget<__DERIVE_PROTOCOL_TRANSPORT>, (#bindings)>>::join(&mut *ctx, handle));
                }));
                state.extend(quote!(#s_ident(<__protocol::RefContextTarget<__DERIVE_PROTOCOL_TRANSPORT> as __protocol::Join<__protocol::Notification<__protocol::RefContextTarget<__DERIVE_PROTOCOL_TRANSPORT>, (#bindings)>>>::Future),));
                let u_ident = format_ident!("__DERIVE_UNWRAP_{}", m_ident);
                state_arms.extend(quote! {
                    __DERIVE_UNRAVEL_STATE::#s_ident(future) => {
                        let wrapped = __protocol::derive_deps::ready!(::core::pin::Pin::new(future).poll(cx, &mut *ctx)).map_err(__DERIVE_UNRAVEL_ERROR::#e_ident)?;
                        this.state = __DERIVE_UNRAVEL_STATE::#u_ident(__protocol::Notify::<(#bindings)>::unwrap(&mut *ctx, wrapped));
                    }
                });
                errors.extend(quote! {
                    #[error(#message)]
                    #e_ident(#[source] #e_ident),
                });
                error_param_names.extend(quote!(#e_ident,));
                error_params.extend(quote!(#e_ident: __protocol::derive_deps::Error + 'static,));
                error_arg_tys.extend(quote!(<<__protocol::RefContextTarget<__DERIVE_PROTOCOL_TRANSPORT> as __protocol::Join<__protocol::Notification<__protocol::RefContextTarget<__DERIVE_PROTOCOL_TRANSPORT>, (#bindings)>>>::Future as __protocol::Future<__protocol::RefContextTarget<__DERIVE_PROTOCOL_TRANSPORT>>>::Error,));
                let s_ident = format_ident!("__DERIVE_UNWRAP_{}", m_ident);
                let e_ident = format_ident!("UNWRAP_{}", m_ident);
                let message = format!(
                    "failed to unwrap arguments by move for method `{}`: {{0}}",
                    m_ident
                );
                state.extend(quote!(#s_ident(<__protocol::RefContextTarget<__DERIVE_PROTOCOL_TRANSPORT> as __protocol::Notify<(#bindings)>>::Unwrap),));
                let u_ident = format_ident!("__DERIVE_FORK_{}", m_ident);
                state_arms.extend(quote! {
                    __DERIVE_UNRAVEL_STATE::#s_ident(future) => {
                        let (#args) = __protocol::derive_deps::ready!(::core::pin::Pin::new(future).poll(cx, &mut *ctx)).map_err(__DERIVE_UNRAVEL_ERROR::#e_ident)?;
                        this.state = __DERIVE_UNRAVEL_STATE::#u_ident(__protocol::FlatUnravel::new(this.instance.take().unwrap().#m_ident(#args)));
                    }
                });
                errors.extend(quote! {
                    #[error(#message)]
                    #e_ident(#[source] #e_ident),
                });
                error_param_names.extend(quote!(#e_ident,));
                error_params.extend(quote!(#e_ident: __protocol::derive_deps::Error + 'static,));
                error_arg_tys.extend(quote!(<<__protocol::RefContextTarget<__DERIVE_PROTOCOL_TRANSPORT> as __protocol::Notify<(#bindings)>>::Unwrap as __protocol::Future<__protocol::RefContextTarget<__DERIVE_PROTOCOL_TRANSPORT>>>::Error,));
                state.extend(quote! {
                    #u_ident(__protocol::FlatUnravel<#ret, __protocol::RefContextTarget<__DERIVE_PROTOCOL_TRANSPORT>>),
                });
                let e_ident = format_ident!("FORK_{}", m_ident);
                let k_ident = format_ident!("__DERIVE_FINALIZE_{}", m_ident);
                state_arms.extend(quote! {
                    __DERIVE_UNRAVEL_STATE::#u_ident(future) => {
                        let finalize = __protocol::derive_deps::ready!(::core::pin::Pin::new(future).poll(cx, &mut *ctx)).map_err(__DERIVE_UNRAVEL_ERROR::#e_ident)?;
                        this.state = __DERIVE_UNRAVEL_STATE::#k_ident(finalize);
                    }
                });
                let message = format!(
                    "failed to fork arguments by move for method `{}`: {{0}}",
                    m_ident
                );
                errors.extend(quote! {
                    #[error(#message)]
                    #e_ident(#[source] #e_ident),
                });
                error_param_names.extend(quote!(#e_ident,));
                error_params.extend(quote!(#e_ident: __protocol::derive_deps::Error + 'static,));
                error_arg_tys.extend(quote!(<__protocol::FlatUnravel<#ret, __protocol::RefContextTarget<__DERIVE_PROTOCOL_TRANSPORT>> as __protocol::Future<__protocol::RefContextTarget<__DERIVE_PROTOCOL_TRANSPORT>>>::Error,));
                state.extend(quote! {
                    #k_ident(<__protocol::FlatUnravel<#ret, __protocol::RefContextTarget<__DERIVE_PROTOCOL_TRANSPORT>> as __protocol::Future<__protocol::RefContextTarget<__DERIVE_PROTOCOL_TRANSPORT>>>::Ok),
                });
                state_arms.extend(quote! {
                    __DERIVE_UNRAVEL_STATE::#k_ident(future) => {
                        __protocol::derive_deps::ready!(::core::pin::Pin::new(future).poll(cx, &mut *ctx)).map_err(__DERIVE_UNRAVEL_ERROR::#e_ident)?;
                        this.state = __DERIVE_UNRAVEL_STATE::Done;
                    }
                });
            } else {
                some_by_ref = true;
                item.generics.where_clause.get_or_insert_with(|| parse_quote!(where)).predicates.push(parse_quote!(__protocol::RefContextTarget<__protocol::RefContextTarget<__DERIVE_PROTOCOL_TRANSPORT>>: __protocol::Fork<#ret>));
                item.generics.where_clause.as_mut().unwrap().predicates.push(parse_quote!(__protocol::RefContextTarget<__protocol::RefContextTarget<__DERIVE_PROTOCOL_TRANSPORT>>: ::core::marker::Unpin + __protocol::Notify<(#bindings)>));
                item.generics.where_clause.as_mut().unwrap().predicates.push(parse_quote!(<__protocol::RefContextTarget<__protocol::RefContextTarget<__DERIVE_PROTOCOL_TRANSPORT>> as __protocol::Fork<#ret>>::Future: Unpin));
                item.generics.where_clause.as_mut().unwrap().predicates.push(parse_quote!(<__protocol::RefContextTarget<__protocol::RefContextTarget<__DERIVE_PROTOCOL_TRANSPORT>> as __protocol::Fork<#ret>>::Finalize: Unpin));
                item.generics.where_clause.as_mut().unwrap().predicates.push(parse_quote!(<__protocol::RefContextTarget<__protocol::RefContextTarget<__DERIVE_PROTOCOL_TRANSPORT>> as __protocol::Fork<#ret>>::Target: Unpin));
                item.generics.where_clause.as_mut().unwrap().predicates.push(parse_quote!(<__protocol::RefContextTarget<__protocol::RefContextTarget<__DERIVE_PROTOCOL_TRANSPORT>> as __protocol::Notify<(#bindings)>>::Unwrap: Unpin));
                item.generics.where_clause.as_mut().unwrap().predicates.push(parse_quote!(<__protocol::RefContextTarget<__protocol::RefContextTarget<__DERIVE_PROTOCOL_TRANSPORT>> as __protocol::Join<<__protocol::RefContextTarget<__protocol::RefContextTarget<__DERIVE_PROTOCOL_TRANSPORT>> as __protocol::Notify<(#bindings)>>::Notification>>::Future: Unpin));
                item.generics.where_clause.as_mut().unwrap().predicates.push(parse_quote!(<__protocol::RefContextTarget<__DERIVE_PROTOCOL_TRANSPORT> as __protocol::ReferenceContext>::JoinOutput: Unpin));
                item.generics.where_clause.as_mut().unwrap().predicates.push(parse_quote!(__protocol::RefContextTarget<__protocol::RefContextTarget<__DERIVE_PROTOCOL_TRANSPORT>>: __protocol::Read<<__protocol::RefContextTarget<__protocol::RefContextTarget<__DERIVE_PROTOCOL_TRANSPORT>> as __protocol::Dispatch<
                    <__protocol::RefContextTarget<__protocol::RefContextTarget<__DERIVE_PROTOCOL_TRANSPORT>> as __protocol::Notify<(#bindings)>>::Notification,
                >>::Handle>));
                item.generics.where_clause.as_mut().unwrap().predicates.push(parse_quote!(__protocol::RefContextTarget<__protocol::RefContextTarget<__DERIVE_PROTOCOL_TRANSPORT>>: __protocol::Write<<__protocol::RefContextTarget<__protocol::RefContextTarget<__DERIVE_PROTOCOL_TRANSPORT>> as __protocol::Dispatch<
                    #ret
                >>::Handle>));
                let k_ident = format_ident!("__DERIVE_CONTEXTUALIZE_{}", m_ident);
                state.extend(quote!(#k_ident(<__protocol::RefContextTarget<__DERIVE_PROTOCOL_TRANSPORT> as __protocol::ReferenceContext>::JoinOutput),));
                arms.extend(quote!(__DERIVE_CALL::#m_ident(handle) => {
                    this.state = __DERIVE_UNRAVEL_STATE::#k_ident(__protocol::ReferenceContext::join_ref(&mut *ctx, handle));
                }));
                let e_ident = format_ident!("CONTEXTUALIZE_{}", m_ident);
                let f_ident = format_ident!("FINALIZE_{}", m_ident);
                state_arms.extend(quote! {
                    __DERIVE_UNRAVEL_STATE::#k_ident(future) => {
                        let context = __protocol::derive_deps::ready!(::core::pin::Pin::new(future).poll(cx, &mut *ctx)).map_err(__DERIVE_UNRAVEL_ERROR::#e_ident)?;
                        let mut fut = __protocol::derive_deps::BorrowUnravel::new(context);
                        let instance = this.instance.as_mut().unwrap();
                        if let ::core::task::Poll::Pending = __protocol::derive_deps::BorrowUnravel::poll(::core::pin::Pin::new(&mut fut),
                            cx,
                            &mut *ctx,
                            |(#args)| {
                                instance.#m_ident(#args)
                            }
                        ).map_err(__DERIVE_UNRAVEL_ERROR::#f_ident)? {
                            this.pending.push(__DERIVE_FINALIZE_ITEMS::#m_ident(fut));
                        }
                        this.state = __DERIVE_UNRAVEL_STATE::Read(::core::marker::PhantomData, ::core::marker::PhantomData);
                    }
                });
                pending_arms.extend(quote! {
                    __DERIVE_FINALIZE_ITEMS::#m_ident(fut) => {
                        let instance = this.instance.as_mut().unwrap();
                        __protocol::derive_deps::BorrowUnravel::poll(::core::pin::Pin::new(fut),
                            cx,
                            &mut *ctx,
                            |(#args)| {
                                instance.#m_ident(#args)
                            }
                        ).map_err(__DERIVE_UNRAVEL_ERROR::#f_ident)?
                    }
                });
                finalize_items.extend(quote! {
                    #m_ident(__protocol::derive_deps::BorrowUnravel<__DERIVE_PROTOCOL_TRANSPORT, (#bindings), #ret>),
                });
                let message = format!(
                    "failed to contextualize sub-instance for method `{}`: {{0}}",
                    m_ident
                );
                errors.extend(quote! {
                    #[error(#message)]
                    #e_ident(#[source] #e_ident),
                });
                error_param_names.extend(quote!(#e_ident,));
                error_params.extend(quote!(#e_ident: __protocol::derive_deps::Error + 'static,));
                error_arg_tys.extend(quote!(<<__protocol::RefContextTarget<__DERIVE_PROTOCOL_TRANSPORT> as __protocol::ReferenceContext>::JoinOutput as __protocol::Future<__protocol::RefContextTarget<__DERIVE_PROTOCOL_TRANSPORT>>>::Error,));
                let message = format!(
                    "failed to finalize sub-instance for method `{}`: {{0}}",
                    m_ident
                );
                errors.extend(quote! {
                    #[error(#message)]
                    #f_ident(#[source] #f_ident),
                });
                error_param_names.extend(quote!(#f_ident,));
                error_params.extend(quote!(#f_ident: __protocol::derive_deps::Error + 'static,));
                error_arg_tys.extend(quote!(__protocol::derive_deps::BorrowUnravelError<(#bindings), #ret, __DERIVE_PROTOCOL_TRANSPORT>,));
            }
        }
    }

    if some_by_ref {
        item.generics.where_clause.as_mut().unwrap().predicates.push(parse_quote!(__protocol::RefContextTarget<__DERIVE_PROTOCOL_TRANSPORT>: __protocol::ReferenceContext + __protocol::Read<<__protocol::RefContextTarget<__DERIVE_PROTOCOL_TRANSPORT> as __protocol::Contextualize>::Handle>));
    }

    let ident = &item.ident;
    let vis = &item.vis;
    let mut type_item = item.clone();

    if has_methods {
        type_item.generics.params.push(parse_quote!(
            __DERIVE_PROTOCOL_TRANSPORT: ?Sized
                + ::core::marker::Unpin
                + __protocol::ReferenceContext
                + __protocol::Write<<__DERIVE_PROTOCOL_TRANSPORT as __protocol::Contextualize>::Handle>
        ));
        item.generics
            .where_clause
            .get_or_insert_with(|| parse_quote!(where))
            .predicates
            .push(parse_quote!(
                <__DERIVE_PROTOCOL_TRANSPORT as __protocol::ReferenceContext>::ForkOutput:
                    ::core::marker::Unpin
            ));
        item.generics.where_clause.as_mut().unwrap().predicates.push(parse_quote! {
            __protocol::RefContextTarget<__DERIVE_PROTOCOL_TRANSPORT>: __protocol::ReferenceContext
                + __protocol::Read<__DERIVE_CALL_ALIAS<__protocol::RefContextTarget<__DERIVE_PROTOCOL_TRANSPORT>, #r_generics>>
                + ::core::marker::Unpin
        });
    } else {
        type_item
            .generics
            .params
            .push(parse_quote!(__DERIVE_PROTOCOL_TRANSPORT: ?Sized));
    }
    let mut assoc_idents = vec![];
    for item in &item.items {
        if let TraitItem::Type(item) = item {
            let ident = format_ident!("__DERIVE_ASSOC_{}", item.ident);
            assoc_idents.push((item.ident.clone(), ident.clone()));
            let bounds = &item.bounds;

            type_item.generics.params.push(if bounds.len() != 0 {
                parse_quote!(#ident: #bounds)
            } else {
                parse_quote!(#ident)
            });
        }
    }
    let mut a = type_item.generics.clone();
    type_item
        .generics
        .params
        .push(parse_quote!('derive_lifetime_param));
    let (_, _, where_clause) = item.generics.split_for_impl();
    let (impl_generics, _, _) = type_item.generics.split_for_impl();

    let mut type_generics = TokenStream::new();
    let mut o_type_generics = TokenStream::new();
    let mut a_type_generics = TokenStream::new();

    for param in &item.generics.params {
        if let GenericParam::Type(ty) = param {
            let ident = &ty.ident;

            type_generics.extend(quote! {
                #ident,
            });
            a_type_generics.extend(quote! {
                #ident,
            });
            o_type_generics.extend(quote! {
                #ident,
            });
        }
    }

    o_type_generics.extend(quote!(__DERIVE_PROTOCOL_TRANSPORT,));

    for (name, ty) in assoc_idents {
        type_generics.extend(quote! {
            #name = #ty,
        });
        o_type_generics.extend(quote! {
            #ty,
        });
        a_type_generics.extend(quote! {
            #ty,
        });
    }

    if !type_generics.is_empty() {
        type_generics = quote!(<#type_generics>);
    }

    a.params
        .push(parse_quote!(__DERIVE_TRAIT_INSTANCE: ?Sized + #ident #type_generics));
    let (k_impl_generics, _, _) = a.split_for_impl();

    let mut k_type_generics = o_type_generics.clone();

    o_type_generics.extend(quote!(__DERIVE_TRAIT_INSTANCE,));

    if !o_type_generics.is_empty() {
        o_type_generics = quote!(<#o_type_generics>);
    }

    if !k_type_generics.is_empty() {
        k_type_generics
            .extend(quote!(dyn #ident #type_generics + 'derive_lifetime_param $(+ $marker)*,));
        k_type_generics = quote!(<#k_type_generics>);
    }

    let (pending_field, pending_create) = if some_by_ref {
        (
            quote!(pending: __alloc::vec::Vec<__DERIVE_FINALIZE_ITEMS #o_type_generics>,),
            quote!(pending: __alloc::vec::Vec::new(),),
        )
    } else {
        (quote!(), quote!())
    };

    let done_arm = if some_by_ref {
        quote! {
            if this.pending.len() == 0 {
                return ::core::task::Poll::Ready(Ok(()));
            } else {
                return ::core::task::Poll::Pending;
            }
        }
    } else {
        quote!(return ::core::task::Poll::Ready(Ok(()));)
    };

    let pending_process = if some_by_ref {
        quote! {
            let mut i = 0;
            while i != this.pending.len() {
                if let ::core::task::Poll::Ready(()) = match &mut this.pending[i] {
                    __DERIVE_FINALIZE_ITEMS::__DERIVE_PHANTOM_MARKER(_, _) => {
                        panic!("got phantom marker state in finalizer")
                    }
                    #pending_arms
                } {
                    this.pending.remove(i);
                } else {
                    i += 1;
                }
            }
        }
    } else {
        quote!()
    };

    let mut unravel = if has_methods {
        quote! {
            #vis enum __DERIVE_UNRAVEL_STATE #k_impl_generics #where_clause {
                Read(::core::marker::PhantomData<(#a_type_generics #r_generics __DERIVE_TRAIT_INSTANCE)>, ::core::marker::PhantomData<__DERIVE_PROTOCOL_TRANSPORT>),
                #state
                Done
            }

            #[derive(Debug, __protocol::derive_deps::Error)]
            #[bounds(
                where
                    __DERIVE_FORK_CONTEXT: __protocol::derive_deps::Error + 'static,
                    __DERIVE_READ: __protocol::derive_deps::Error + 'static,
                    #error_params
            )]
            #vis enum __DERIVE_UNRAVEL_ERROR<__ERROR_PHANTOM: ::core::fmt::Debug, __DERIVE_FORK_CONTEXT, __DERIVE_READ, #error_param_names> {
                #[error("invalid error outcome")]
                __DERIVE_PHANTOM(__ERROR_PHANTOM),
                #[error("failed to contextualize erased object: {0}")]
                Contextualize(#[source] __DERIVE_FORK_CONTEXT),
                #[error("failed to read item handle in erased object: {0}")]
                Read(#[source] __DERIVE_READ),
                #errors
            }

            type __DERIVE_ERROR_ALIAS<__DERIVE_PROTOCOL_TRANSPORT, #a_type_generics> = __DERIVE_UNRAVEL_ERROR<
                ::core::marker::PhantomData<(#a_type_generics)>,
                __protocol::ForkContextRefError<
                    <<__DERIVE_PROTOCOL_TRANSPORT as __protocol::ReferenceContext>::ForkOutput as __protocol::Future<__DERIVE_PROTOCOL_TRANSPORT>>::Error,
                    <__DERIVE_PROTOCOL_TRANSPORT as __protocol::Write<<__DERIVE_PROTOCOL_TRANSPORT as __protocol::Contextualize>::Handle>>::Error,
                >,
                <__protocol::RefContextTarget<__DERIVE_PROTOCOL_TRANSPORT> as __protocol::Read<__DERIVE_CALL_ALIAS<__protocol::RefContextTarget<__DERIVE_PROTOCOL_TRANSPORT>, #r_generics>>>::Error,
                #error_arg_tys
            >;

            #vis struct __DERIVE_UNRAVEL_SHIM #k_impl_generics #where_clause {
                context: <__DERIVE_PROTOCOL_TRANSPORT as __protocol::ReferenceContext>::Context,
                data: ::core::marker::PhantomData<(#a_type_generics #r_generics)>,
                instance: Option<__alloc::boxed::Box<__DERIVE_TRAIT_INSTANCE>>,
                #pending_field
                state: __DERIVE_UNRAVEL_STATE #o_type_generics
            }

            impl #k_impl_generics ::core::marker::Unpin for __DERIVE_UNRAVEL_SHIM #o_type_generics #where_clause {}

            impl #k_impl_generics __protocol::Future<__DERIVE_PROTOCOL_TRANSPORT> for __DERIVE_UNRAVEL_SHIM #o_type_generics #where_clause {
                type Ok = ();
                type Error = __DERIVE_ERROR_ALIAS<__DERIVE_PROTOCOL_TRANSPORT, #a_type_generics>;

                fn poll<R: ::core::borrow::BorrowMut<__DERIVE_PROTOCOL_TRANSPORT>>(mut self: ::core::pin::Pin<&mut Self>, cx: &mut ::core::task::Context, mut ctx: R) -> ::core::task::Poll<Result<(), __DERIVE_ERROR_ALIAS<__DERIVE_PROTOCOL_TRANSPORT, #a_type_generics>>> {
                    let this = &mut *self;
                    let ctx = __protocol::ContextReference::with(&mut this.context, ctx.borrow_mut());
                    #pending_process

                    loop {
                        match &mut this.state {
                            __DERIVE_UNRAVEL_STATE::Read(_, _) => {
                                let handle = __DERIVE_CALL::<__protocol::RefContextTarget<__DERIVE_PROTOCOL_TRANSPORT>, #r_generics>::from(__protocol::derive_deps::ready!(__protocol::Read::<__DERIVE_CALL_ALIAS<__protocol::RefContextTarget<__DERIVE_PROTOCOL_TRANSPORT>, #r_generics>>::read(::core::pin::Pin::new(&mut *ctx), cx)).map_err(__DERIVE_UNRAVEL_ERROR::Read)?);
                                match handle {
                                    __DERIVE_CALL::__DERIVE_TERMINATE => {
                                        this.state = __DERIVE_UNRAVEL_STATE::Done;
                                    }
                                    #arms
                                }
                            }
                            #state_arms
                            __DERIVE_UNRAVEL_STATE::Done => {
                                #done_arm
                            }
                        }
                    }
                }
            }
        }
    } else {
        quote!()
    };

    if some_by_ref {
        unravel.extend(quote! {
            #vis enum __DERIVE_FINALIZE_ITEMS #k_impl_generics #where_clause {
                __DERIVE_PHANTOM_MARKER(::core::marker::PhantomData<(#a_type_generics #r_generics __DERIVE_TRAIT_INSTANCE)>, ::core::marker::PhantomData<__DERIVE_PROTOCOL_TRANSPORT>),
                #finalize_items
            }
        });
    }

    let stream = if has_methods {
        quote! {
            type Finalize = __DERIVE_UNRAVEL_SHIM #k_type_generics;
            type Target = __protocol::future::MapErr<__protocol::ForkContextRef<__DERIVE_PROTOCOL_TRANSPORT, __DERIVE_UNRAVEL_SHIM #k_type_generics, Self, fn(Self, <__DERIVE_PROTOCOL_TRANSPORT as __protocol::ReferenceContext>::Context) -> __DERIVE_UNRAVEL_SHIM #k_type_generics>, fn(__protocol::ForkContextRefError<
                <<__DERIVE_PROTOCOL_TRANSPORT as __protocol::ReferenceContext>::ForkOutput as __protocol::Future<__DERIVE_PROTOCOL_TRANSPORT>>::Error,
                <__DERIVE_PROTOCOL_TRANSPORT as __protocol::Write<<__DERIVE_PROTOCOL_TRANSPORT as __protocol::Contextualize>::Handle>>::Error,
            >) -> __DERIVE_ERROR_ALIAS<__DERIVE_PROTOCOL_TRANSPORT, #a_type_generics>>;

            fn unravel(self) -> Self::Target {
                __protocol::FutureExt::map_err(__protocol::ForkContextRef::new(self, |instance, context| {
                    __DERIVE_UNRAVEL_SHIM {
                        context,
                        state: __DERIVE_UNRAVEL_STATE::Read(::core::marker::PhantomData, ::core::marker::PhantomData),
                        data: ::core::marker::PhantomData,
                        #pending_create
                        instance: Some(instance)
                    }
                }), __DERIVE_UNRAVEL_ERROR::Contextualize)
            }
        }
    } else {
        quote! {
            type Finalize = __protocol::future::Ready<()>;
            type Target = __protocol::future::Ready<Self::Finalize>;

            fn unravel(self) -> Self::Target {
                __protocol::future::ok(__protocol::future::ok(()))
            }
        }
    };

    quote! {
        #unravel

        macro_rules! marker_variants {
            ($(
                $($marker:ident)*
            ),+) => {
                $(
                    impl #impl_generics __protocol::Unravel<__DERIVE_PROTOCOL_TRANSPORT> for __alloc::boxed::Box<dyn #ident #type_generics + 'derive_lifetime_param $(+ $marker)*> #where_clause {
                        #stream
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
