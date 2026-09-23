#![forbid(unsafe_code)]

use proc_macro::TokenStream;
use quote::quote;
use syn::{parse_macro_input, Data, DeriveInput};

#[proc_macro_attribute]
pub fn contract(args: TokenStream, item: TokenStream) -> TokenStream {
    let input = parse_macro_input!(item as DeriveInput);

    if !args.is_empty() {
        return syn::Error::new_spanned(&input, "contract does not accept arguments")
            .into_compile_error()
            .into();
    }

    if !input.generics.params.is_empty() {
        return syn::Error::new_spanned(
            &input.generics,
            "generic contracts are not supported",
        )
        .into_compile_error()
        .into();
    }

    if matches!(&input.data, Data::Union(_)) {
        return syn::Error::new_spanned(
            &input.ident,
            "contracts must be structs or enums",
        )
        .into_compile_error()
        .into();
    }

    let name = &input.ident;

    quote! {
        #[derive(::orb_contract_schema::__private::schemars::JsonSchema)]
        #[schemars(crate = "orb_contract_schema::__private::schemars")]
        #input

        ::orb_contract_schema::__private::inventory::submit! {
            ::orb_contract_schema::Contract {
                name: ::core::concat!(
                    ::core::module_path!(),
                    "::",
                    ::core::stringify!(#name),
                ),
                schema: || {
                    ::orb_contract_schema::__private::schemars::schema_for!(#name)
                },
            }
        }
    }
    .into()
}
