//! Procedural macros for agentwire.

#![warn(unsafe_op_in_unsafe_fn)]
#![warn(clippy::pedantic)]

extern crate proc_macro;

mod broker;
mod test;

use proc_macro::TokenStream;

#[proc_macro_derive(Broker, attributes(broker, agent))]
pub fn derive_broker(input: TokenStream) -> TokenStream {
    broker::proc_macro_derive(input)
}

#[proc_macro_attribute]
pub fn test(attr: TokenStream, item: TokenStream) -> TokenStream {
    test::proc_macro_attribute(attr, item)
}
