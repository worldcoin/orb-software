use proc_macro::TokenStream;
use quote::quote;
use std::mem::take;
use syn::{
    parse::{Parse, ParseStream, Result},
    parse_macro_input,
    punctuated::Punctuated,
    Expr, Ident, ItemFn, LitStr, Token,
};

enum TestAttr {
    Init(Expr),
    Timeout(Expr),
}

impl Parse for TestAttr {
    fn parse(input: ParseStream) -> Result<Self> {
        let ident = input.parse::<Ident>()?;
        match ident.to_string().as_str() {
            "init" => {
                input.parse::<Token![=]>()?;
                Ok(Self::Init(input.parse()?))
            }
            "timeout" => {
                input.parse::<Token![=]>()?;
                Ok(Self::Timeout(input.parse()?))
            }
            ident => panic!("Unknown option: {ident}"),
        }
    }
}

pub fn proc_macro_attribute(attr: TokenStream, item: TokenStream) -> TokenStream {
    let test_attrs =
        parse_macro_input!(attr with Punctuated::<TestAttr, Token![,]>::parse_terminated);
    let init = test_attrs
        .iter()
        .find_map(|attr| if let TestAttr::Init(expr) = attr { Some(quote!(#expr)) } else { None })
        .unwrap_or_else(|| quote!(|| {}));
    let timeout = test_attrs
        .iter()
        .find_map(
            |attr| {
                if let TestAttr::Timeout(expr) = attr { Some(quote!(#expr)) } else { None }
            },
        )
        .unwrap_or_else(|| quote!(::agentwire::testing_rt::DEFAULT_TIMEOUT));

    let ItemFn { attrs, vis, mut sig, block } = parse_macro_input!(item as ItemFn);
    let test_name = LitStr::new(&sig.ident.to_string(), sig.ident.span());
    assert!(take(&mut sig.asyncness).is_some(), "Test function must be async");

    let expanded = quote! {
        #(#attrs)*
        #[test]
        #vis #sig {
            struct TestId;
            let test_id = ::std::any::TypeId::of::<TestId>();
            ::agentwire::testing_rt::run_broker_test(
                ::std::stringify!(#test_name),
                &::std::format!("{test_id:?}"),
                ::std::time::Duration::from_millis(#timeout),
                #init,
                ::std::boxed::Box::pin(async move #block),
            )
        }
    };
    expanded.into()
}
