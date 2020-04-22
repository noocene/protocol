use super::{desugar_object, rewrite_ty};
use proc_macro2::TokenStream;
use quote::{format_ident, quote, quote_spanned};
use syn::{
    parse_quote, spanned::Spanned, FnArg, GenericParam, ItemTrait, ReturnType, TraitItem,
    TypeParamBound, Index
};

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

            type_item_b.generics.params.push(if bounds.len() > 0 {
                parse_quote!(#ident: #bounds)
            } else {
                parse_quote!(#ident)
            });

            bounds.push(parse_quote!('derive_lifetime_param));

            type_item
                .generics
                .params
                .push(parse_quote!(#ident: #bounds));
        }
    }
    let (_, o_type_generics, where_clause) = item.generics.split_for_impl();
    let mut where_clause = where_clause.cloned().unwrap_or_else(|| parse_quote!(where));

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
    let mut logic = vec![];

    let mut has_methods = false;

    let handles: Box<dyn Fn(TokenStream, TokenStream) -> TokenStream> = if item
        .supertraits
        .is_empty()
    {
        Box::new(|_, transport| quote!(<#transport as __protocol::Contextualize>::Handle))
    } else {
        Box::new(|o_t, transport| {
            let mut stream = quote!(<#transport as __protocol::Contextualize>::Handle);

            for supertrait in &item.supertraits {
                match supertrait {
                    TypeParamBound::Trait(supertrait) => {
                        let path = &supertrait.path;
                        stream.extend(quote!(, <<dyn #path as __protocol::derive_deps::Trait>::Coalesce as __protocol::derive_deps::TraitCoalesce<#o_t>>::Handle));
                    }
                    _ => {}
                }
            }

            stream = quote!((#stream));

            stream
        })
    };

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
                    let span = ty.span();

                    let ty = if let Some(ty) = rewrite_ty(*ty.clone(), &self_ty) {
                        desugar_object(ty)
                    } else {
                        return quote_spanned!(span => const __DERIVE_ERROR: () = { compile_error!(
                            "object-safe traits cannot use the `Self` type"
                        ); };);
                    };
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
                let span = ty.span();
                let pat = *ty.pat.clone();
                bindings.extend(quote!(#pat,));
                let ty = if let Some(ty) = rewrite_ty((*ty.ty).clone(), &self_ty) {
                    ty
                } else {
                    return quote_spanned!(span => const __DERIVE_ERROR: () = { compile_error!(
                        "object-safe traits cannot use the `Self` type"
                    ); };);
                };
                r_args.extend(quote!(#ty,));
            }

            let get_context = if moves {
                for ty in &arg_tys {
                    let span = ty.span();
                    let ident = format_ident!("T{}", format!("{}", ident_idx));
                    let ty = if let Some(ty) = rewrite_ty((*ty.ty).clone(), &self_ty) {
                        ty
                    } else {
                        return quote_spanned!(span => const __DERIVE_ERROR: () = { compile_error!(
                            "object-safe traits cannot use the `Self` type"
                        ); };);
                    };
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
                bounds.push((quote!(#ret), quote!(#n_args)));
                call.extend(quote! {
                    #ident(<C as __protocol::Dispatch<<C as __protocol::Notify<(#args)>>::Notification>>::Handle),
                });
                quote!(let mut this = self; let context = this.1.take().unwrap();)
            } else {
                r_context_bounds.extend(quote!(+ ::core::marker::Unpin));
                c_context_bounds.extend(quote!(+ ::core::marker::Unpin));
                some_by_ref = true;
                ref_bounds.push((quote!(#ret), quote!(#r_args)));
                call.extend(quote! {
                    #ident(<C as __protocol::Contextualize>::Handle),
                });
                quote!(let context = self.1.as_ref().unwrap().clone();)
            };

            let sig = &method.sig;

            logic.push((
                ident.clone(),
                moves,
                get_context,
                ret,
                sig.clone(),
                r_args,
                bindings.clone(),
            ));
        }
    }

    let context_trait = if some_by_ref {
        r_context_bounds.extend(quote!(+ Clone + __protocol::CloneContext));
        d_context_bounds.extend(quote!(+ __protocol::Contextualize));
        c_context_bounds.extend(quote!(+ Clone + __protocol::CloneContext));
        quote!(__protocol::ShareContext)
    } else {
        quote!(__protocol::CloneContext)
    };

    for (ident, moves, get_context, ret, sig, r_args, bindings) in logic {
        let logic = if moves {
            quote!(<#ret as __protocol::allocated::Flatten<__protocol::allocated::ProtocolError, __protocol::derive_deps::MoveCoalesce<(#r_args), #ret, <__DERIVE_PROTOCOL_TRANSPORT as #context_trait>::Context, _, _>>>::flatten(__protocol::derive_deps::MoveCoalesce::new(context, (#bindings), |handle| {
                let a: __DERIVE_CALL_NEWTYPE<__DERIVE_CALL_ALIAS<<__DERIVE_PROTOCOL_TRANSPORT as #context_trait>::Context, #r_generics>> = __DERIVE_CALL::<<__DERIVE_PROTOCOL_TRANSPORT as #context_trait>::Context, #r_generics>::#ident(handle).into();
                a.0
            })))
        } else {
            quote! {
                <#ret as __protocol::allocated::Flatten<__protocol::allocated::ProtocolError, __protocol::derive_deps::BorrowCoalesce<(#r_args), #ret, <__DERIVE_PROTOCOL_TRANSPORT as #context_trait>::Context, _, _>>>::flatten(__protocol::derive_deps::BorrowCoalesce::new(context, (#bindings), |handle| {
                    let a: __DERIVE_CALL_NEWTYPE<__DERIVE_CALL_ALIAS<<__DERIVE_PROTOCOL_TRANSPORT as #context_trait>::Context, #r_generics>> = __DERIVE_CALL::<<__DERIVE_PROTOCOL_TRANSPORT as #context_trait>::Context, #r_generics>::#ident(handle).into();
                    a.0
                }))
            }
        };

        impl_stream.extend(quote!(#sig {
            #get_context

            #logic
        }));
    }

    let mut ewb_ext = quote!();

    let mut supertrait_stream = quote!();

    if has_methods {
        r_context_bounds.extend(
            quote! {
                + __protocol::Write<__DERIVE_CALL_ALIAS<<__DERIVE_PROTOCOL_TRANSPORT as #context_trait>::Context, #r_generics>>
                + __protocol::FinalizeImmediate<__protocol::derive_deps::Complete<__DERIVE_CALL_ALIAS<<__DERIVE_PROTOCOL_TRANSPORT as #context_trait>::Context, #r_generics>>>
            }
        );
        c_context_bounds.extend(
        quote! {
                + __protocol::Write<__DERIVE_CALL_ALIAS<<__DERIVE_PROTOCOL_TRANSPORT as #context_trait>::Context, #r_generics>>
                + __protocol::FinalizeImmediate<__protocol::derive_deps::Complete<__DERIVE_CALL_ALIAS<<__DERIVE_PROTOCOL_TRANSPORT as #context_trait>::Context, #r_generics>>>
            },
        );
        where_clause.predicates.push(parse_quote!(__DERIVE_PROTOCOL_TRANSPORT: #context_trait));
        where_clause.predicates.push(parse_quote!(<<__DERIVE_PROTOCOL_TRANSPORT as #context_trait>::Context as __protocol::Write<__DERIVE_CALL_ALIAS<<__DERIVE_PROTOCOL_TRANSPORT as #context_trait>::Context, #r_generics>>>::Error: __protocol::derive_deps::Error + ::core::marker::Send + 'static));
        where_clause.predicates.push(parse_quote! {
            <<__DERIVE_PROTOCOL_TRANSPORT as #context_trait>::Context as __protocol::FinalizeImmediate<__protocol::derive_deps::Complete<__DERIVE_CALL_ALIAS<<__DERIVE_PROTOCOL_TRANSPORT as #context_trait>::Context, #r_generics>>>>::Target: __protocol::Write<__DERIVE_CALL_ALIAS<<__DERIVE_PROTOCOL_TRANSPORT as #context_trait>::Context, #r_generics>> + ::core::marker::Unpin
        });
    }

    for (ret, n_args) in bounds {
        c_context_bounds.extend(quote! {
            + __protocol::Join<#ret>
            + __protocol::Read<<<__DERIVE_PROTOCOL_TRANSPORT as #context_trait>::Context as __protocol::Dispatch<#ret>>::Handle>
            + __protocol::Finalize<<<__DERIVE_PROTOCOL_TRANSPORT as #context_trait>::Context as __protocol::Fork<<<__DERIVE_PROTOCOL_TRANSPORT as #context_trait>::Context as __protocol::Notify<(#n_args)>>::Notification>>::Finalize>
        });
        where_clause.predicates.push(parse_quote!(<<<__DERIVE_PROTOCOL_TRANSPORT as #context_trait>::Context as __protocol::Fork<<<__DERIVE_PROTOCOL_TRANSPORT as #context_trait>::Context as __protocol::Notify<(#n_args)>>::Notification>>::Future as __protocol::Future<<__DERIVE_PROTOCOL_TRANSPORT as #context_trait>::Context>>::Error: __protocol::derive_deps::Error + ::core::marker::Send + 'static));
        where_clause.predicates.push(parse_quote!(<<<__DERIVE_PROTOCOL_TRANSPORT as #context_trait>::Context as __protocol::Notify<(#n_args)>>::Wrap as __protocol::Future<<__DERIVE_PROTOCOL_TRANSPORT as #context_trait>::Context>>::Error: __protocol::derive_deps::Error + ::core::marker::Send + 'static));
        where_clause.predicates.push(parse_quote!(<<__DERIVE_PROTOCOL_TRANSPORT as #context_trait>::Context as __protocol::Read<<<__DERIVE_PROTOCOL_TRANSPORT as #context_trait>::Context as __protocol::Dispatch<#ret>>::Handle>>::Error: __protocol::derive_deps::Error + ::core::marker::Send + 'static));
        where_clause.predicates.push(parse_quote!(<<<__DERIVE_PROTOCOL_TRANSPORT as #context_trait>::Context as __protocol::Join<#ret>>::Future as __protocol::Future<<__DERIVE_PROTOCOL_TRANSPORT as #context_trait>::Context>>::Error: __protocol::derive_deps::Error + ::core::marker::Send + 'static));
        where_clause.predicates.push(parse_quote!(<<<__DERIVE_PROTOCOL_TRANSPORT as #context_trait>::Context as __protocol::Fork<<<__DERIVE_PROTOCOL_TRANSPORT as #context_trait>::Context as __protocol::Notify<(#n_args)>>::Notification>>::Target as __protocol::Future<<__DERIVE_PROTOCOL_TRANSPORT as #context_trait>::Context>>::Error: __protocol::derive_deps::Error + ::core::marker::Send + 'static));
        where_clause.predicates.push(parse_quote!(<<<__DERIVE_PROTOCOL_TRANSPORT as #context_trait>::Context as __protocol::Finalize<<<__DERIVE_PROTOCOL_TRANSPORT as #context_trait>::Context as __protocol::Fork<<<__DERIVE_PROTOCOL_TRANSPORT as #context_trait>::Context as __protocol::Notify<(#n_args)>>::Notification>>::Finalize>>::Output as __protocol::Future<<__DERIVE_PROTOCOL_TRANSPORT as #context_trait>::Context>>::Error: __protocol::derive_deps::Error + ::core::marker::Send + 'static));
        where_clause.predicates.push(parse_quote! {
            <<__DERIVE_PROTOCOL_TRANSPORT as #context_trait>::Context as __protocol::Fork<<<__DERIVE_PROTOCOL_TRANSPORT as #context_trait>::Context as __protocol::Notify<(#n_args)>>::Notification>>::Finalize: __protocol::Future<
                <<__DERIVE_PROTOCOL_TRANSPORT as #context_trait>::Context as __protocol::Finalize<
                    <<__DERIVE_PROTOCOL_TRANSPORT as #context_trait>::Context as __protocol::Fork<<<__DERIVE_PROTOCOL_TRANSPORT as #context_trait>::Context as __protocol::Notify<(#n_args)>>::Notification>>::Finalize,
                >>::Target,
            >
        });
        ewb_ext.extend(quote! {
            <<__DERIVE_PROTOCOL_TRANSPORT as #context_trait>::Context as __protocol::Fork<<<__DERIVE_PROTOCOL_TRANSPORT as #context_trait>::Context as __protocol::Notify<(#n_args)>>::Notification>>::Finalize: __protocol::Future<
                <<__DERIVE_PROTOCOL_TRANSPORT as #context_trait>::Context as __protocol::Finalize<
                    <<__DERIVE_PROTOCOL_TRANSPORT as #context_trait>::Context as __protocol::Fork<<<__DERIVE_PROTOCOL_TRANSPORT as #context_trait>::Context as __protocol::Notify<(#n_args)>>::Notification>>::Finalize,
                >>::Target,
            >,
        });
        where_clause.predicates.push(parse_quote! {
            <<__DERIVE_PROTOCOL_TRANSPORT as #context_trait>::Context as __protocol::Finalize<
                <<__DERIVE_PROTOCOL_TRANSPORT as #context_trait>::Context as __protocol::Fork<<<__DERIVE_PROTOCOL_TRANSPORT as #context_trait>::Context as __protocol::Notify<(#n_args)>>::Notification>>::Finalize,
            >>::Output: ::core::marker::Unpin
        });
        where_clause.predicates.push(parse_quote!(<<__DERIVE_PROTOCOL_TRANSPORT as #context_trait>::Context as __protocol::Join<#ret>>::Future: ::core::marker::Unpin));
        where_clause.predicates.push(parse_quote!(<<__DERIVE_PROTOCOL_TRANSPORT as #context_trait>::Context as __protocol::Fork<<<__DERIVE_PROTOCOL_TRANSPORT as #context_trait>::Context as __protocol::Notify<(#n_args)>>::Notification>>::Target: ::core::marker::Unpin));
        where_clause.predicates.push(parse_quote!(<<__DERIVE_PROTOCOL_TRANSPORT as #context_trait>::Context as __protocol::Notify<(#n_args)>>::Wrap: ::core::marker::Unpin));
        where_clause.predicates.push(parse_quote!(<<__DERIVE_PROTOCOL_TRANSPORT as #context_trait>::Context as __protocol::Fork<<<__DERIVE_PROTOCOL_TRANSPORT as #context_trait>::Context as __protocol::Notify<(#n_args)>>::Notification>>::Future: ::core::marker::Unpin));
        where_clause.predicates.push(parse_quote!(#ret: __protocol::allocated::Flatten<__protocol::allocated::ProtocolError, __protocol::derive_deps::MoveCoalesce<(#n_args), #ret, <__DERIVE_PROTOCOL_TRANSPORT as #context_trait>::Context, __DERIVE_CALL_ALIAS<<__DERIVE_PROTOCOL_TRANSPORT as #context_trait>::Context, #r_generics>, fn(<<__DERIVE_PROTOCOL_TRANSPORT as #context_trait>::Context as __protocol::Dispatch<<<__DERIVE_PROTOCOL_TRANSPORT as #context_trait>::Context as __protocol::Notify<(#n_args)>>::Notification>>::Handle) -> __DERIVE_CALL_ALIAS<<__DERIVE_PROTOCOL_TRANSPORT as #context_trait>::Context, #r_generics>>>));
       
    }

    for (ret, r_args) in ref_bounds {
        where_clause.predicates.push(parse_quote! {
            <<__DERIVE_PROTOCOL_TRANSPORT as #context_trait>::Context as __protocol::CloneContext>::Context: __protocol::Join<#ret>
                + __protocol::Notify<(#r_args)>
                + __protocol::Finalize<<<<__DERIVE_PROTOCOL_TRANSPORT as #context_trait>::Context as __protocol::CloneContext>::Context as __protocol::Fork<<<<__DERIVE_PROTOCOL_TRANSPORT as #context_trait>::Context as __protocol::CloneContext>::Context as __protocol::Notify<(#r_args)>>::Notification>>::Finalize>
                + __protocol::Read<<<<__DERIVE_PROTOCOL_TRANSPORT as #context_trait>::Context as __protocol::CloneContext>::Context as __protocol::Dispatch<#ret>>::Handle>
                + __protocol::Write<<<<__DERIVE_PROTOCOL_TRANSPORT as #context_trait>::Context as __protocol::CloneContext>::Context as __protocol::Dispatch<<<<__DERIVE_PROTOCOL_TRANSPORT as #context_trait>::Context as __protocol::CloneContext>::Context as __protocol::Notify<(#r_args)>>::Notification>>::Handle>
        });
        where_clause.predicates.push(parse_quote! {
            <<<<__DERIVE_PROTOCOL_TRANSPORT as #context_trait>::Context as __protocol::CloneContext>::Context as __protocol::Join<
                #ret,
            >>::Future as __protocol::Future<<<__DERIVE_PROTOCOL_TRANSPORT as #context_trait>::Context as __protocol::CloneContext>::Context>>::Error: ::core::marker::Send + __protocol::derive_deps::Error + 'static
        });
        where_clause.predicates.push(parse_quote! {
            <<<__DERIVE_PROTOCOL_TRANSPORT as #context_trait>::Context as __protocol::CloneContext>::ForkOutput as __protocol::Future<<__DERIVE_PROTOCOL_TRANSPORT as #context_trait>::Context>>::Error: ::core::marker::Send + __protocol::derive_deps::Error + 'static
        });
        where_clause.predicates.push(parse_quote! {
            <<__DERIVE_PROTOCOL_TRANSPORT as #context_trait>::Context as __protocol::CloneContext>::ForkOutput: ::core::marker::Unpin
        });
        where_clause.predicates.push(parse_quote! {
            <<<__DERIVE_PROTOCOL_TRANSPORT as #context_trait>::Context as __protocol::CloneContext>::Context as __protocol::Join<
                #ret,
            >>::Future: ::core::marker::Unpin
        });
        where_clause.predicates.push(parse_quote! {
            <<__DERIVE_PROTOCOL_TRANSPORT as #context_trait>::Context as __protocol::CloneContext>::Context: ::core::marker::Unpin
        });
        where_clause.predicates.push(parse_quote! {
            <<<__DERIVE_PROTOCOL_TRANSPORT as #context_trait>::Context as __protocol::CloneContext>::Context as __protocol::Read<
                <<<__DERIVE_PROTOCOL_TRANSPORT as #context_trait>::Context as __protocol::CloneContext>::Context as __protocol::Dispatch<
                    #ret,
                >>::Handle,
            >>::Error: ::core::marker::Send + __protocol::derive_deps::Error + 'static
        });
        where_clause.predicates.push(parse_quote! {
            <<<<__DERIVE_PROTOCOL_TRANSPORT as #context_trait>::Context as __protocol::CloneContext>::Context as __protocol::Fork<
                <<<__DERIVE_PROTOCOL_TRANSPORT as #context_trait>::Context as __protocol::CloneContext>::Context as __protocol::Notify<(#r_args)>>::Notification,
            >>::Future as __protocol::Future<<<__DERIVE_PROTOCOL_TRANSPORT as #context_trait>::Context as __protocol::CloneContext>::Context>>::Error: ::core::marker::Send + __protocol::derive_deps::Error + 'static
        });
        where_clause.predicates.push(parse_quote! {
            <<<<__DERIVE_PROTOCOL_TRANSPORT as #context_trait>::Context as __protocol::CloneContext>::Context as __protocol::Fork<
                <<<__DERIVE_PROTOCOL_TRANSPORT as #context_trait>::Context as __protocol::CloneContext>::Context as __protocol::Notify<(#r_args)>>::Notification,
            >>::Target as __protocol::Future<<<__DERIVE_PROTOCOL_TRANSPORT as #context_trait>::Context as __protocol::CloneContext>::Context>>::Error: ::core::marker::Send + __protocol::derive_deps::Error + 'static
        });
        where_clause.predicates.push(parse_quote! {
            <<<__DERIVE_PROTOCOL_TRANSPORT as #context_trait>::Context as __protocol::CloneContext>::Context as __protocol::Fork<
                <<<__DERIVE_PROTOCOL_TRANSPORT as #context_trait>::Context as __protocol::CloneContext>::Context as __protocol::Notify<(#r_args)>>::Notification,
            >>::Future: ::core::marker::Unpin
        });
        where_clause.predicates.push(parse_quote! {
            <<<__DERIVE_PROTOCOL_TRANSPORT as #context_trait>::Context as __protocol::CloneContext>::Context as __protocol::Fork<
                <<<__DERIVE_PROTOCOL_TRANSPORT as #context_trait>::Context as __protocol::CloneContext>::Context as __protocol::Notify<(#r_args)>>::Notification,
            >>::Target: ::core::marker::Unpin
        });
        where_clause.predicates.push(parse_quote! {
            <<<__DERIVE_PROTOCOL_TRANSPORT as #context_trait>::Context as __protocol::CloneContext>::Context as __protocol::Notify<(#r_args)>>::Wrap: ::core::marker::Unpin
        });
        where_clause.predicates.push(parse_quote! {
            <<<<__DERIVE_PROTOCOL_TRANSPORT as #context_trait>::Context as __protocol::CloneContext>::Context as __protocol::Notify<(#r_args)>>::Wrap as __protocol::Future<<<__DERIVE_PROTOCOL_TRANSPORT as #context_trait>::Context as __protocol::CloneContext>::Context>>::Error: ::core::marker::Send + __protocol::derive_deps::Error + 'static
        });
        where_clause.predicates.push(parse_quote! {
            <<<__DERIVE_PROTOCOL_TRANSPORT as #context_trait>::Context as __protocol::CloneContext>::Context as __protocol::Fork<<<<__DERIVE_PROTOCOL_TRANSPORT as #context_trait>::Context as __protocol::CloneContext>::Context as __protocol::Notify<(#r_args)>>::Notification>>::Finalize: __protocol::Future<
                <<<__DERIVE_PROTOCOL_TRANSPORT as #context_trait>::Context as __protocol::CloneContext>::Context as __protocol::Finalize<
                    <<<__DERIVE_PROTOCOL_TRANSPORT as #context_trait>::Context as __protocol::CloneContext>::Context as __protocol::Fork<<<<__DERIVE_PROTOCOL_TRANSPORT as #context_trait>::Context as __protocol::CloneContext>::Context as __protocol::Notify<(#r_args)>>::Notification>>::Finalize,
                >>::Target,
            >
        });
        where_clause.predicates.push(parse_quote! {
            <<<<__DERIVE_PROTOCOL_TRANSPORT as #context_trait>::Context as __protocol::CloneContext>::Context as __protocol::Finalize<
                <<<__DERIVE_PROTOCOL_TRANSPORT as #context_trait>::Context as __protocol::CloneContext>::Context as __protocol::Fork<
                    <<<__DERIVE_PROTOCOL_TRANSPORT as #context_trait>::Context as __protocol::CloneContext>::Context as __protocol::Notify<(#r_args)>>::Notification,
                >>::Finalize,
            >>::Output as __protocol::Future<<<__DERIVE_PROTOCOL_TRANSPORT as #context_trait>::Context as __protocol::CloneContext>::Context>>::Error: ::core::marker::Send + __protocol::derive_deps::Error + 'static
        });
        where_clause.predicates.push(parse_quote! {
            <<<__DERIVE_PROTOCOL_TRANSPORT as #context_trait>::Context as __protocol::CloneContext>::Context as __protocol::Finalize<
                <<<__DERIVE_PROTOCOL_TRANSPORT as #context_trait>::Context as __protocol::CloneContext>::Context as __protocol::Fork<
                    <<<__DERIVE_PROTOCOL_TRANSPORT as #context_trait>::Context as __protocol::CloneContext>::Context as __protocol::Notify<(#r_args)>>::Notification,
                >>::Finalize,
            >>::Output: ::core::marker::Unpin
        });
        where_clause.predicates.push(parse_quote! {
            <<<__DERIVE_PROTOCOL_TRANSPORT as #context_trait>::Context as __protocol::CloneContext>::Context as __protocol::Write<
                <<<__DERIVE_PROTOCOL_TRANSPORT as #context_trait>::Context as __protocol::CloneContext>::Context as __protocol::Dispatch<<<<__DERIVE_PROTOCOL_TRANSPORT as #context_trait>::Context as __protocol::CloneContext>::Context as __protocol::Notify<(#r_args)>>::Notification>>::Handle,
            >>::Error: ::core::marker::Send + __protocol::derive_deps::Error + 'static
        });
        where_clause.predicates.push(parse_quote!(#ret: __protocol::allocated::Flatten<__protocol::allocated::ProtocolError, __protocol::derive_deps::BorrowCoalesce<(#r_args), #ret, <__DERIVE_PROTOCOL_TRANSPORT as #context_trait>::Context, __DERIVE_CALL_ALIAS<<__DERIVE_PROTOCOL_TRANSPORT as #context_trait>::Context, #r_generics>, fn(<<__DERIVE_PROTOCOL_TRANSPORT as #context_trait>::Context as __protocol::Contextualize>::Handle) -> __DERIVE_CALL_ALIAS<<__DERIVE_PROTOCOL_TRANSPORT as #context_trait>::Context, #r_generics>>>));
    }

    let mut context_binding = quote!();
    let mut c_where = quote!(());

    let mut transport_bounds = quote!();

    let j_call = if some_by_ref {
        quote!(join_shared)
    } else {
        quote!(join_owned)
    };

    let mut k_assoc_only;

    let stream = if call.is_empty() {
        k_assoc_only = assoc_only.clone();
        quote! {
            type Future = __protocol::future::Ready<Self>;

            fn coalesce() -> Self::Future {
                __protocol::future::ok(Box::new(__DERIVE_COALESCE_SHIM(::core::marker::PhantomData)))
            }
        }
    } else {
        context_binding = quote!(, Option<<__DERIVE_PROTOCOL_TRANSPORT as #context_trait>::Context>);
        c_where = quote!(<__DERIVE_PROTOCOL_TRANSPORT as #context_trait>::Context: Sized #c_context_bounds, <__DERIVE_PROTOCOL_TRANSPORT as #context_trait>::Context);
        c.params
            .push(parse_quote!(__DERIVE_PROTOCOL_TRANSPORT));

        let handle = (handles)(
            quote!(__DERIVE_PROTOCOL_TRANSPORT),
            quote!(__DERIVE_PROTOCOL_TRANSPORT),
        );

        transport_bounds.extend(
            quote!(+ 'derive_lifetime_param + #context_trait + Unpin + __protocol::Read<#handle>),
        );

        where_clause.predicates.push(parse_quote!(
            <__DERIVE_PROTOCOL_TRANSPORT as #context_trait>::JoinOutput: Unpin
        ));

        let (context_wrapper, context_construct) = if some_by_ref {
            let mut assoc_only = assoc_only.clone();
            for param in &item.generics.params {
                if let GenericParam::Type(ty) = param {
                    let ident = &ty.ident;
                    assoc_only.extend(quote!(#ident,));
                }
            }
            (
                quote!(__protocol::JoinContextShared<__DERIVE_PROTOCOL_TRANSPORT, Self, fn(<__DERIVE_PROTOCOL_TRANSPORT as __protocol::ShareContext>::Context) -> Self>),
                quote!(__protocol::JoinContextShared::new(|context| Box::new(
                    __DERIVE_COALESCE_SHIM::<__DERIVE_PROTOCOL_TRANSPORT, #assoc_only>(::core::marker::PhantomData, Some(context))
                ))),
            )
        } else {
            let mut assoc_only = assoc_only.clone();
            for param in &item.generics.params {
                if let GenericParam::Type(ty) = param {
                    let ident = &ty.ident;
                    assoc_only.extend(quote!(#ident,));
                }
            }
            (
                quote!(
                    __protocol::JoinContextOwned<__DERIVE_PROTOCOL_TRANSPORT, Self, fn(<__DERIVE_PROTOCOL_TRANSPORT as __protocol::CloneContext>::Context) -> Self>
                ),
                quote!(__protocol::JoinContextOwned::new(|context| Box::new(
                    __DERIVE_COALESCE_SHIM::<__DERIVE_PROTOCOL_TRANSPORT, #assoc_only>(::core::marker::PhantomData, Some(context))
                ))),
            )
        };

        let (context_wrapper, context_construct) = if item.supertraits.is_empty() {
            (context_wrapper, context_construct)
        } else {
            let handle = (handles)(
                quote!(__DERIVE_PROTOCOL_TRANSPORT),
                quote!(__DERIVE_PROTOCOL_TRANSPORT),
            );
            
            let mut assoc_only = assoc_only.clone();

            for param in &item.generics.params {
                if let GenericParam::Type(ty) = param {
                    let ident = &ty.ident;
                    assoc_only.extend(quote!(#ident,));
                }
            }

            let a_assoc_only = assoc_only.clone();

            assoc_only.extend(quote!(__DERIVE_PROTOCOL_TRANSPORT));

            let mut decomp = quote!(__derive_0_handle);

            let mut construct_stream = quote!();
            let mut variants = quote!();
            let mut arms = quote!();

            let mut b_tuple = quote!(<__DERIVE_PROTOCOL_TRANSPORT as __protocol::Contextualize>::Handle,);

            let mut first_path = None;

            let mut last_idx = 0;

            for (idx, supertrait) in item.supertraits.iter().filter_map(|item| if let TypeParamBound::Trait(supertrait) = item { Some(supertrait) } else { None }).enumerate() {
                let ident = format_ident!("__derive_{}_handle", idx + 1);
                let next_ident = format_ident!("__derive_{}_handle", idx + 2);
                let v_ident = format_ident!("State{}", idx);
                let v_next_ident = format_ident!("State{}", idx + 1);
                decomp.extend(quote!(, #ident));
                let path = &supertrait.path;

                if idx == 0 {
                    first_path = Some(path);
                }

                if idx != 0 {
                    construct_stream.extend(quote! {
                        #ident,
                    });
                } else {
                    construct_stream.extend(quote! {
                        <dyn #path as __protocol::derive_deps::Trait>::coalesce::<__DERIVE_PROTOCOL_TRANSPORT>(#ident),
                    });
                }
                b_tuple.extend(quote!(<<dyn #path as __protocol::derive_deps::Trait>::Coalesce as __protocol::derive_deps::TraitCoalesce<__DERIVE_PROTOCOL_TRANSPORT>>::Handle,));
                context_binding.extend(quote!(, Option<<<dyn #path as __protocol::derive_deps::Trait>::Coalesce as __protocol::derive_deps::TraitCoalesce<__DERIVE_PROTOCOL_TRANSPORT>>::Coalesce>));
                let mut bindings = quote!(__derive_0_handle,);
                let mut c = quote!(<__DERIVE_PROTOCOL_TRANSPORT as __protocol::Contextualize>::Handle,);
                let mut lt = quote!();
                let mut gt = quote!();
                for (idx_1, supertrait_1) in item.supertraits.iter().filter_map(|item| if let TypeParamBound::Trait(supertrait) = item { Some(supertrait) } else { None }).enumerate() {
                    let path = &supertrait_1.path;
                    let ident = format_ident!("__derive_{}_handle", idx_1 + 1);
                    bindings.extend(quote!(#ident,));
                    if idx_1 < idx {
                        lt.extend(quote!(#ident,));
                        c.extend(quote!(<<dyn #path as __protocol::derive_deps::Trait>::Coalesce as __protocol::derive_deps::TraitCoalesce<__DERIVE_PROTOCOL_TRANSPORT>>::Coalesce,));
                    } else if idx_1 == idx {
                        c.extend(quote!(<<dyn #path as __protocol::derive_deps::Trait>::Coalesce as __protocol::derive_deps::TraitCoalesce<__DERIVE_PROTOCOL_TRANSPORT>>::Future,));
                    } else {
                        if idx_1 -1 != idx { gt.extend(quote!(#ident,)); }
                        c.extend(quote!(<<dyn #path as __protocol::derive_deps::Trait>::Coalesce as __protocol::derive_deps::TraitCoalesce<__DERIVE_PROTOCOL_TRANSPORT>>::Handle,));
                    }
                }

                variants.extend(quote! {
                    #v_ident(#c),
                });
                let next_path = item.supertraits.iter().skip(idx + 1).filter_map(|item| if let TypeParamBound::Trait(supertrait) = item { Some(supertrait) } else { None }).next();

                if let Some(next_path) = next_path {
                    arms.extend(quote! {
                        __DERIVE_COALESCE_READER::#v_ident(#bindings) => {
                            let coalesce = __protocol::derive_deps::ready!(<<<dyn #path as __protocol::derive_deps::Trait>::Coalesce as __protocol::derive_deps::TraitCoalesce<__DERIVE_PROTOCOL_TRANSPORT>>::Future as __protocol::Future<__DERIVE_PROTOCOL_TRANSPORT>>::poll(::core::pin::Pin::new(#ident), cx, <__DERIVE_R as ::core::borrow::BorrowMut<__DERIVE_PROTOCOL_TRANSPORT>>::borrow_mut(&mut ctx))).unwrap_or_else(|_| panic!());
                            let data = ::core::mem::replace(this, __DERIVE_COALESCE_READER::Read(::core::marker::PhantomData));
                            if let __DERIVE_COALESCE_READER::#v_ident(#bindings) = data {
                                *this = __DERIVE_COALESCE_READER::#v_next_ident(__derive_0_handle, #lt coalesce, <dyn #next_path as __protocol::derive_deps::Trait>::coalesce::<__DERIVE_PROTOCOL_TRANSPORT>(#next_ident), #gt);
                            } else {
                                panic!("invalid state")
                            }
                        }
                    });
                } else {
                    arms.extend(quote! {
                        __DERIVE_COALESCE_READER::#v_ident(#bindings) => {
                            let coalesce = __protocol::derive_deps::ready!(<<<dyn #path as __protocol::derive_deps::Trait>::Coalesce as __protocol::derive_deps::TraitCoalesce<__DERIVE_PROTOCOL_TRANSPORT>>::Future as __protocol::Future<__DERIVE_PROTOCOL_TRANSPORT>>::poll(::core::pin::Pin::new(#ident), cx, <__DERIVE_R as ::core::borrow::BorrowMut<__DERIVE_PROTOCOL_TRANSPORT>>::borrow_mut(&mut ctx))).unwrap_or_else(|_| panic!());
                            let data = ::core::mem::replace(this, __DERIVE_COALESCE_READER::Read(::core::marker::PhantomData));
                            if let __DERIVE_COALESCE_READER::#v_ident(#bindings) = data {
                                *this = __DERIVE_COALESCE_READER::LastState(#lt coalesce, ctx.borrow_mut().#j_call(__derive_0_handle));
                            } else {
                                panic!("invalid state")
                            }
                        }
                    });
                }
                last_idx = idx;
            }

            let first_path = first_path.unwrap();

            construct_stream = quote! {
                *this = __DERIVE_COALESCE_READER::State0(__derive_0_handle, #construct_stream);                
            };

            let n_decomp = decomp.clone();

            decomp = quote!((#decomp));

            let last_ident = format_ident!("__derive_{}_handle", last_idx + 1);

            let mut ct_stream = quote!(Some(context),);

            for i in 0..=last_idx {
                let ident = format_ident!("__derive_{}_handle", i);
                ct_stream.extend(quote!(Some(#ident),));
            }

            let mut c = quote!();
            for supertrait_1 in item.supertraits.iter().filter_map(|item| if let TypeParamBound::Trait(supertrait) = item { Some(supertrait) } else { None }) {
                let path = &supertrait_1.path;
                c.extend(quote!(<<dyn #path as __protocol::derive_deps::Trait>::Coalesce as __protocol::derive_deps::TraitCoalesce<__DERIVE_PROTOCOL_TRANSPORT>>::Coalesce,));
            }
    
            supertrait_stream.extend(quote! {
                #vis enum __DERIVE_COALESCE_READER<#assoc_only> where __DERIVE_PROTOCOL_TRANSPORT: ?Sized + #context_trait {
                    Read(::core::marker::PhantomData<(#assoc_only)>,),
                    #variants
                    LastState(#c <__DERIVE_PROTOCOL_TRANSPORT as #context_trait>::JoinOutput)
                }
    
                impl<#assoc_only> __DERIVE_COALESCE_READER<#assoc_only> where __DERIVE_PROTOCOL_TRANSPORT: ?Sized + #context_trait {
                    fn new() -> Self {
                        __DERIVE_COALESCE_READER::Read(::core::marker::PhantomData)
                    }

                    fn new_from_handles(data: (#b_tuple)) -> Self {
                        let #decomp = data;
                        let __derive_1_handle = <dyn #first_path as __protocol::derive_deps::Trait>::coalesce::<__DERIVE_PROTOCOL_TRANSPORT>(__derive_1_handle);
                        __DERIVE_COALESCE_READER::State0(#n_decomp)
                    }
                }

                impl<#assoc_only> Unpin for __DERIVE_COALESCE_READER<#assoc_only> where __DERIVE_PROTOCOL_TRANSPORT: ?Sized + #context_trait {}
    
                impl<#assoc_only> __protocol::Future<__DERIVE_PROTOCOL_TRANSPORT> for __DERIVE_COALESCE_READER<#assoc_only> where __DERIVE_PROTOCOL_TRANSPORT: ::core::marker::Unpin + ?Sized + #context_trait + __protocol::Read<#handle>, <__DERIVE_PROTOCOL_TRANSPORT as #context_trait>::JoinOutput: Unpin,
                    <__DERIVE_PROTOCOL_TRANSPORT as #context_trait>::Context: __protocol::FinalizeImmediate<__protocol::derive_deps::Complete<__DERIVE_CALL_ALIAS<<__DERIVE_PROTOCOL_TRANSPORT as #context_trait>::Context, #r_generics>>> #d_context_bounds,
                    <<__DERIVE_PROTOCOL_TRANSPORT as #context_trait>::Context as __protocol::FinalizeImmediate<__protocol::derive_deps::Complete<__DERIVE_CALL_ALIAS<<__DERIVE_PROTOCOL_TRANSPORT as #context_trait>::Context, #r_generics>>>>::Target: ::core::marker::Unpin + __protocol::Write<__DERIVE_CALL_ALIAS<<__DERIVE_PROTOCOL_TRANSPORT as #context_trait>::Context, #r_generics>>
                {
                    type Ok = __DERIVE_COALESCE_SHIM<__DERIVE_PROTOCOL_TRANSPORT, #a_assoc_only>;
                    type Error = ();

                    fn poll<__DERIVE_R: ::core::borrow::BorrowMut<__DERIVE_PROTOCOL_TRANSPORT>>(mut self: ::core::pin::Pin<&mut Self>, cx: &mut ::core::task::Context<'_>, mut ctx: __DERIVE_R) -> ::core::task::Poll<::core::result::Result<<Self as __protocol::Future<__DERIVE_PROTOCOL_TRANSPORT>>::Ok, <Self as __protocol::Future<__DERIVE_PROTOCOL_TRANSPORT>>::Error>> {
                        let this = &mut *self;
                        loop {
                            match this {
                                __DERIVE_COALESCE_READER::Read(_) => {
                                    let #decomp = __protocol::derive_deps::ready!(::core::pin::Pin::new(<__DERIVE_R as ::core::borrow::BorrowMut<__DERIVE_PROTOCOL_TRANSPORT>>::borrow_mut(&mut ctx)).read(cx)).unwrap_or_else(|_| panic!());
                                    #construct_stream
                                }
                                #arms
                                __DERIVE_COALESCE_READER::LastState(#n_decomp) => {
                                    let context = __protocol::derive_deps::ready!(::core::pin::Pin::new(#last_ident).poll(cx, ctx.borrow_mut())).unwrap_or_else(|_| panic!());
                                    let data = ::core::mem::replace(this, __DERIVE_COALESCE_READER::Read(::core::marker::PhantomData));
                                    if let __DERIVE_COALESCE_READER::LastState(#n_decomp) = data {
                                        return ::core::task::Poll::Ready(Ok(__DERIVE_COALESCE_SHIM(::core::marker::PhantomData, #ct_stream)));
                                    } else {
                                        panic!("invalid state")
                                    }
                                }
                            }
                        }
                    }
                }
            });
    
            (
                quote!(__protocol::future::MapOk<__DERIVE_COALESCE_READER<#assoc_only>, fn(__DERIVE_COALESCE_SHIM<__DERIVE_PROTOCOL_TRANSPORT, #a_assoc_only>) -> Self>),
                quote!(__protocol::FutureExt::map_ok(__DERIVE_COALESCE_READER::new(), |shim| Box::new(shim))),
            )
        };

        k_assoc_only = assoc_only.clone();

        assoc_only = quote!(__DERIVE_PROTOCOL_TRANSPORT,)
            .into_iter()
            .chain(assoc_only.into_iter())
            .collect();

        quote! {
            type Future = #context_wrapper;

            fn coalesce() -> Self::Future {
                #context_construct
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
                    k_assoc_only.extend(quote!(#ident,));
                }
            }
            let mut data = quote!(__DERIVE_PROTOCOL_TRANSPORT: ?Sized + #context_trait, <__DERIVE_PROTOCOL_TRANSPORT as #context_trait>::Context: __protocol::FinalizeImmediate<__protocol::derive_deps::Complete<__DERIVE_CALL_ALIAS<<__DERIVE_PROTOCOL_TRANSPORT as #context_trait>::Context, #r_generics>>> #d_context_bounds, <<__DERIVE_PROTOCOL_TRANSPORT as #context_trait>::Context as __protocol::FinalizeImmediate<__protocol::derive_deps::Complete<__DERIVE_CALL_ALIAS<<__DERIVE_PROTOCOL_TRANSPORT as #context_trait>::Context, #r_generics>>>>::Target: __protocol::Write<__DERIVE_CALL_ALIAS<<__DERIVE_PROTOCOL_TRANSPORT as #context_trait>::Context, #r_generics>> + ::core::marker::Unpin,);
            for supertrait in &item.supertraits {
                if let TypeParamBound::Trait(supertrait) = supertrait {
                    let path = &supertrait.path;
                    data.extend(quote! {
                        <dyn #path as __protocol::derive_deps::Trait>::Coalesce: __protocol::derive_deps::TraitCoalesce<__DERIVE_PROTOCOL_TRANSPORT>,
                    });
                }
            }
            data
        } else {
            quote!()
        };

        #[allow(unused_mut)]
        let mut stream = quote! {
            #vis struct __DERIVE_COALESCE_SHIM<#assoc_only>(::core::marker::PhantomData<*const (#k_assoc_only __DERIVE_PROTOCOL_TRANSPORT)> #context_binding) where #ewb;

            impl<#assoc_only> ::core::marker::Unpin for __DERIVE_COALESCE_SHIM<#assoc_only> where #c_where: Sized, #ewb #ewb_ext {}
            unsafe impl<#assoc_only> ::core::marker::Sync for __DERIVE_COALESCE_SHIM<#assoc_only> where #c_where: ::core::marker::Sync, #ewb #ewb_ext {}
            unsafe impl<#assoc_only> ::core::marker::Send for __DERIVE_COALESCE_SHIM<#assoc_only> where #c_where: ::core::marker::Send, #ewb #ewb_ext {}
        };

        #[cfg(feature = "std")]
        {
            stream.extend(quote! {
                impl<#assoc_only> ::std::panic::UnwindSafe for __DERIVE_COALESCE_SHIM<#assoc_only> where #c_where: ::std::panic::UnwindSafe, #ewb #ewb_ext {}
                impl<#assoc_only> ::std::panic::RefUnwindSafe for __DERIVE_COALESCE_SHIM<#assoc_only> where #c_where: ::std::panic::RefUnwindSafe, #ewb #ewb_ext {}
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
        where_clause.predicates.push(
            parse_quote!(<__DERIVE_PROTOCOL_TRANSPORT as #context_trait>::Context: Sized #c_context_bounds),
        );
    }

    type_item
        .generics
        .params
        .push(parse_quote!('derive_lifetime_param));
    type_item_b
        .generics
        .params
        .push(parse_quote!(__DERIVE_DISPATCH_TY: #ident #type_generics + ?Sized));
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

    let mut st_bounds = quote!();

    for supertrait in &item.supertraits {
        if let TypeParamBound::Trait(supertrait) = supertrait {
            let path = &supertrait.path;
            st_bounds.extend(quote! {
                , <dyn #path as __protocol::derive_deps::Trait>::Coalesce: __protocol::derive_deps::TraitCoalesce<__DERIVE_PROTOCOL_TRANSPORT>
            });
        }
    }

    let other_shim = if has_methods {
        quote! {
            impl #assoc_only Drop for __DERIVE_COALESCE_SHIM #assoc_only where __DERIVE_PROTOCOL_TRANSPORT: ?Sized + #context_trait, <__DERIVE_PROTOCOL_TRANSPORT as #context_trait>::Context: __protocol::FinalizeImmediate<__protocol::derive_deps::Complete<__DERIVE_CALL_ALIAS<<__DERIVE_PROTOCOL_TRANSPORT as #context_trait>::Context, #r_generics>>> #d_context_bounds, <<__DERIVE_PROTOCOL_TRANSPORT as #context_trait>::Context as __protocol::FinalizeImmediate<__protocol::derive_deps::Complete<__DERIVE_CALL_ALIAS<<__DERIVE_PROTOCOL_TRANSPORT as #context_trait>::Context, #r_generics>>>>::Target: __protocol::Write<__DERIVE_CALL_ALIAS<<__DERIVE_PROTOCOL_TRANSPORT as #context_trait>::Context, #r_generics>> + ::core::marker::Unpin #st_bounds {
                fn drop(&mut self) {
                    if let Some(context) = self.1.as_mut() {
                        let a: __DERIVE_CALL_NEWTYPE<__DERIVE_CALL_ALIAS<<__DERIVE_PROTOCOL_TRANSPORT as #context_trait>::Context, #r_generics>> = __DERIVE_CALL::<<__DERIVE_PROTOCOL_TRANSPORT as #context_trait>::Context, #r_generics>::__DERIVE_TERMINATE.into();
                        __protocol::derive_deps::Complete::complete::<<__DERIVE_PROTOCOL_TRANSPORT as #context_trait>::Context, _>(context, a.0);
                    }
                }
            }
        }
    } else {
        quote!()
    };

    let m_bound = if has_methods {
        quote!(, <__DERIVE_PROTOCOL_TRANSPORT as #context_trait>::Context: Sized $(+ $marker)*)
    } else {
        quote!()
    };

    let mut idx = 2;

    for supertrait in &item.supertraits {
        match supertrait {
            TypeParamBound::Trait(supertrait) => {
                let path = &supertrait.path;
                let t_index = Index::from(idx);
                supertrait_stream.extend(quote! {
                    impl #assoc_only __protocol::derive_deps::ObjectRef<dyn #path> for __DERIVE_COALESCE_SHIM #assoc_only 
                    where
                        __DERIVE_PROTOCOL_TRANSPORT: #context_trait,
                        <__DERIVE_PROTOCOL_TRANSPORT as #context_trait>::Context: __protocol::FinalizeImmediate<__protocol::derive_deps::Complete<__DERIVE_CALL_ALIAS<<__DERIVE_PROTOCOL_TRANSPORT as #context_trait>::Context, #r_generics>>> #d_context_bounds,
                        <<__DERIVE_PROTOCOL_TRANSPORT as #context_trait>::Context as __protocol::FinalizeImmediate<__protocol::derive_deps::Complete<__DERIVE_CALL_ALIAS<<__DERIVE_PROTOCOL_TRANSPORT as #context_trait>::Context, #r_generics>>>>::Target: __protocol::Write<__DERIVE_CALL_ALIAS<<__DERIVE_PROTOCOL_TRANSPORT as #context_trait>::Context, #r_generics>> + ::core::marker::Unpin,
                        for<'a> __protocol::derive_deps::ObjectWrapperRef<'a, dyn #path, <<dyn #path as __protocol::derive_deps::Trait>::Coalesce as __protocol::derive_deps::TraitCoalesce<__DERIVE_PROTOCOL_TRANSPORT>>::Coalesce>: __protocol::derive_deps::ObjectRef<dyn #path>,
                        dyn #path: __protocol::derive_deps::Trait #st_bounds
                    {
                        fn call_ref(&self, args: <dyn #path as __protocol::derive_deps::Trait>::RefArgs) -> <dyn #path as __protocol::derive_deps::Trait>::RefRets {
                            __protocol::derive_deps::ObjectWrapperRef::new(self.#t_index.as_ref().unwrap()).call_ref(args)
                        }
                    }
                    impl #assoc_only __protocol::derive_deps::ObjectMut<dyn #path> for __DERIVE_COALESCE_SHIM #assoc_only 
                    where
                        __DERIVE_PROTOCOL_TRANSPORT: #context_trait,
                        <__DERIVE_PROTOCOL_TRANSPORT as #context_trait>::Context: __protocol::FinalizeImmediate<__protocol::derive_deps::Complete<__DERIVE_CALL_ALIAS<<__DERIVE_PROTOCOL_TRANSPORT as #context_trait>::Context, #r_generics>>> #d_context_bounds,
                        <<__DERIVE_PROTOCOL_TRANSPORT as #context_trait>::Context as __protocol::FinalizeImmediate<__protocol::derive_deps::Complete<__DERIVE_CALL_ALIAS<<__DERIVE_PROTOCOL_TRANSPORT as #context_trait>::Context, #r_generics>>>>::Target: __protocol::Write<__DERIVE_CALL_ALIAS<<__DERIVE_PROTOCOL_TRANSPORT as #context_trait>::Context, #r_generics>> + ::core::marker::Unpin,
                        for<'a> __protocol::derive_deps::ObjectWrapperMut<'a, dyn #path, <<dyn #path as __protocol::derive_deps::Trait>::Coalesce as __protocol::derive_deps::TraitCoalesce<__DERIVE_PROTOCOL_TRANSPORT>>::Coalesce>: __protocol::derive_deps::ObjectMut<dyn #path>,
                        dyn #path: __protocol::derive_deps::Trait #st_bounds
                    {
                        fn call_mut(&mut self, args: <dyn #path as __protocol::derive_deps::Trait>::MutArgs) -> <dyn #path as __protocol::derive_deps::Trait>::MutRets {
                            __protocol::derive_deps::ObjectWrapperMut::new(self.#t_index.as_mut().unwrap()).call_mut(args)
                        }
                    }
                    impl #assoc_only __protocol::derive_deps::ObjectMove<dyn #path> for __DERIVE_COALESCE_SHIM #assoc_only 
                    where
                        __DERIVE_PROTOCOL_TRANSPORT: #context_trait,
                        <__DERIVE_PROTOCOL_TRANSPORT as #context_trait>::Context: __protocol::FinalizeImmediate<__protocol::derive_deps::Complete<__DERIVE_CALL_ALIAS<<__DERIVE_PROTOCOL_TRANSPORT as #context_trait>::Context, #r_generics>>> #d_context_bounds,
                        <<__DERIVE_PROTOCOL_TRANSPORT as #context_trait>::Context as __protocol::FinalizeImmediate<__protocol::derive_deps::Complete<__DERIVE_CALL_ALIAS<<__DERIVE_PROTOCOL_TRANSPORT as #context_trait>::Context, #r_generics>>>>::Target: __protocol::Write<__DERIVE_CALL_ALIAS<<__DERIVE_PROTOCOL_TRANSPORT as #context_trait>::Context, #r_generics>> + ::core::marker::Unpin,
                        __protocol::derive_deps::ObjectWrapperMove<dyn #path, Box<<<dyn #path as __protocol::derive_deps::Trait>::Coalesce as __protocol::derive_deps::TraitCoalesce<__DERIVE_PROTOCOL_TRANSPORT>>::Coalesce>>: __protocol::derive_deps::ObjectMove<dyn #path>,
                        dyn #path: __protocol::derive_deps::Trait #st_bounds
                    {
                        fn call_move(mut self, args: <dyn #path as __protocol::derive_deps::Trait>::MoveArgs) -> <dyn #path as __protocol::derive_deps::Trait>::MoveRets {
                            __protocol::derive_deps::ObjectWrapperMove::new(Box::new(self.#t_index.take().unwrap())).call_move(args)
                        }
                    }
                });
                idx += 1;
            }
            _ => {}
        }
    }

    let mut k_where_clause = where_clause.clone();
    k_where_clause.predicates.push(parse_quote!(__DERIVE_COALESCE_SHIM #assoc_only: #ident #type_generics));

    quote! {
        #shim

        impl #o_impl_generics #ident #o_type_generics for __DERIVE_COALESCE_SHIM #assoc_only #where_clause {
            #impl_stream
        }

        #supertrait_stream

        #other_shim

        macro_rules! marker_variants {
            ($(
                $($marker:ident)*
            ),+) => {
                $(
                    impl #impl_generics __protocol::Coalesce<__DERIVE_PROTOCOL_TRANSPORT> for __alloc::boxed::Box<dyn #ident #type_generics + 'derive_lifetime_param $(+ $marker)*> #k_where_clause #m_bound {
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
