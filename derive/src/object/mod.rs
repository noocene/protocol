use crate::{make_ty, write_coalesce_conv, write_unravel_conv};
use proc_macro2::TokenStream;
use quote::{format_ident, quote, quote_spanned};
use syn::{
    parse2, parse_quote, spanned::Spanned, Data, DataEnum, DeriveInput, FnArg, GenericArgument,
    GenericParam, Item, ItemEnum, ItemTrait, PathArguments, ReturnType, TraitItem, Type,
    TypeParamBound,
};
use synstructure::{BindStyle, Structure};

mod coalesce;
mod unravel;

fn desugar_object(mut ty: Type) -> Type {
    match &mut ty {
        Type::Array(array) => {
            *array.elem = desugar_object(*array.elem.clone());
        }
        Type::Group(group) => {
            *group.elem = desugar_object(*group.elem.clone());
        }
        Type::Paren(paren) => {
            *paren.elem = desugar_object(*paren.elem.clone());
        }
        Type::TraitObject(object) => {
            let mut has_lifetime = false;

            for bound in &mut object.bounds {
                match bound {
                    TypeParamBound::Trait(bound) => {
                        let path = bound.path.clone();
                        let ty: Type = parse_quote!(#path);
                        let path = desugar_object(ty);
                        bound.path = parse_quote!(#path);
                    }
                    TypeParamBound::Lifetime(_) => {
                        has_lifetime = true;
                    }
                }
            }

            if !has_lifetime {
                object.bounds.push(parse_quote!('static));
            }
        }
        Type::Path(path) => {
            if let Some(qself) = &mut path.qself {
                *qself.ty = desugar_object(*qself.ty.clone())
            }

            for segment in &mut path.path.segments {
                match &mut segment.arguments {
                    PathArguments::AngleBracketed(args) => {
                        for arg in &mut args.args {
                            match arg {
                                GenericArgument::Type(ty) => {
                                    *ty = desugar_object(ty.clone());
                                }
                                GenericArgument::Binding(binding) => {
                                    binding.ty = desugar_object(binding.ty.clone());
                                }
                                GenericArgument::Constraint(constraint) => {
                                    for bound in &mut constraint.bounds {
                                        match bound {
                                            TypeParamBound::Trait(bound) => {
                                                let path = bound.path.clone();
                                                let ty: Type = parse_quote!(#path);
                                                let path = desugar_object(ty);
                                                bound.path = parse_quote!(#path);
                                            }
                                            _ => {}
                                        }
                                    }
                                }
                                _ => {}
                            }
                        }
                    }
                    PathArguments::Parenthesized(args) => {
                        for input in &mut args.inputs {
                            *input = desugar_object(input.clone());
                        }
                        match &mut args.output {
                            ReturnType::Type(_, ty) => *ty.as_mut() = desugar_object(*ty.clone()),
                            _ => {}
                        }
                    }
                    _ => {}
                }
            }
        }
        Type::Ptr(ptr) => {
            *ptr.elem = desugar_object(*ptr.elem.clone());
        }
        Type::Reference(reference) => {
            *reference.elem = desugar_object(*reference.elem.clone());
        }
        Type::Slice(slice) => {
            *slice.elem = desugar_object(*slice.elem.clone());
        }
        Type::Tuple(tuple) => {
            for ty in &mut tuple.elems {
                *ty = desugar_object(ty.clone());
            }
        }
        _ => {}
    };

    ty
}

fn rewrite_ty(mut ty: Type, self_ty: &TokenStream) -> Option<Type> {
    match &mut ty {
        Type::Array(array) => {
            *array.elem = rewrite_ty(*array.elem.clone(), &self_ty)?;
        }
        Type::Group(group) => {
            *group.elem = rewrite_ty(*group.elem.clone(), &self_ty)?;
        }
        Type::Paren(paren) => {
            *paren.elem = rewrite_ty(*paren.elem.clone(), &self_ty)?;
        }
        Type::TraitObject(object) => {
            for bound in &mut object.bounds {
                match bound {
                    TypeParamBound::Trait(bound) => {
                        let path = bound.path.clone();
                        let ty: Type = parse_quote!(#path);
                        let path = rewrite_ty(ty, &self_ty)?;
                        bound.path = parse_quote!(#path);
                    }
                    _ => {}
                }
            }
        }
        Type::Path(path) => {
            if let Some(qself) = &mut path.qself {
                *qself.ty = rewrite_ty(*qself.ty.clone(), &self_ty)?
            }

            for segment in &mut path.path.segments {
                match &mut segment.arguments {
                    PathArguments::AngleBracketed(args) => {
                        for arg in &mut args.args {
                            match arg {
                                GenericArgument::Type(ty) => {
                                    *ty = rewrite_ty(ty.clone(), &self_ty)?;
                                }
                                GenericArgument::Binding(binding) => {
                                    binding.ty = rewrite_ty(binding.ty.clone(), &self_ty)?;
                                }
                                GenericArgument::Constraint(constraint) => {
                                    for bound in &mut constraint.bounds {
                                        match bound {
                                            TypeParamBound::Trait(bound) => {
                                                let path = bound.path.clone();
                                                let ty: Type = parse_quote!(#path);
                                                let path = rewrite_ty(ty, &self_ty)?;
                                                bound.path = parse_quote!(#path);
                                            }
                                            _ => {}
                                        }
                                    }
                                }
                                _ => {}
                            }
                        }
                    }
                    PathArguments::Parenthesized(args) => {
                        for input in &mut args.inputs {
                            *input = rewrite_ty(input.clone(), &self_ty)?;
                        }
                        match &mut args.output {
                            ReturnType::Type(_, ty) => {
                                *ty.as_mut() = rewrite_ty(*ty.clone(), &self_ty)?
                            }
                            _ => {}
                        }
                    }
                    _ => {}
                }
            }

            let p = if let Some(segment) = path.path.segments.iter().next() {
                if format!("{}", segment.ident) == "Self" {
                    if path.path.segments.len() > 1 {
                        let ident = format_ident!("__DERIVE_ASSOC_{}", path.path.segments[1].ident);
                        parse_quote!(#ident)
                    } else {
                        return None;
                    }
                } else {
                    parse_quote!(#path)
                }
            } else {
                parse_quote!(#path)
            };

            *path = p;
        }
        Type::Ptr(ptr) => {
            *ptr.elem = rewrite_ty(*ptr.elem.clone(), &self_ty)?;
        }
        Type::Reference(reference) => {
            *reference.elem = rewrite_ty(*reference.elem.clone(), &self_ty)?;
        }
        Type::Slice(slice) => {
            *slice.elem = rewrite_ty(*slice.elem.clone(), &self_ty)?;
        }
        Type::Tuple(tuple) => {
            for ty in &mut tuple.elems {
                *ty = rewrite_ty(ty.clone(), &self_ty)?;
            }
        }
        _ => {}
    };

    Some(ty)
}

pub fn generate(mut item: ItemTrait) -> TokenStream {
    let mut mut_stream = quote!();
    let mut mut_ret_stream = quote!();
    let mut ref_stream = quote!();
    let mut ref_ret_stream = quote!();
    let mut move_stream = quote!();
    let mut move_ret_stream = quote!();

    let mut object_move_stream = quote!();
    let mut object_ref_stream = quote!();
    let mut object_mut_stream = quote!();

    let mut dispatch_stream = quote!();

    let mut t_c_generics = quote!();

    let mut d_context_bounds = quote!();

    let mut call = quote!();
    let target: FnArg = parse_quote!(self: Box<Self>);
    let mut call_generics = quote!();
    let mut call_context_bounds = quote!();
    let vis = &item.vis;
    let ident = &item.ident;
    let mut r_generics = quote!();
    let mut type_item = item.clone();
    let mut params = quote!();
    let mut ty_generics = quote!();
    let mut na_ty_generics = quote!();
    let mut assoc_idents = vec![];
    for param in &item.generics.params {
        if let GenericParam::Type(ty) = param {
            let ident = &ty.ident;
            t_c_generics.extend(quote!(#ident,));
            ty_generics.extend(quote!(#ident,));
            na_ty_generics.extend(quote!(#ident,));
            params.extend(quote!(#ident,));
        } else if let GenericParam::Lifetime(l) = param {
            ty_generics.extend(quote!(#l,));
            na_ty_generics.extend(quote!(#l,));
            params.extend(quote!(#l,));
        }
    }
    for item in &mut item.items {
        if let TraitItem::Type(item) = item {
            let o_ident = &item.ident;
            let ident = format_ident!("__DERIVE_ASSOC_{}", item.ident);
            ty_generics.extend(quote!(#o_ident = #ident,));
            params.extend(quote!(#ident,));
            assoc_idents.push((item.ident.clone(), ident.clone()));
            let mut bounds = item.bounds.clone();
            bounds.push(parse_quote!('derive_lifetime_param));

            type_item
                .generics
                .params
                .push(parse_quote!(#ident: #bounds));
        }
    }
    if !ty_generics.is_empty() {
        ty_generics = quote!(<#ty_generics>);
    }
    if !na_ty_generics.is_empty() {
        na_ty_generics = quote!(<#na_ty_generics>);
    }
    let (_, o_type_generics, _) = item.generics.split_for_impl();
    type_item
        .generics
        .params
        .push(parse_quote!('derive_lifetime_param));
    let (impl_generics, _, _) = type_item.generics.split_for_impl();
    let mut type_item_k = type_item.clone();
    type_item_k
        .generics
        .params
        .push(parse_quote!(__DERIVE_OBJECT_TY));
    let (k_impl_generics, _, _) = type_item_k.generics.split_for_impl();
    let self_ty = quote!(#ident #o_type_generics);
    let mut ident_idx = 0;

    let mut some_by_ref = false;

    let mut has_methods = false;

    let o_ident = &item.ident;

    let ki = item.clone();
    let handles: Box<dyn Fn(TokenStream, TokenStream) -> TokenStream> = if item
        .supertraits
        .is_empty()
    {
        Box::new(|_, transport| quote!(<#transport as __protocol::Contextualize>::Handle))
    } else {
        Box::new(|o_t, transport| {
            let mut stream = quote!(<#transport as __protocol::Contextualize>::Handle);

            for supertrait in &ki.supertraits {
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

            let mut args = quote!();
            let mut r_args = quote!();
            let mut bindings = quote!();

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

            let ident = &method.sig.ident;

            if moves {
                move_stream.extend(quote!(#ident(#r_args),));
                move_ret_stream.extend(quote!(#ident(#ret),));

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
                    call_generics.extend(quote!(#ident,));
                    r_generics.extend(quote!(#ty,));

                    args.extend(quote!(#ident,));
                }
                d_context_bounds.extend(quote!(+ __protocol::Notify<(#r_args)>));
                call_context_bounds.extend(quote!(+ __protocol::Notify<(#args)>));
                let sig = &method.sig;
                dispatch_stream.extend(quote! { #sig {
                    if let __DERIVE_MOVE_RET::#ident(ret) =
                    <__DERIVE_OBJECT_TY as __protocol::derive_deps::ObjectMove<
                        dyn #o_ident #ty_generics + 'derive_lifetime_param,
                    >>::call_move(*self, __DERIVE_MOVE_ARGS::#ident(#bindings)) {
                        ret
                    } else {
                        panic!("invalid ret in move")
                    }
                }});
                object_move_stream.extend(quote! {
                    __DERIVE_MOVE_ARGS::#ident(#bindings) => {
                        __DERIVE_MOVE_RET::#ident(<__DERIVE_OBJECT_TY as #o_ident #ty_generics>::#ident(self.0, #bindings))
                    }
                });
                call.extend(quote! {
                    #ident(<C as __protocol::Dispatch<<C as __protocol::Notify<(#args)>>::Notification>>::Handle),
                });
            } else {
                method.sig.inputs.first().map(|input| match input {
                    FnArg::Receiver(receiver) => {
                        if receiver.mutability.is_some() {
                            let sig = &method.sig;
                            object_mut_stream.extend(quote! {
                                __DERIVE_MUT_ARGS::#ident(#bindings) => {
                                    __DERIVE_MUT_RET::#ident(<__DERIVE_OBJECT_TY as #o_ident #ty_generics>::#ident(self.0, #bindings))
                                }
                            });
                            dispatch_stream.extend(quote! { #sig {
                                if let __DERIVE_MUT_RET::#ident(ret) =
                                <__DERIVE_OBJECT_TY as __protocol::derive_deps::ObjectMut<
                                    dyn #o_ident #ty_generics + 'derive_lifetime_param,
                                >>::call_mut(self, __DERIVE_MUT_ARGS::#ident(#bindings)) {
                                    ret
                                } else {
                                    panic!("invalid ret in mut")
                                }
                            }});
                            mut_stream.extend(quote!(#ident(#r_args),));
                            mut_ret_stream.extend(quote!(#ident(#ret),));
                        } else {
                            let sig = &method.sig;
                            object_ref_stream.extend(quote! {
                                __DERIVE_REF_ARGS::#ident(#bindings) => {
                                    __DERIVE_REF_RET::#ident(<__DERIVE_OBJECT_TY as #o_ident #ty_generics>::#ident(self.0, #bindings))
                                }
                            });
                            dispatch_stream.extend(quote! { #sig {
                                if let __DERIVE_REF_RET::#ident(ret) =
                                <__DERIVE_OBJECT_TY as __protocol::derive_deps::ObjectRef<
                                    dyn #o_ident #ty_generics + 'derive_lifetime_param,
                                >>::call_ref(self, __DERIVE_REF_ARGS::#ident(#bindings)) {
                                    ret
                                } else {
                                    panic!("invalid ret in ref")
                                }
                            }});
                            ref_stream.extend(quote!(#ident(#r_args),));
                            ref_ret_stream.extend(quote!(#ident(#ret),));
                        }
                    }
                    _ => {}
                });

                some_by_ref = true;

                call.extend(quote! {
                    #ident(<C as __protocol::Contextualize>::Handle),
                });
            };
        }
    }

    let (ctxtualize, context_trait) = if some_by_ref {
        d_context_bounds.extend(quote!(+ __protocol::Contextualize));
        (
            quote!(__protocol::Contextualize),
            quote!(__protocol::ShareContext),
        )
    } else {
        (quote!(Sized), quote!(__protocol::JoinContext))
    };

    if !call.is_empty() {
        call = quote!(#vis enum __DERIVE_CALL<C: #ctxtualize #call_context_bounds, #call_generics> {
            #call
            __DERIVE_TERMINATE
        });

        let data = parse2::<ItemEnum>(call.clone()).unwrap();
        let input = DeriveInput {
            attrs: data.attrs,
            generics: data.generics,
            ident: data.ident,
            vis: data.vis,
            data: Data::Enum(DataEnum {
                enum_token: data.enum_token,
                variants: data.variants,
                brace_token: data.brace_token,
            }),
        };
        let mut structure = Structure::new(&input);
        let ty = make_ty(structure.clone());

        let from = write_coalesce_conv(structure.variants());
        structure.bind_with(|_| BindStyle::Move);
        let into = write_unravel_conv(structure);

        call.extend(quote! {
            #vis struct __DERIVE_CALL_NEWTYPE<T>(#vis T);

            #vis type __DERIVE_CALL_ALIAS<C, #call_generics> = #ty;

            impl<C: #ctxtualize #call_context_bounds, #call_generics> From<#ty> for __DERIVE_CALL<C, #call_generics> {
                fn from(data: #ty) -> Self {
                    #from
                }
            }

            impl<C: #ctxtualize #call_context_bounds, #call_generics> Into<__DERIVE_CALL_NEWTYPE<#ty>> for __DERIVE_CALL<C, #call_generics> {
                fn into(self) -> __DERIVE_CALL_NEWTYPE<#ty> {
                    __DERIVE_CALL_NEWTYPE(#into)
                }
            }
        });
    }

    let mut marker_stream = quote!();

    if !move_stream.is_empty() {
        move_stream = quote! {
            #vis enum __DERIVE_MOVE_ARGS #impl_generics {
                __DERIVE_PHANTOM(::core::marker::PhantomData<(#params)>, ::core::marker::PhantomData<&'derive_lifetime_param ()>),
                #move_stream
            }
        };
        move_ret_stream = quote! {
            #vis enum __DERIVE_MOVE_RET #impl_generics {
                __DERIVE_PHANTOM(::core::marker::PhantomData<(#params)>, ::core::marker::PhantomData<&'derive_lifetime_param ()>),
                #move_ret_stream
            }
        };
        marker_stream.extend(quote! {
            type MoveArgs = __DERIVE_MOVE_ARGS<'derive_lifetime_param, #params>;
            type MoveRets = __DERIVE_MOVE_RET<'derive_lifetime_param, #params>;
        });
    } else {
        marker_stream.extend(quote! {
            type MoveArgs = __protocol::derive_deps::Void;
            type MoveRets = __protocol::derive_deps::Void;
        });
    }

    if !ref_stream.is_empty() {
        ref_stream = quote! {
            #vis enum __DERIVE_REF_ARGS #impl_generics {
                __DERIVE_PHANTOM(::core::marker::PhantomData<(#params)>, ::core::marker::PhantomData<&'derive_lifetime_param ()>),
                #ref_stream
            }
        };
        ref_ret_stream = quote! {
            #vis enum __DERIVE_REF_RET #impl_generics {
                __DERIVE_PHANTOM(::core::marker::PhantomData<(#params)>, ::core::marker::PhantomData<&'derive_lifetime_param ()>),
                #ref_ret_stream
            }
        };
        marker_stream.extend(quote! {
            type RefArgs = __DERIVE_REF_ARGS<'derive_lifetime_param, #params>;
            type RefRets = __DERIVE_REF_RET<'derive_lifetime_param, #params>;
        });
    } else {
        marker_stream.extend(quote! {
            type RefArgs = __protocol::derive_deps::Void;
            type RefRets = __protocol::derive_deps::Void;
        });
    }

    if !mut_stream.is_empty() {
        mut_stream = quote! {
            #vis enum __DERIVE_MUT_ARGS #impl_generics {
                __DERIVE_PHANTOM(::core::marker::PhantomData<(#params)>, ::core::marker::PhantomData<&'derive_lifetime_param ()>),
                #mut_stream
            }
        };
        mut_ret_stream = quote! {
            #vis enum __DERIVE_MUT_RET #impl_generics {
                __DERIVE_PHANTOM(::core::marker::PhantomData<(#params)>, ::core::marker::PhantomData<&'derive_lifetime_param ()>),
                #mut_ret_stream
            }
        };
        marker_stream.extend(quote! {
            type MutArgs = __DERIVE_MUT_ARGS<'derive_lifetime_param, #params>;
            type MutRets = __DERIVE_MUT_RET<'derive_lifetime_param, #params>;
        });
    } else {
        marker_stream.extend(quote! {
            type MutArgs = __protocol::derive_deps::Void;
            type MutRets = __protocol::derive_deps::Void;
        });
    }

    marker_stream.extend(quote! {
        type Coalesce = __DERIVE_COALESCE_SHIM_WRAPPER<#t_c_generics>;
    });

    let c_generics = quote!(__DERIVE_PROTOCOL_TRANSPORT, #t_c_generics);

    let coalesce = coalesce::generate(item.clone());

    let t_fut = quote!(__protocol::future::MapOk<<__DERIVE_PROTOCOL_TRANSPORT as #context_trait>::JoinOutput, fn(<__DERIVE_PROTOCOL_TRANSPORT as #context_trait>::Context) -> __DERIVE_COALESCE_SHIM<__DERIVE_PROTOCOL_TRANSPORT, #t_c_generics>>);

    let j_call = if some_by_ref {
        quote!(join_shared)
    } else {
        quote!(join_owned)
    };

    let (t_c_bound, shim, shim_ctor, handle_ty, future_ty) = if has_methods {
        (
            if item.supertraits.is_empty() {
                quote!(where <__DERIVE_PROTOCOL_TRANSPORT as #context_trait>::JoinOutput: Unpin, __DERIVE_PROTOCOL_TRANSPORT: ?Sized + #context_trait, <__DERIVE_PROTOCOL_TRANSPORT as #context_trait>::Context: __protocol::FinalizeImmediate<__protocol::derive_deps::Complete<__DERIVE_CALL_ALIAS<<__DERIVE_PROTOCOL_TRANSPORT as #context_trait>::Context, #r_generics>>> #d_context_bounds, <<__DERIVE_PROTOCOL_TRANSPORT as #context_trait>::Context as __protocol::FinalizeImmediate<__protocol::derive_deps::Complete<__DERIVE_CALL_ALIAS<<__DERIVE_PROTOCOL_TRANSPORT as #context_trait>::Context, #r_generics>>>>::Target: __protocol::Write<__DERIVE_CALL_ALIAS<<__DERIVE_PROTOCOL_TRANSPORT as #context_trait>::Context, #r_generics>> + ::core::marker::Unpin)
            } else {
                let handle = (handles)(
                    quote!(__DERIVE_PROTOCOL_TRANSPORT),
                    quote!(__DERIVE_PROTOCOL_TRANSPORT),
                );

                quote!(where <__DERIVE_PROTOCOL_TRANSPORT as #context_trait>::JoinOutput: Unpin, __DERIVE_PROTOCOL_TRANSPORT: __protocol::Read<#handle> + Unpin + ?Sized + #context_trait, <__DERIVE_PROTOCOL_TRANSPORT as #context_trait>::Context: __protocol::FinalizeImmediate<__protocol::derive_deps::Complete<__DERIVE_CALL_ALIAS<<__DERIVE_PROTOCOL_TRANSPORT as #context_trait>::Context, #r_generics>>> #d_context_bounds, <<__DERIVE_PROTOCOL_TRANSPORT as #context_trait>::Context as __protocol::FinalizeImmediate<__protocol::derive_deps::Complete<__DERIVE_CALL_ALIAS<<__DERIVE_PROTOCOL_TRANSPORT as #context_trait>::Context, #r_generics>>>>::Target: __protocol::Write<__DERIVE_CALL_ALIAS<<__DERIVE_PROTOCOL_TRANSPORT as #context_trait>::Context, #r_generics>> + ::core::marker::Unpin)
            },
            quote!(__DERIVE_COALESCE_SHIM<__DERIVE_PROTOCOL_TRANSPORT, #t_c_generics>),
            if item.supertraits.is_empty() {
                quote!(__protocol::WithContext::new(
                    context,
                    |ctx, context| __protocol::FutureExt::map_ok(
                        ctx.#j_call(context),
                        |context| __DERIVE_COALESCE_SHIM::<__DERIVE_PROTOCOL_TRANSPORT, #t_c_generics>(::core::marker::PhantomData, Some(context))
                    )
                ))
            } else {
                quote!(__DERIVE_COALESCE_READER::new_from_handles(context))
            },
            if item.supertraits.is_empty() {
                quote!(<__DERIVE_PROTOCOL_TRANSPORT as __protocol::Contextualize>::Handle)
            } else {
                (handles)(
                    quote!(__DERIVE_PROTOCOL_TRANSPORT),
                    quote!(__DERIVE_PROTOCOL_TRANSPORT),
                )
            },
            if item.supertraits.is_empty() {
                quote!(__protocol::WithContext<__DERIVE_PROTOCOL_TRANSPORT, #t_fut, fn(&mut __DERIVE_PROTOCOL_TRANSPORT, <__DERIVE_PROTOCOL_TRANSPORT as __protocol::Contextualize>::Handle) -> #t_fut, <__DERIVE_PROTOCOL_TRANSPORT as __protocol::Contextualize>::Handle>)
            } else {
                quote!(__DERIVE_COALESCE_READER<#t_c_generics __DERIVE_PROTOCOL_TRANSPORT>)
            },
        )
    } else {
        (
            quote!(where __DERIVE_PROTOCOL_TRANSPORT: ?Sized),
            quote!(__DERIVE_COALESCE_SHIM),
            quote!(__protocol::future::ok(__DERIVE_COALESCE_SHIM(
                ::core::marker::PhantomData,
            ))),
            quote!(()),
            quote!(__protocol::future::Ready<__DERIVE_COALESCE_SHIM>),
        )
    };

    let mut supertrait_bounds = quote!();

    for supertrait in &item.supertraits {
        match supertrait {
            TypeParamBound::Trait(supertrait) => {
                let path = &supertrait.path;
                supertrait_bounds.extend(
                    quote!(+ __protocol::derive_deps::Object<dyn #path + 'derive_lifetime_param>),
                );
            }
            _ => {}
        }
    }

    let object_move_stream = if object_move_stream.is_empty() {
        quote!(panic!("got bottom type"))
    } else {
        quote! {
            match args {
                #object_move_stream
                __DERIVE_MOVE_ARGS::__DERIVE_PHANTOM(_, _) => {
                    panic!("invalid argument wrapper")
                }
            }
        }
    };
    let object_mut_stream = if object_mut_stream.is_empty() {
        quote!(panic!("got bottom type"))
    } else {
        quote! {
            match args {
                #object_mut_stream
                __DERIVE_MUT_ARGS::__DERIVE_PHANTOM(_, _) => {
                    panic!("invalid argument wrapper")
                }
            }
        }
    };
    let object_ref_stream = if object_ref_stream.is_empty() {
        quote!(panic!("got bottom type"))
    } else {
        quote! {
            match args {
                #object_ref_stream
                __DERIVE_REF_ARGS::__DERIVE_PHANTOM(_, _) => {
                    panic!("invalid argument wrapper")
                }
            }
        }
    };

    let object_impls = quote! {
        impl #impl_generics __protocol::derive_deps::Trait for dyn #ident #ty_generics + 'derive_lifetime_param {
            #marker_stream
        }

        impl #k_impl_generics __protocol::derive_deps::ObjectMove<dyn #ident #ty_generics> for __protocol::derive_deps::ObjectWrapperMove<dyn #ident #ty_generics, Box<__DERIVE_OBJECT_TY>> where __DERIVE_OBJECT_TY: ?Sized + #ident #ty_generics {
            fn call_move(mut self, args: <dyn #ident #ty_generics as __protocol::derive_deps::Trait>::MoveArgs) -> <dyn #ident #ty_generics as __protocol::derive_deps::Trait>::MoveRets {
                #object_move_stream
            }
        }

        impl #k_impl_generics __protocol::derive_deps::ObjectMut<dyn #ident #ty_generics> for __protocol::derive_deps::ObjectWrapperMut<'derive_lifetime_param, dyn #ident #ty_generics, __DERIVE_OBJECT_TY> where __DERIVE_OBJECT_TY: ?Sized + #ident #ty_generics {
            fn call_mut(&mut self, args: <dyn #ident #ty_generics as __protocol::derive_deps::Trait>::MutArgs) -> <dyn #ident #ty_generics as __protocol::derive_deps::Trait>::MutRets {
                #object_mut_stream
            }
        }

        impl #k_impl_generics __protocol::derive_deps::ObjectRef<dyn #ident #ty_generics> for __protocol::derive_deps::ObjectWrapperRef<'derive_lifetime_param, dyn #ident #ty_generics, __DERIVE_OBJECT_TY> where __DERIVE_OBJECT_TY: ?Sized + #ident #ty_generics {
            fn call_ref(&self, args: <dyn #ident #ty_generics as __protocol::derive_deps::Trait>::RefArgs) -> <dyn #ident #ty_generics as __protocol::derive_deps::Trait>::RefRets {
                #object_ref_stream
            }
        }
    };

    let mut st_bounds = quote!();

    for supertrait in &item.supertraits {
        if let TypeParamBound::Trait(supertrait) = supertrait {
            let path = &supertrait.path;
            st_bounds.extend(quote! {
                , dyn #path: __protocol::derive_deps::Trait
                , <dyn #path as __protocol::derive_deps::Trait>::Coalesce: __protocol::derive_deps::TraitCoalesce<__DERIVE_PROTOCOL_TRANSPORT>
            });
        }
    }

    let mut data = if assoc_idents.len() == 0 {
        quote! {
            pub struct __DERIVE_COALESCE_SHIM_WRAPPER<#t_c_generics>(::core::marker::PhantomData<(#t_c_generics)>);

            impl<#c_generics> __protocol::derive_deps::TraitCoalesce<__DERIVE_PROTOCOL_TRANSPORT> for __DERIVE_COALESCE_SHIM_WRAPPER<#t_c_generics> #t_c_bound #st_bounds {
                type Coalesce = #shim;
                type Handle = #handle_ty;
                type Future = #future_ty;

                fn new(context: Self::Handle) -> Self::Future {
                    #shim_ctor
                }
            }

            impl #k_impl_generics #ident #na_ty_generics for __DERIVE_OBJECT_TY where __DERIVE_OBJECT_TY: __protocol::derive_deps::Object<dyn #ident #ty_generics + 'derive_lifetime_param> #supertrait_bounds {
                #dispatch_stream
            }

            #object_impls

            #move_stream
            #move_ret_stream
            #ref_stream
            #ref_ret_stream
            #mut_stream
            #mut_ret_stream
        }
    } else {
        quote!()
    };

    data.extend(quote! {
        #call

        #coalesce
    });

    if let Ok(Item::Const(_)) = parse2::<Item>(coalesce) {
    } else {
        let unravel = unravel::generate(item);

        data.extend(unravel);
    }

    data
}
