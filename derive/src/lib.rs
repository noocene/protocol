use proc_macro2::TokenStream;
use quote::{format_ident, quote, quote_spanned, ToTokens};
use std::collections::HashMap;
use syn::{
    parse::{Parse, ParseStream, Result},
    parse2, parse_quote,
    punctuated::Punctuated,
    Data, DataEnum, DataStruct, DeriveInput, Item, Meta, NestedMeta, Token, Type,
};
use synstructure::Structure;

mod object;
mod structure;

use structure::{make_ty, write_coalesce_conv, write_unravel_conv};

struct Metas {
    metas: Punctuated<NestedMeta, Token![,]>,
}

impl Parse for Metas {
    fn parse(input: ParseStream) -> Result<Self> {
        Ok(Metas {
            metas: Punctuated::parse_terminated(input)?,
        })
    }
}

static META_NAMES: &'static [&'static str] = &["with"];

fn parse_metas(
    stream: TokenStream,
) -> std::result::Result<HashMap<&'static str, Vec<NestedMeta>>, TokenStream> {
    let mut registry: HashMap<&'static str, Vec<NestedMeta>> = HashMap::new();

    if !stream.is_empty() {
        match parse2::<Metas>(stream) {
            Ok(metas) => {
                for meta in metas.metas {
                    match meta {
                        NestedMeta::Lit(_) => {
                            return Err(quote!(
                                const _DERIVE_PROTOCOL_Error: () = { compile_error!("invalid meta in `protocol` attribute: expected nested meta, got literal") };
                            )
                            .into());
                        }
                        NestedMeta::Meta(data) => match data {
                            Meta::NameValue(_) => {
                                return Err(quote!(
                                    const _DERIVE_PROTOCOL_Error: () = { compile_error!("invalid meta in `protocol` attribute: expected list, got key-value pair") };
                                )
                                .into());
                            }
                            Meta::Path(_) => {
                                return Err(quote!(
                                    const _DERIVE_PROTOCOL_Error: () = { compile_error!("invalid meta in `protocol` attribute: expected list, got bare path") };
                                )
                                .into());
                            }
                            Meta::List(data) => {
                                if let Some(ident) = data.path.get_ident() {
                                    let ident = format!("{}", ident);

                                    if let Some(pos) =
                                        META_NAMES.iter().position(|item| *item == ident)
                                    {
                                        if registry.contains_key(META_NAMES[pos]) {
                                            let message = format!(
                                                "duplicate key `{}` in `protocol` attribute",
                                                ident
                                            );

                                            return Err(quote!(
                                                const _DERIVE_PROTOCOL_Error: () =
                                                    { compile_error!(#message) };
                                            )
                                            .into());
                                        }

                                        registry.insert(
                                            META_NAMES[pos],
                                            data.nested.into_iter().collect(),
                                        );
                                    } else {
                                        let message = format!(
                                            "unknown key `{}` in `protocol` attribute",
                                            ident
                                        );

                                        return Err(quote!(
                                            const _DERIVE_PROTOCOL_Error: () =
                                                { compile_error!(#message) };
                                        )
                                        .into());
                                    }
                                } else {
                                    let message = format!(
                                        "unknown key {} in `protocol` attribute",
                                        data.path.to_token_stream()
                                    );

                                    return Err(quote!(
                                        const _DERIVE_PROTOCOL_Error: () =
                                            { compile_error!(#message) };
                                    )
                                    .into());
                                }
                            }
                        },
                    }
                }
            }
            Err(e) => {
                let message = format!("invalid metas in `protocol` attribute: {}", e);

                let span = e.span();

                return Err(quote_spanned!(
                    span =>
                    const _DERIVE_PROTOCOL_Error: () =
                        { compile_error!(#message) };
                )
                .into());
            }
        }
    }

    Ok(registry)
}

#[proc_macro_attribute]
pub fn protocol(
    attribute: proc_macro::TokenStream,
    data: proc_macro::TokenStream,
) -> proc_macro::TokenStream {
    let data: TokenStream = data.into();

    let mut registry = match parse_metas(attribute.into()) {
        Ok(data) => data,
        Err(e) => return e.into(),
    };

    let adapter: Option<Type> = if let Some(metas) = registry.remove("with") {
        if metas.len() != 1 {
            return quote!(
                const _DERIVE_PROTOCOL_Error: () = {
                    compile_error!("invalid `with` in `protocol` attribute: expected a single path")
                };
            )
            .into();
        }

        if let NestedMeta::Meta(Meta::Path(path)) = metas.into_iter().next().unwrap() {
            Some(parse_quote!(#path))
        } else {
            return quote!(
                const _DERIVE_PROTOCOL_Error: () = {
                    compile_error!("invalid `with` in `protocol` attribute: expected a single path")
                };
            )
            .into();
        }
    } else {
        None
    };

    let item = parse2::<Item>(data.clone());

    let mut ident = None;

    let imp: TokenStream = match item {
        Ok(item) => match item {
            Item::Struct(data) => {
                ident = Some(data.ident.clone());
                let input = DeriveInput {
                    attrs: data.attrs,
                    generics: data.generics,
                    ident: data.ident,
                    vis: data.vis,
                    data: Data::Struct(DataStruct {
                        fields: data.fields,
                        struct_token: data.struct_token,
                        semi_token: data.semi_token,
                    }),
                };
                let structure = Structure::new(&input);
                if let Some(adapter) = adapter {
                    structure::generate_adapted(structure, adapter)
                } else {
                    structure::generate(structure)
                }
            }
            Item::Enum(data) => {
                ident = Some(data.ident.clone());
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
                let structure = Structure::new(&input);
                if let Some(adapter) = adapter {
                    structure::generate_adapted(structure, adapter)
                } else {
                    structure::generate(structure)
                }
            }
            Item::Trait(data) => {
                ident = Some(data.ident.clone());
                if data.supertraits.len() > 0 {
                    quote!(compile_error!(
                        "invalid item: supertraits are not yet supported"
                    ))
                } else {
                    object::generate(data)
                }
            }
            _ => quote!(compile_error!(
                "invalid item: expected struct, enum, or trait"
            )),
        },
        _ => quote!(compile_error!(
            "invalid item: expected struct, enum, or trait"
        )),
    }
    .into();

    let item_ident = format_ident!(
        "__DERIVE_PROTOCOL_{}",
        ident.unwrap_or_else(|| format_ident!("FAILURE"))
    );

    (quote! {
        #[allow(non_upper_case_globals, unused_parens, non_camel_case_types)]
        const #item_ident: () = {
            extern crate protocol as __protocol;
            extern crate alloc as __alloc;

            #imp
        };

        #data
    })
    .into()
}
