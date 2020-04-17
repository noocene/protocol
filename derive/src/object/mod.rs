use crate::{make_ty, write_coalesce_conv, write_unravel_conv};
use proc_macro2::TokenStream;
use quote::{format_ident, quote, quote_spanned};
use syn::{
    parse2, parse_quote, spanned::Spanned, Data, DataEnum, DeriveInput, FnArg, GenericArgument,
    Item, ItemEnum, ItemTrait, PathArguments, ReturnType, TraitItem, Type, TypeParamBound,
};
use synstructure::{BindStyle, Structure};

mod coalesce;
mod unravel;

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
    let mut call = quote!();
    let target: FnArg = parse_quote!(self: Box<Self>);
    let mut call_generics = quote!();
    let mut call_context_bounds = quote!();
    let vis = &item.vis;
    let ident = &item.ident;
    let mut r_generics = quote!();
    let mut type_item = item.clone();
    let mut assoc_idents = vec![];
    for item in &mut item.items {
        if let TraitItem::Type(item) = item {
            let ident = format_ident!("__DERIVE_ASSOC_{}", item.ident);
            assoc_idents.push((item.ident.clone(), ident.clone()));
            let mut bounds = item.bounds.clone();
            bounds.push(parse_quote!('derive_lifetime_param));

            type_item
                .generics
                .params
                .push(parse_quote!(#ident: #bounds));
        }
    }
    let (_, o_type_generics, _) = item.generics.split_for_impl();
    let self_ty = quote!(#ident #o_type_generics);
    let mut ident_idx = 0;

    let mut some_by_ref = false;

    for item in &item.items {
        if let TraitItem::Method(method) = item {
            let moves = method
                .sig
                .inputs
                .first()
                .map(|item| item == &target)
                .unwrap_or(false);

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
                call_context_bounds.extend(quote!(+ __protocol::Notify<(#args)>));
                call.extend(quote! {
                    #ident(<C as __protocol::Dispatch<<C as __protocol::Notify<(#args)>>::Notification>>::Handle),
                });
            } else {
                some_by_ref = true;

                call.extend(quote! {
                    #ident(<C as __protocol::Contextualize>::Handle),
                });
            };
        }
    }

    let ctxtualize = if some_by_ref {
        quote!(__protocol::Contextualize)
    } else {
        quote!(Sized)
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

            #vis type __DERIVE_CALL_ALIAS<C: #ctxtualize #call_context_bounds, #call_generics> = #ty;

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

    let coalesce = coalesce::generate(item.clone());

    let mut data = quote! {
        #call

        #coalesce
    };

    if let Ok(Item::Const(_)) = parse2::<Item>(coalesce) {
    } else {
        let unravel = unravel::generate(item);

        data.extend(unravel);
    }

    data
}
