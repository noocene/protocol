use proc_macro2::TokenStream;
use quote::{format_ident, quote};
use syn::{parse, Data, DataEnum, DataStruct, DeriveInput, Item};
use synstructure::Structure;

mod structure;

#[proc_macro_attribute]
pub fn protocol(
    _: proc_macro::TokenStream,
    data: proc_macro::TokenStream,
) -> proc_macro::TokenStream {
    let item = parse::<Item>(data.clone());

    let mut ident = None;

    let imp: TokenStream = match item {
        Ok(item) => match item {
            Item::Struct(data) => {
                ident = Some(data.ident.clone());
                structure::generate(Structure::new(&DeriveInput {
                    attrs: data.attrs,
                    generics: data.generics,
                    ident: data.ident,
                    vis: data.vis,
                    data: Data::Struct(DataStruct {
                        fields: data.fields,
                        struct_token: data.struct_token,
                        semi_token: data.semi_token,
                    }),
                }))
            }
            Item::Enum(data) => {
                ident = Some(data.ident.clone());
                structure::generate(Structure::new(&DeriveInput {
                    attrs: data.attrs,
                    generics: data.generics,
                    ident: data.ident,
                    vis: data.vis,
                    data: Data::Enum(DataEnum {
                        enum_token: data.enum_token,
                        variants: data.variants,
                        brace_token: data.brace_token,
                    }),
                }))
            }
            _ => quote!(compile_error!("invalid item: expected struct")),
        },
        _ => quote!(compile_error!("invalid item: expected struct")),
    }
    .into();

    let item_ident = format_ident!(
        "__DERIVE_PROTOCOL_{}",
        ident.unwrap_or_else(|| format_ident!("FAILURE"))
    );

    let data: TokenStream = data.into();

    (quote! {
        #[allow(non_upper_case_globals)]
        const #item_ident: () = {
            extern crate protocol as __protocol;

            #imp
        };

        #data
    })
    .into()
}
