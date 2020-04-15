use super::rewrite_ty;
use proc_macro2::TokenStream;
use quote::{format_ident, quote};
use syn::{parse_quote, FnArg, GenericParam, ItemTrait, ReturnType, TraitItem};

pub fn generate(mut item: ItemTrait) -> TokenStream {
    let ident = &item.ident;
    let vis = &item.vis;
    let mut type_item = item.clone();
    let mut type_item_b = item.clone();
    let mut assoc_idents = vec![];
    for item in &mut item.items {
        if let TraitItem::Type(item) = item {
            let ident = format_ident!("__DERIVE_ASSOC_{}", item.ident);
            assoc_idents.push((item.ident.clone(), ident.clone()));
            let mut bounds = item.bounds.clone();
            type_item_b
                .generics
                .params
                .push(parse_quote!(#ident: #bounds));

            bounds.push(parse_quote!('derive_lifetime_param));

            type_item
                .generics
                .params
                .push(parse_quote!(#ident: #bounds));
        }
    }
    let (_, o_type_generics, where_clause) = item.generics.split_for_impl();
    let p_where_clause = where_clause.cloned();
    let mut where_clause = where_clause.cloned().unwrap_or(parse_quote!(where));
    let mut ty_where_clause = where_clause.clone();

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
    let mut delegate_stream = TokenStream::new();

    for (name, ty) in assoc_idents {
        type_generics.extend(quote! {
            #name = #ty,
        });
        assoc_only.extend(quote!(#ty,));
        impl_stream.extend(quote! {
            type #name = #ty;
        });
        delegate_stream.extend(quote! {
            type #name = #ty;
        });
    }

    if !type_generics.is_empty() {
        type_generics = quote!(<#type_generics>);
    }

    let mut c = type_item.generics.clone();
    c.params.push(parse_quote!('derive_lifetime_param));
    type_item_b
        .generics
        .params
        .push(parse_quote!('derive_lifetime_param));

    let mut call = quote!();
    let mut r_generics = quote!();
    let mut r_context_bounds = quote!();
    let mut d_context_bounds = quote!();
    let mut c_context_bounds = quote!();

    let target: FnArg = parse_quote!(self: Box<Self>);

    let mut ident_idx = 0;

    let mut some_by_ref = false;

    let self_ty = quote!(#ident #o_type_generics);

    let mut bounds = vec![];
    let mut ref_bounds = vec![];

    let mut has_methods = false;

    for item in &item.items {
        if let TraitItem::Method(method) = item {
            has_methods = true;

            let moves = method
                .sig
                .inputs
                .first()
                .map(|item| item == &target)
                .unwrap_or(false);

            let ident = &method.sig.ident;

            let mut args = quote!();
            let mut r_args = quote!();
            let mut bindings = quote!();

            let ret = match &method.sig.output {
                ReturnType::Default => quote!(()),
                ReturnType::Type(_, ty) => {
                    let ty = rewrite_ty(*ty.clone(), &self_ty);
                    quote!(#ty)
                }
            };

            let arg_tys: Vec<_> = method
                .sig
                .inputs
                .iter()
                .skip(1)
                .filter_map(|item| match item {
                    FnArg::Typed(ty) => Some(ty),
                    _ => None,
                })
                .collect();

            for ty in &arg_tys {
                let pat = *ty.pat.clone();
                bindings.extend(quote!(#pat,));
                let ty = rewrite_ty((*ty.ty).clone(), &self_ty);
                r_args.extend(quote!(#ty,));
            }

            let get_context = if moves {
                for ty in &arg_tys {
                    let ident = format_ident!("T{}", format!("{}", ident_idx));
                    let ty = rewrite_ty((*ty.ty).clone(), &self_ty);
                    ident_idx += 1;
                    r_generics.extend(quote!(#ty,));

                    args.extend(quote!(#ident,));
                }
                let n_args = if r_args.is_empty() {
                    quote!(())
                } else {
                    r_args.clone()
                };
                r_context_bounds
                    .extend(quote!(+ __protocol::Notify<(#n_args)> + ::core::marker::Unpin));
                d_context_bounds.extend(quote!(+ __protocol::Notify<(#n_args)>));
                c_context_bounds
                    .extend(quote!(+ __protocol::Notify<(#n_args)> + ::core::marker::Unpin));
                r_context_bounds.extend(quote! {
                    + __protocol::Join<#ret>
                    + __protocol::Read<<__DERIVE_PROTOCOL_TRANSPORT as __protocol::Dispatch<#ret>>::Handle>
                    + __protocol::Finalize<<__DERIVE_PROTOCOL_TRANSPORT as __protocol::Fork<<__DERIVE_PROTOCOL_TRANSPORT as __protocol::Notify<(#n_args)>>::Notification>>::Finalize>
                });
                where_clause.predicates.push(parse_quote!(<<__DERIVE_PROTOCOL_TRANSPORT as __protocol::Fork<<__DERIVE_PROTOCOL_TRANSPORT as __protocol::Notify<(#n_args)>>::Notification>>::Future as __protocol::Future<__DERIVE_PROTOCOL_TRANSPORT>>::Error: __protocol::derive_deps::Error + ::core::marker::Send + 'static));
                where_clause.predicates.push(parse_quote!(<<__DERIVE_PROTOCOL_TRANSPORT as __protocol::Notify<(#n_args)>>::Wrap as __protocol::Future<__DERIVE_PROTOCOL_TRANSPORT>>::Error: __protocol::derive_deps::Error + ::core::marker::Send + 'static));
                where_clause.predicates.push(parse_quote!(<__DERIVE_PROTOCOL_TRANSPORT as __protocol::Read<<__DERIVE_PROTOCOL_TRANSPORT as __protocol::Dispatch<#ret>>::Handle>>::Error: __protocol::derive_deps::Error + ::core::marker::Send + 'static));
                where_clause.predicates.push(parse_quote!(<<__DERIVE_PROTOCOL_TRANSPORT as __protocol::Join<#ret>>::Future as __protocol::Future<__DERIVE_PROTOCOL_TRANSPORT>>::Error: __protocol::derive_deps::Error + ::core::marker::Send + 'static));
                where_clause.predicates.push(parse_quote!(<<__DERIVE_PROTOCOL_TRANSPORT as __protocol::Fork<<__DERIVE_PROTOCOL_TRANSPORT as __protocol::Notify<(#n_args)>>::Notification>>::Target as __protocol::Future<__DERIVE_PROTOCOL_TRANSPORT>>::Error: __protocol::derive_deps::Error + ::core::marker::Send + 'static));
                where_clause.predicates.push(parse_quote!(<<__DERIVE_PROTOCOL_TRANSPORT as __protocol::Finalize<<__DERIVE_PROTOCOL_TRANSPORT as __protocol::Fork<<__DERIVE_PROTOCOL_TRANSPORT as __protocol::Notify<(#n_args)>>::Notification>>::Finalize>>::Output as __protocol::Future<__DERIVE_PROTOCOL_TRANSPORT>>::Error: __protocol::derive_deps::Error + ::core::marker::Send + 'static));
                where_clause.predicates.push(parse_quote! {
                    <__DERIVE_PROTOCOL_TRANSPORT as __protocol::Fork<<__DERIVE_PROTOCOL_TRANSPORT as __protocol::Notify<(#n_args)>>::Notification>>::Finalize: __protocol::Future<
                        <__DERIVE_PROTOCOL_TRANSPORT as __protocol::Finalize<
                            <__DERIVE_PROTOCOL_TRANSPORT as __protocol::Fork<<__DERIVE_PROTOCOL_TRANSPORT as __protocol::Notify<(#n_args)>>::Notification>>::Finalize,
                        >>::Target,
                    >
                });
                where_clause.predicates.push(parse_quote! {
                    <__DERIVE_PROTOCOL_TRANSPORT as __protocol::Finalize<
                        <__DERIVE_PROTOCOL_TRANSPORT as __protocol::Fork<<__DERIVE_PROTOCOL_TRANSPORT as __protocol::Notify<(#n_args)>>::Notification>>::Finalize,
                    >>::Output: ::core::marker::Unpin
                });
                where_clause.predicates.push(parse_quote!(<__DERIVE_PROTOCOL_TRANSPORT as __protocol::Join<#ret>>::Future: ::core::marker::Unpin));
                where_clause.predicates.push(parse_quote!(<__DERIVE_PROTOCOL_TRANSPORT as __protocol::Fork<<__DERIVE_PROTOCOL_TRANSPORT as __protocol::Notify<(#n_args)>>::Notification>>::Target: ::core::marker::Unpin));
                where_clause.predicates.push(parse_quote!(<__DERIVE_PROTOCOL_TRANSPORT as __protocol::Notify<(#n_args)>>::Wrap: ::core::marker::Unpin));
                where_clause.predicates.push(parse_quote!(<__DERIVE_PROTOCOL_TRANSPORT as __protocol::Fork<<__DERIVE_PROTOCOL_TRANSPORT as __protocol::Notify<(#n_args)>>::Notification>>::Future: ::core::marker::Unpin));
                bounds.push((quote!(#ret), quote!(#n_args)));
                call.extend(quote! {
                    #ident(<C as __protocol::Dispatch<<C as __protocol::Notify<(#args)>>::Notification>>::Handle),
                });
                quote!(let mut this = self; let context = this.1.take().unwrap();)
            } else {
                r_context_bounds.extend(quote!(+ ::core::marker::Unpin));
                c_context_bounds.extend(quote!(+ ::core::marker::Unpin));
                some_by_ref = true;
                where_clause.predicates.push(parse_quote! {
                    <__DERIVE_PROTOCOL_TRANSPORT as __protocol::CloneContext>::Context: __protocol::Join<#ret>
                        + __protocol::Notify<(#r_args)>
                        + __protocol::Finalize<<<__DERIVE_PROTOCOL_TRANSPORT as __protocol::CloneContext>::Context as __protocol::Fork<<<__DERIVE_PROTOCOL_TRANSPORT as __protocol::CloneContext>::Context as __protocol::Notify<(#r_args)>>::Notification>>::Finalize>
                        + __protocol::Read<<<__DERIVE_PROTOCOL_TRANSPORT as __protocol::CloneContext>::Context as __protocol::Dispatch<#ret>>::Handle>
                        + __protocol::Write<<<__DERIVE_PROTOCOL_TRANSPORT as __protocol::CloneContext>::Context as __protocol::Dispatch<<<__DERIVE_PROTOCOL_TRANSPORT as __protocol::CloneContext>::Context as __protocol::Notify<(#r_args)>>::Notification>>::Handle>
                });
                where_clause.predicates.push(parse_quote! {
                    <<<__DERIVE_PROTOCOL_TRANSPORT as __protocol::CloneContext>::Context as __protocol::Join<
                        #ret,
                    >>::Future as __protocol::Future<<__DERIVE_PROTOCOL_TRANSPORT as __protocol::CloneContext>::Context>>::Error: ::core::marker::Send + __protocol::derive_deps::Error + 'static
                });
                where_clause.predicates.push(parse_quote! {
                    <<__DERIVE_PROTOCOL_TRANSPORT as __protocol::CloneContext>::ForkOutput as __protocol::Future<__DERIVE_PROTOCOL_TRANSPORT>>::Error: ::core::marker::Send + __protocol::derive_deps::Error + 'static
                });
                where_clause.predicates.push(parse_quote! {
                    <__DERIVE_PROTOCOL_TRANSPORT as __protocol::CloneContext>::ForkOutput: ::core::marker::Unpin
                });
                where_clause.predicates.push(parse_quote! {
                    <<__DERIVE_PROTOCOL_TRANSPORT as __protocol::CloneContext>::Context as __protocol::Join<
                        #ret,
                    >>::Future: ::core::marker::Unpin
                });
                where_clause.predicates.push(parse_quote! {
                    <__DERIVE_PROTOCOL_TRANSPORT as __protocol::CloneContext>::Context: ::core::marker::Unpin
                });
                where_clause.predicates.push(parse_quote! {
                    <<__DERIVE_PROTOCOL_TRANSPORT as __protocol::CloneContext>::Context as __protocol::Read<
                        <<__DERIVE_PROTOCOL_TRANSPORT as __protocol::CloneContext>::Context as __protocol::Dispatch<
                            #ret,
                        >>::Handle,
                    >>::Error: ::core::marker::Send + __protocol::derive_deps::Error + 'static
                });
                where_clause.predicates.push(parse_quote! {
                    <<<__DERIVE_PROTOCOL_TRANSPORT as __protocol::CloneContext>::Context as __protocol::Fork<
                        <<__DERIVE_PROTOCOL_TRANSPORT as __protocol::CloneContext>::Context as __protocol::Notify<(#r_args)>>::Notification,
                    >>::Future as __protocol::Future<<__DERIVE_PROTOCOL_TRANSPORT as __protocol::CloneContext>::Context>>::Error: ::core::marker::Send + __protocol::derive_deps::Error + 'static
                });
                where_clause.predicates.push(parse_quote! {
                    <<<__DERIVE_PROTOCOL_TRANSPORT as __protocol::CloneContext>::Context as __protocol::Fork<
                        <<__DERIVE_PROTOCOL_TRANSPORT as __protocol::CloneContext>::Context as __protocol::Notify<(#r_args)>>::Notification,
                    >>::Target as __protocol::Future<<__DERIVE_PROTOCOL_TRANSPORT as __protocol::CloneContext>::Context>>::Error: ::core::marker::Send + __protocol::derive_deps::Error + 'static
                });
                where_clause.predicates.push(parse_quote! {
                    <<__DERIVE_PROTOCOL_TRANSPORT as __protocol::CloneContext>::Context as __protocol::Fork<
                        <<__DERIVE_PROTOCOL_TRANSPORT as __protocol::CloneContext>::Context as __protocol::Notify<(#r_args)>>::Notification,
                    >>::Future: ::core::marker::Unpin
                });
                where_clause.predicates.push(parse_quote! {
                    <<__DERIVE_PROTOCOL_TRANSPORT as __protocol::CloneContext>::Context as __protocol::Fork<
                        <<__DERIVE_PROTOCOL_TRANSPORT as __protocol::CloneContext>::Context as __protocol::Notify<(#r_args)>>::Notification,
                    >>::Target: ::core::marker::Unpin
                });
                where_clause.predicates.push(parse_quote! {
                    <<__DERIVE_PROTOCOL_TRANSPORT as __protocol::CloneContext>::Context as __protocol::Notify<(#r_args)>>::Wrap: ::core::marker::Unpin
                });
                where_clause.predicates.push(parse_quote! {
                    <<<__DERIVE_PROTOCOL_TRANSPORT as __protocol::CloneContext>::Context as __protocol::Notify<(#r_args)>>::Wrap as __protocol::Future<<__DERIVE_PROTOCOL_TRANSPORT as __protocol::CloneContext>::Context>>::Error: ::core::marker::Send + __protocol::derive_deps::Error + 'static
                });
                where_clause.predicates.push(parse_quote! {
                    <<__DERIVE_PROTOCOL_TRANSPORT as __protocol::CloneContext>::Context as __protocol::Fork<<<__DERIVE_PROTOCOL_TRANSPORT as __protocol::CloneContext>::Context as __protocol::Notify<(#r_args)>>::Notification>>::Finalize: __protocol::Future<
                        <<__DERIVE_PROTOCOL_TRANSPORT as __protocol::CloneContext>::Context as __protocol::Finalize<
                            <<__DERIVE_PROTOCOL_TRANSPORT as __protocol::CloneContext>::Context as __protocol::Fork<<<__DERIVE_PROTOCOL_TRANSPORT as __protocol::CloneContext>::Context as __protocol::Notify<(#r_args)>>::Notification>>::Finalize,
                        >>::Target,
                    >
                });
                where_clause.predicates.push(parse_quote! {
                    <<<__DERIVE_PROTOCOL_TRANSPORT as __protocol::CloneContext>::Context as __protocol::Finalize<
                        <<__DERIVE_PROTOCOL_TRANSPORT as __protocol::CloneContext>::Context as __protocol::Fork<
                            <<__DERIVE_PROTOCOL_TRANSPORT as __protocol::CloneContext>::Context as __protocol::Notify<(#r_args)>>::Notification,
                        >>::Finalize,
                    >>::Output as __protocol::Future<<__DERIVE_PROTOCOL_TRANSPORT as __protocol::CloneContext>::Context>>::Error: ::core::marker::Send + __protocol::derive_deps::Error + 'static
                });
                where_clause.predicates.push(parse_quote! {
                    <<__DERIVE_PROTOCOL_TRANSPORT as __protocol::CloneContext>::Context as __protocol::Finalize<
                        <<__DERIVE_PROTOCOL_TRANSPORT as __protocol::CloneContext>::Context as __protocol::Fork<
                            <<__DERIVE_PROTOCOL_TRANSPORT as __protocol::CloneContext>::Context as __protocol::Notify<(#r_args)>>::Notification,
                        >>::Finalize,
                    >>::Output: ::core::marker::Unpin
                });
                where_clause.predicates.push(parse_quote! {
                    <<__DERIVE_PROTOCOL_TRANSPORT as __protocol::CloneContext>::Context as __protocol::Write<
                        <<__DERIVE_PROTOCOL_TRANSPORT as __protocol::CloneContext>::Context as __protocol::Dispatch<<<__DERIVE_PROTOCOL_TRANSPORT as __protocol::CloneContext>::Context as __protocol::Notify<(#r_args)>>::Notification>>::Handle,
                    >>::Error: ::core::marker::Send + __protocol::derive_deps::Error + 'static
                });
                ref_bounds.push((quote!(#ret), quote!(#r_args)));
                call.extend(quote! {
                    #ident(<C as __protocol::Contextualize>::Handle),
                });
                quote!(let context = self.1.as_ref().unwrap().clone();)
            };

            let sig = &method.sig;

            let logic = if moves {
                quote!(<#ret as __protocol::allocated::Flatten<__protocol::allocated::ProtocolError, __protocol::derive_deps::MoveCoalesce<(#r_args), #ret, __DERIVE_PROTOCOL_TRANSPORT, _, _>>>::flatten(__protocol::derive_deps::MoveCoalesce::new(context, (#bindings), |handle| {
                    __DERIVE_CALL::#ident(handle)
                })))
            } else {
                quote! {
                    <#ret as __protocol::allocated::Flatten<__protocol::allocated::ProtocolError, __protocol::derive_deps::BorrowCoalesce<(#r_args), #ret, __DERIVE_PROTOCOL_TRANSPORT, _, _>>>::flatten(__protocol::derive_deps::BorrowCoalesce::new(context, (#bindings), |handle| {
                        __DERIVE_CALL::#ident(handle)
                    }))
                }
            };

            impl_stream.extend(quote!(#sig {
                #get_context

                #logic
            }));

            if moves {
                delegate_stream.extend(quote!(#sig {
                    (*self).#ident(#bindings)
                }));
            } else {
                delegate_stream.extend(quote!(#sig {
                    (**self).#ident(#bindings)
                }));
            }
        }
    }

    let (context_trait, context_wrapper) = if some_by_ref {
        r_context_bounds.extend(quote!(+ Clone + __protocol::CloneContext));
        d_context_bounds.extend(quote!(+ __protocol::Contextualize));
        c_context_bounds.extend(quote!(+ Clone + __protocol::CloneContext));
        (
            quote!(__protocol::ShareContext),
            quote!(__protocol::JoinContextShared),
        )
    } else {
        (
            quote!(__protocol::CloneContext),
            quote!(__protocol::JoinContextOwned),
        )
    };

    if has_methods {
        r_context_bounds.extend(
            quote! { 
                + __protocol::Write<__DERIVE_CALL<__DERIVE_PROTOCOL_TRANSPORT, #r_generics>>
                + __protocol::FinalizeImmediate<__protocol::derive_deps::Complete<__DERIVE_CALL<__DERIVE_PROTOCOL_TRANSPORT, #r_generics>>>
            }
        );
        where_clause.predicates.push(parse_quote!(<__DERIVE_PROTOCOL_TRANSPORT as __protocol::Write<__DERIVE_CALL<__DERIVE_PROTOCOL_TRANSPORT, #r_generics>>>::Error: __protocol::derive_deps::Error + ::core::marker::Send + 'static));
        where_clause.predicates.push(parse_quote! {
            <__DERIVE_PROTOCOL_TRANSPORT as __protocol::FinalizeImmediate<__protocol::derive_deps::Complete<__DERIVE_CALL<__DERIVE_PROTOCOL_TRANSPORT, #r_generics>>>>::Target: __protocol::Write<__DERIVE_CALL<__DERIVE_PROTOCOL_TRANSPORT, #r_generics>> + ::core::marker::Unpin
        });
        c_context_bounds.extend(
        quote! {
                + __protocol::Write<__DERIVE_CALL<<__DERIVE_PROTOCOL_TRANSPORT as #context_trait>::Context, #r_generics>>
                + __protocol::FinalizeImmediate<__protocol::derive_deps::Complete<__DERIVE_CALL<<__DERIVE_PROTOCOL_TRANSPORT as #context_trait>::Context, #r_generics>>>
            },
        );
        ty_where_clause.predicates.push(parse_quote!(<<__DERIVE_PROTOCOL_TRANSPORT as #context_trait>::Context as __protocol::Write<__DERIVE_CALL<<__DERIVE_PROTOCOL_TRANSPORT as #context_trait>::Context, #r_generics>>>::Error: __protocol::derive_deps::Error + ::core::marker::Send + 'static));
        ty_where_clause.predicates.push(parse_quote! {
            <<__DERIVE_PROTOCOL_TRANSPORT as #context_trait>::Context as __protocol::FinalizeImmediate<__protocol::derive_deps::Complete<__DERIVE_CALL<<__DERIVE_PROTOCOL_TRANSPORT as #context_trait>::Context, #r_generics>>>>::Target: __protocol::Write<__DERIVE_CALL<<__DERIVE_PROTOCOL_TRANSPORT as #context_trait>::Context, #r_generics>> + ::core::marker::Unpin
        });
    }

    for (ret, n_args) in bounds {
        c_context_bounds.extend(quote! {
            + __protocol::Join<#ret>
            + __protocol::Read<<<__DERIVE_PROTOCOL_TRANSPORT as #context_trait>::Context as __protocol::Dispatch<#ret>>::Handle>
            + __protocol::Finalize<<<__DERIVE_PROTOCOL_TRANSPORT as #context_trait>::Context as __protocol::Fork<<<__DERIVE_PROTOCOL_TRANSPORT as #context_trait>::Context as __protocol::Notify<(#n_args)>>::Notification>>::Finalize>
        });
        ty_where_clause.predicates.push(parse_quote!(<<<__DERIVE_PROTOCOL_TRANSPORT as #context_trait>::Context as __protocol::Fork<<<__DERIVE_PROTOCOL_TRANSPORT as #context_trait>::Context as __protocol::Notify<(#n_args)>>::Notification>>::Future as __protocol::Future<<__DERIVE_PROTOCOL_TRANSPORT as #context_trait>::Context>>::Error: __protocol::derive_deps::Error + ::core::marker::Send + 'static));
        ty_where_clause.predicates.push(parse_quote!(<<<__DERIVE_PROTOCOL_TRANSPORT as #context_trait>::Context as __protocol::Notify<(#n_args)>>::Wrap as __protocol::Future<<__DERIVE_PROTOCOL_TRANSPORT as #context_trait>::Context>>::Error: __protocol::derive_deps::Error + ::core::marker::Send + 'static));
        ty_where_clause.predicates.push(parse_quote!(<<__DERIVE_PROTOCOL_TRANSPORT as #context_trait>::Context as __protocol::Read<<<__DERIVE_PROTOCOL_TRANSPORT as #context_trait>::Context as __protocol::Dispatch<#ret>>::Handle>>::Error: __protocol::derive_deps::Error + ::core::marker::Send + 'static));
        ty_where_clause.predicates.push(parse_quote!(<<<__DERIVE_PROTOCOL_TRANSPORT as #context_trait>::Context as __protocol::Join<#ret>>::Future as __protocol::Future<<__DERIVE_PROTOCOL_TRANSPORT as #context_trait>::Context>>::Error: __protocol::derive_deps::Error + ::core::marker::Send + 'static));
        ty_where_clause.predicates.push(parse_quote!(<<<__DERIVE_PROTOCOL_TRANSPORT as #context_trait>::Context as __protocol::Fork<<<__DERIVE_PROTOCOL_TRANSPORT as #context_trait>::Context as __protocol::Notify<(#n_args)>>::Notification>>::Target as __protocol::Future<<__DERIVE_PROTOCOL_TRANSPORT as #context_trait>::Context>>::Error: __protocol::derive_deps::Error + ::core::marker::Send + 'static));
        ty_where_clause.predicates.push(parse_quote!(<<<__DERIVE_PROTOCOL_TRANSPORT as #context_trait>::Context as __protocol::Finalize<<<__DERIVE_PROTOCOL_TRANSPORT as #context_trait>::Context as __protocol::Fork<<<__DERIVE_PROTOCOL_TRANSPORT as #context_trait>::Context as __protocol::Notify<(#n_args)>>::Notification>>::Finalize>>::Output as __protocol::Future<<__DERIVE_PROTOCOL_TRANSPORT as #context_trait>::Context>>::Error: __protocol::derive_deps::Error + ::core::marker::Send + 'static));
        ty_where_clause.predicates.push(parse_quote! {
            <<__DERIVE_PROTOCOL_TRANSPORT as #context_trait>::Context as __protocol::Fork<<<__DERIVE_PROTOCOL_TRANSPORT as #context_trait>::Context as __protocol::Notify<(#n_args)>>::Notification>>::Finalize: __protocol::Future<
                <<__DERIVE_PROTOCOL_TRANSPORT as #context_trait>::Context as __protocol::Finalize<
                    <<__DERIVE_PROTOCOL_TRANSPORT as #context_trait>::Context as __protocol::Fork<<<__DERIVE_PROTOCOL_TRANSPORT as #context_trait>::Context as __protocol::Notify<(#n_args)>>::Notification>>::Finalize,
                >>::Target,
            >
        });
        ty_where_clause.predicates.push(parse_quote! {
            <<__DERIVE_PROTOCOL_TRANSPORT as #context_trait>::Context as __protocol::Finalize<
                <<__DERIVE_PROTOCOL_TRANSPORT as #context_trait>::Context as __protocol::Fork<<<__DERIVE_PROTOCOL_TRANSPORT as #context_trait>::Context as __protocol::Notify<(#n_args)>>::Notification>>::Finalize,
            >>::Output: ::core::marker::Unpin
        });
        ty_where_clause.predicates.push(parse_quote!(<<__DERIVE_PROTOCOL_TRANSPORT as #context_trait>::Context as __protocol::Join<#ret>>::Future: ::core::marker::Unpin));
        ty_where_clause.predicates.push(parse_quote!(<<__DERIVE_PROTOCOL_TRANSPORT as #context_trait>::Context as __protocol::Fork<<<__DERIVE_PROTOCOL_TRANSPORT as #context_trait>::Context as __protocol::Notify<(#n_args)>>::Notification>>::Target: ::core::marker::Unpin));
        ty_where_clause.predicates.push(parse_quote!(<<__DERIVE_PROTOCOL_TRANSPORT as #context_trait>::Context as __protocol::Notify<(#n_args)>>::Wrap: ::core::marker::Unpin));
        ty_where_clause.predicates.push(parse_quote!(<<__DERIVE_PROTOCOL_TRANSPORT as #context_trait>::Context as __protocol::Fork<<<__DERIVE_PROTOCOL_TRANSPORT as #context_trait>::Context as __protocol::Notify<(#n_args)>>::Notification>>::Future: ::core::marker::Unpin));
        ty_where_clause.predicates.push(parse_quote!(#ret: __protocol::allocated::Flatten<__protocol::allocated::ProtocolError, __protocol::derive_deps::MoveCoalesce<(#n_args), #ret, <__DERIVE_PROTOCOL_TRANSPORT as #context_trait>::Context, __DERIVE_CALL<<__DERIVE_PROTOCOL_TRANSPORT as #context_trait>::Context, #r_generics>, fn(<<__DERIVE_PROTOCOL_TRANSPORT as #context_trait>::Context as __protocol::Dispatch<<<__DERIVE_PROTOCOL_TRANSPORT as #context_trait>::Context as __protocol::Notify<(#n_args)>>::Notification>>::Handle) -> __DERIVE_CALL<<__DERIVE_PROTOCOL_TRANSPORT as #context_trait>::Context, #r_generics>>>));
        where_clause.predicates.push(parse_quote!(#ret: __protocol::allocated::Flatten<__protocol::allocated::ProtocolError, __protocol::derive_deps::MoveCoalesce<(#n_args), #ret, __DERIVE_PROTOCOL_TRANSPORT, __DERIVE_CALL<__DERIVE_PROTOCOL_TRANSPORT, #r_generics>, fn(<__DERIVE_PROTOCOL_TRANSPORT as __protocol::Dispatch<<__DERIVE_PROTOCOL_TRANSPORT as __protocol::Notify<(#n_args)>>::Notification>>::Handle) -> __DERIVE_CALL<__DERIVE_PROTOCOL_TRANSPORT, #r_generics>>>));
    }

    for (ret, r_args) in ref_bounds {
        ty_where_clause.predicates.push(parse_quote! {
            <<__DERIVE_PROTOCOL_TRANSPORT as #context_trait>::Context as __protocol::CloneContext>::Context: __protocol::Join<#ret> 
                + __protocol::Notify<(#r_args)>
                + __protocol::Finalize<<<<__DERIVE_PROTOCOL_TRANSPORT as #context_trait>::Context as __protocol::CloneContext>::Context as __protocol::Fork<<<<__DERIVE_PROTOCOL_TRANSPORT as #context_trait>::Context as __protocol::CloneContext>::Context as __protocol::Notify<(#r_args)>>::Notification>>::Finalize>
                + __protocol::Read<<<<__DERIVE_PROTOCOL_TRANSPORT as #context_trait>::Context as __protocol::CloneContext>::Context as __protocol::Dispatch<#ret>>::Handle>
                + __protocol::Write<<<<__DERIVE_PROTOCOL_TRANSPORT as #context_trait>::Context as __protocol::CloneContext>::Context as __protocol::Dispatch<<<<__DERIVE_PROTOCOL_TRANSPORT as #context_trait>::Context as __protocol::CloneContext>::Context as __protocol::Notify<(#r_args)>>::Notification>>::Handle>
        });
        ty_where_clause.predicates.push(parse_quote! {
            <<<<__DERIVE_PROTOCOL_TRANSPORT as #context_trait>::Context as __protocol::CloneContext>::Context as __protocol::Join<
                #ret,
            >>::Future as __protocol::Future<<<__DERIVE_PROTOCOL_TRANSPORT as #context_trait>::Context as __protocol::CloneContext>::Context>>::Error: ::core::marker::Send + __protocol::derive_deps::Error + 'static
        });
        ty_where_clause.predicates.push(parse_quote! {
            <<<__DERIVE_PROTOCOL_TRANSPORT as #context_trait>::Context as __protocol::CloneContext>::ForkOutput as __protocol::Future<<__DERIVE_PROTOCOL_TRANSPORT as #context_trait>::Context>>::Error: ::core::marker::Send + __protocol::derive_deps::Error + 'static
        });
        ty_where_clause.predicates.push(parse_quote! {
            <<__DERIVE_PROTOCOL_TRANSPORT as #context_trait>::Context as __protocol::CloneContext>::ForkOutput: ::core::marker::Unpin
        });
        ty_where_clause.predicates.push(parse_quote! {
            <<<__DERIVE_PROTOCOL_TRANSPORT as #context_trait>::Context as __protocol::CloneContext>::Context as __protocol::Join<
                #ret,
            >>::Future: ::core::marker::Unpin
        });
        ty_where_clause.predicates.push(parse_quote! {
            <<__DERIVE_PROTOCOL_TRANSPORT as #context_trait>::Context as __protocol::CloneContext>::Context: ::core::marker::Unpin
        });
        ty_where_clause.predicates.push(parse_quote! {
            <<<__DERIVE_PROTOCOL_TRANSPORT as #context_trait>::Context as __protocol::CloneContext>::Context as __protocol::Read<
                <<<__DERIVE_PROTOCOL_TRANSPORT as #context_trait>::Context as __protocol::CloneContext>::Context as __protocol::Dispatch<
                    #ret,
                >>::Handle,
            >>::Error: ::core::marker::Send + __protocol::derive_deps::Error + 'static
        });
        ty_where_clause.predicates.push(parse_quote! {
            <<<<__DERIVE_PROTOCOL_TRANSPORT as #context_trait>::Context as __protocol::CloneContext>::Context as __protocol::Fork<
                <<<__DERIVE_PROTOCOL_TRANSPORT as #context_trait>::Context as __protocol::CloneContext>::Context as __protocol::Notify<(#r_args)>>::Notification,
            >>::Future as __protocol::Future<<<__DERIVE_PROTOCOL_TRANSPORT as #context_trait>::Context as __protocol::CloneContext>::Context>>::Error: ::core::marker::Send + __protocol::derive_deps::Error + 'static
        });
        ty_where_clause.predicates.push(parse_quote! {
            <<<<__DERIVE_PROTOCOL_TRANSPORT as #context_trait>::Context as __protocol::CloneContext>::Context as __protocol::Fork<
                <<<__DERIVE_PROTOCOL_TRANSPORT as #context_trait>::Context as __protocol::CloneContext>::Context as __protocol::Notify<(#r_args)>>::Notification,
            >>::Target as __protocol::Future<<<__DERIVE_PROTOCOL_TRANSPORT as #context_trait>::Context as __protocol::CloneContext>::Context>>::Error: ::core::marker::Send + __protocol::derive_deps::Error + 'static
        });
        ty_where_clause.predicates.push(parse_quote! {
            <<<__DERIVE_PROTOCOL_TRANSPORT as #context_trait>::Context as __protocol::CloneContext>::Context as __protocol::Fork<
                <<<__DERIVE_PROTOCOL_TRANSPORT as #context_trait>::Context as __protocol::CloneContext>::Context as __protocol::Notify<(#r_args)>>::Notification,
            >>::Future: ::core::marker::Unpin
        });
        ty_where_clause.predicates.push(parse_quote! {
            <<<__DERIVE_PROTOCOL_TRANSPORT as #context_trait>::Context as __protocol::CloneContext>::Context as __protocol::Fork<
                <<<__DERIVE_PROTOCOL_TRANSPORT as #context_trait>::Context as __protocol::CloneContext>::Context as __protocol::Notify<(#r_args)>>::Notification,
            >>::Target: ::core::marker::Unpin
        });
        ty_where_clause.predicates.push(parse_quote! {
            <<<__DERIVE_PROTOCOL_TRANSPORT as #context_trait>::Context as __protocol::CloneContext>::Context as __protocol::Notify<(#r_args)>>::Wrap: ::core::marker::Unpin
        });
        ty_where_clause.predicates.push(parse_quote! {
            <<<<__DERIVE_PROTOCOL_TRANSPORT as #context_trait>::Context as __protocol::CloneContext>::Context as __protocol::Notify<(#r_args)>>::Wrap as __protocol::Future<<<__DERIVE_PROTOCOL_TRANSPORT as #context_trait>::Context as __protocol::CloneContext>::Context>>::Error: ::core::marker::Send + __protocol::derive_deps::Error + 'static
        });
        ty_where_clause.predicates.push(parse_quote! {
            <<<__DERIVE_PROTOCOL_TRANSPORT as #context_trait>::Context as __protocol::CloneContext>::Context as __protocol::Fork<<<<__DERIVE_PROTOCOL_TRANSPORT as #context_trait>::Context as __protocol::CloneContext>::Context as __protocol::Notify<(#r_args)>>::Notification>>::Finalize: __protocol::Future<
                <<<__DERIVE_PROTOCOL_TRANSPORT as #context_trait>::Context as __protocol::CloneContext>::Context as __protocol::Finalize<
                    <<<__DERIVE_PROTOCOL_TRANSPORT as #context_trait>::Context as __protocol::CloneContext>::Context as __protocol::Fork<<<<__DERIVE_PROTOCOL_TRANSPORT as #context_trait>::Context as __protocol::CloneContext>::Context as __protocol::Notify<(#r_args)>>::Notification>>::Finalize,
                >>::Target,
            >
        });
        ty_where_clause.predicates.push(parse_quote! {
            <<<<__DERIVE_PROTOCOL_TRANSPORT as #context_trait>::Context as __protocol::CloneContext>::Context as __protocol::Finalize<
                <<<__DERIVE_PROTOCOL_TRANSPORT as #context_trait>::Context as __protocol::CloneContext>::Context as __protocol::Fork<
                    <<<__DERIVE_PROTOCOL_TRANSPORT as #context_trait>::Context as __protocol::CloneContext>::Context as __protocol::Notify<(#r_args)>>::Notification,
                >>::Finalize,
            >>::Output as __protocol::Future<<<__DERIVE_PROTOCOL_TRANSPORT as #context_trait>::Context as __protocol::CloneContext>::Context>>::Error: ::core::marker::Send + __protocol::derive_deps::Error + 'static
        });
        ty_where_clause.predicates.push(parse_quote! {
            <<<__DERIVE_PROTOCOL_TRANSPORT as #context_trait>::Context as __protocol::CloneContext>::Context as __protocol::Finalize<
                <<<__DERIVE_PROTOCOL_TRANSPORT as #context_trait>::Context as __protocol::CloneContext>::Context as __protocol::Fork<
                    <<<__DERIVE_PROTOCOL_TRANSPORT as #context_trait>::Context as __protocol::CloneContext>::Context as __protocol::Notify<(#r_args)>>::Notification,
                >>::Finalize,
            >>::Output: ::core::marker::Unpin
        });
        ty_where_clause.predicates.push(parse_quote! {
            <<<__DERIVE_PROTOCOL_TRANSPORT as #context_trait>::Context as __protocol::CloneContext>::Context as __protocol::Write<
                <<<__DERIVE_PROTOCOL_TRANSPORT as #context_trait>::Context as __protocol::CloneContext>::Context as __protocol::Dispatch<<<<__DERIVE_PROTOCOL_TRANSPORT as #context_trait>::Context as __protocol::CloneContext>::Context as __protocol::Notify<(#r_args)>>::Notification>>::Handle,
            >>::Error: ::core::marker::Send + __protocol::derive_deps::Error + 'static
        });
        ty_where_clause.predicates.push(parse_quote!(#ret: __protocol::allocated::Flatten<__protocol::allocated::ProtocolError, __protocol::derive_deps::BorrowCoalesce<(#r_args), #ret, <__DERIVE_PROTOCOL_TRANSPORT as #context_trait>::Context, __DERIVE_CALL<<__DERIVE_PROTOCOL_TRANSPORT as #context_trait>::Context, #r_generics>, fn(<<__DERIVE_PROTOCOL_TRANSPORT as #context_trait>::Context as __protocol::Contextualize>::Handle) -> __DERIVE_CALL<<__DERIVE_PROTOCOL_TRANSPORT as #context_trait>::Context, #r_generics>>>));
        where_clause.predicates.push(parse_quote!(#ret: __protocol::allocated::Flatten<__protocol::allocated::ProtocolError, __protocol::derive_deps::BorrowCoalesce<(#r_args), #ret, __DERIVE_PROTOCOL_TRANSPORT, __DERIVE_CALL<__DERIVE_PROTOCOL_TRANSPORT, #r_generics>, fn(<__DERIVE_PROTOCOL_TRANSPORT as __protocol::Contextualize>::Handle) -> __DERIVE_CALL<__DERIVE_PROTOCOL_TRANSPORT, #r_generics>>>));
    }

    let mut context_binding = quote!();
    let mut c_where = quote!(());

    let mut transport_bounds = quote!();

    let stream = if call.is_empty() {
        quote! {
            type Future = __protocol::future::Ready<Self>;

            fn coalesce() -> Self::Future {
                __protocol::future::ok(Box::new(__DERIVE_COALESCE_SHIM(::core::marker::PhantomData)))
            }
        }
    } else {
        context_binding = quote!(, Option<__DERIVE_PROTOCOL_TRANSPORT>);
        c_where = quote!(__DERIVE_PROTOCOL_TRANSPORT);
        assoc_only = quote!(__DERIVE_PROTOCOL_TRANSPORT,)
            .into_iter()
            .chain(assoc_only.into_iter())
            .collect();
        c.params
            .push(parse_quote!(__DERIVE_PROTOCOL_TRANSPORT: Sized #r_context_bounds));

        transport_bounds.extend(quote!(+ 'derive_lifetime_param + #context_trait + Unpin + __protocol::Read<<__DERIVE_PROTOCOL_TRANSPORT as __protocol::Contextualize>::Handle>));

        ty_where_clause.predicates.push(parse_quote!(
            <__DERIVE_PROTOCOL_TRANSPORT as #context_trait>::JoinOutput: Unpin
        ));

        quote! {
            type Future = #context_wrapper<__DERIVE_PROTOCOL_TRANSPORT, Self, fn(<__DERIVE_PROTOCOL_TRANSPORT as #context_trait>::Context) -> Self>;

            fn coalesce() -> Self::Future {
                #context_wrapper::new(|context| Box::new(__DERIVE_COALESCE_SHIM(::core::marker::PhantomData, Some(context))))
            }
        }
    };

    let (o_impl_generics, _, _) = c.split_for_impl();

    let shim = if assoc_only.is_empty() {
        quote!(#vis struct __DERIVE_COALESCE_SHIM(::core::marker::PhantomData<()>);)
    } else {
        let ewb = if has_methods {
            for param in &item.generics.params {
                if let GenericParam::Type(ty) = param {
                    let ident = &ty.ident;
                    assoc_only.extend(quote!(#ident,));
                }
            }
            quote!( __DERIVE_PROTOCOL_TRANSPORT: __protocol::FinalizeImmediate<__protocol::derive_deps::Complete<__DERIVE_CALL<__DERIVE_PROTOCOL_TRANSPORT, #r_generics>>> #d_context_bounds, <__DERIVE_PROTOCOL_TRANSPORT as __protocol::FinalizeImmediate<__protocol::derive_deps::Complete<__DERIVE_CALL<__DERIVE_PROTOCOL_TRANSPORT, #r_generics>>>>::Target: __protocol::Write<__DERIVE_CALL<__DERIVE_PROTOCOL_TRANSPORT, #r_generics>> + ::core::marker::Unpin)
        } else {
            quote!()
        };

        #[allow(unused_mut)]
        let mut stream = quote! {
            #vis struct __DERIVE_COALESCE_SHIM<#assoc_only>(::core::marker::PhantomData<*const (#assoc_only)> #context_binding) where #ewb;

            impl<#assoc_only> ::core::marker::Unpin for __DERIVE_COALESCE_SHIM<#assoc_only> where #ewb {}
            unsafe impl<#assoc_only> ::core::marker::Sync for __DERIVE_COALESCE_SHIM<#assoc_only> where #c_where: ::core::marker::Sync, #ewb {}
            unsafe impl<#assoc_only> ::core::marker::Send for __DERIVE_COALESCE_SHIM<#assoc_only> where #c_where: ::core::marker::Send, #ewb {}
        };

        #[cfg(feature = "std")]
        {
            stream.extend(quote! {
                impl<#assoc_only> ::std::panic::UnwindSafe for __DERIVE_COALESCE_SHIM<#assoc_only> where #c_where: ::std::panic::UnwindSafe, #ewb {}
                impl<#assoc_only> ::std::panic::RefUnwindSafe for __DERIVE_COALESCE_SHIM<#assoc_only> where #c_where: ::std::panic::RefUnwindSafe, #ewb {}
            })
        }

        stream
    };

    let assoc_only = if assoc_only.is_empty() {
        quote!()
    } else {
        quote!(<#assoc_only>)
    };

    if has_methods {
        ty_where_clause.predicates.push(
            parse_quote!(<__DERIVE_PROTOCOL_TRANSPORT as #context_trait>::Context: Sized #c_context_bounds),
        );
    }
    
    type_item
        .generics
        .params
        .push(parse_quote!('derive_lifetime_param));
    let (p_impl_generics, _, _) = type_item_b.generics.split_for_impl();
    type_item
        .generics
        .params
        .push(parse_quote!(__DERIVE_PROTOCOL_TRANSPORT: ?Sized #transport_bounds));
    for param in &mut type_item.generics.params {
        if let GenericParam::Type(ty) = param {
            ty.bounds.push(parse_quote!('derive_lifetime_param))
        }
    }
    let (impl_generics, _, _) = type_item.generics.split_for_impl();

    let other_shim = if has_methods {
        quote! {
            impl #assoc_only Drop for __DERIVE_COALESCE_SHIM #assoc_only where __DERIVE_PROTOCOL_TRANSPORT: __protocol::FinalizeImmediate<__protocol::derive_deps::Complete<__DERIVE_CALL<__DERIVE_PROTOCOL_TRANSPORT, #r_generics>>> #d_context_bounds, <__DERIVE_PROTOCOL_TRANSPORT as __protocol::FinalizeImmediate<__protocol::derive_deps::Complete<__DERIVE_CALL<__DERIVE_PROTOCOL_TRANSPORT, #r_generics>>>>::Target: __protocol::Write<__DERIVE_CALL<__DERIVE_PROTOCOL_TRANSPORT, #r_generics>> + ::core::marker::Unpin {
                fn drop(&mut self) {
                    if let Some(context) = self.1.as_mut() {
                        __protocol::derive_deps::Complete::complete::<__DERIVE_PROTOCOL_TRANSPORT, _>(context, __DERIVE_CALL::__DERIVE_TERMINATE);
                    }
                }
            }
        }
    } else {
        quote!()
    };

    quote! {
        #shim

        impl #o_impl_generics #ident #o_type_generics for __DERIVE_COALESCE_SHIM #assoc_only #where_clause {
            #impl_stream
        }

        #other_shim

        macro_rules! marker_variants {
            ($(
                $($marker:ident)*
            ),+) => {
                $(
                    impl #impl_generics __protocol::Coalesce<__DERIVE_PROTOCOL_TRANSPORT> for __alloc::boxed::Box<dyn #ident #type_generics + 'derive_lifetime_param $(+ $marker)*> #ty_where_clause {
                        #stream
                    }

                    impl #p_impl_generics #ident #o_type_generics for __alloc::boxed::Box<dyn #ident #type_generics + 'derive_lifetime_param $(+ $marker)*> #p_where_clause {
                        #delegate_stream
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
