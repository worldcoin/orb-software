use heck::ToSnakeCase as _;
use proc_macro::TokenStream;
use proc_macro2::Span;
use quote::{format_ident, quote, quote_spanned};
use std::{collections::HashMap, hash::Hash};
use syn::{
    parse::{Parse, ParseStream, Result},
    parse_macro_input,
    punctuated::{Pair, Punctuated},
    spanned::Spanned,
    Data, DataStruct, DeriveInput, Expr, Field, Fields, FieldsNamed, Ident, Path,
    Token,
};

#[derive(Debug)]
struct AgentAttrSpanned {
    attr: AgentAttr,
    span: Span,
}

#[derive(Debug, PartialEq, Eq, Hash, Clone)]
enum AgentAttr {
    Task,
    Thread,
    Process,
    Init,
    InitAsync,
    Logger(Expr),
}

impl Parse for AgentAttrSpanned {
    fn parse(input: ParseStream) -> Result<Self> {
        let ident = input.parse::<Ident>()?;
        let attr = match ident.to_string().as_str() {
            "task" => AgentAttr::Task,
            "thread" => AgentAttr::Thread,
            "process" => AgentAttr::Process,
            "init" => AgentAttr::Init,
            "init_async" => AgentAttr::InitAsync,
            "logger" => {
                input.parse::<Token![=]>()?;
                AgentAttr::Logger(input.parse()?)
            }
            ident => {
                return Err(syn::Error::new_spanned(
                    ident,
                    "Unknown #[agent] option: {ident}",
                ))
            }
        };

        Ok(Self {
            attr,
            span: ident.span(),
        })
    }
}

#[derive(Debug)]
struct BrokerAttrSpanned {
    attr: BrokerAttr,
    span: Span,
}

#[derive(Debug, PartialEq, Eq, Hash, Clone)]
enum BrokerAttr {
    Plan(Path),
    Error(Path),
    PollExtra,
}

impl Parse for BrokerAttrSpanned {
    fn parse(input: ParseStream) -> Result<Self> {
        let ident = input.parse::<Ident>()?;
        let attr = match ident.to_string().as_str() {
            "plan" => {
                input.parse::<Token![=]>()?;
                BrokerAttr::Plan(input.parse()?)
            }
            "error" => {
                input.parse::<Token![=]>()?;
                BrokerAttr::Error(input.parse()?)
            }
            "poll_extra" => BrokerAttr::PollExtra,
            ident => {
                return Err(syn::Error::new(
                    ident.span(),
                    "Unknown #[broker] option: {ident}",
                ))
            }
        };

        Ok(Self {
            attr,
            span: ident.span(),
        })
    }
}

#[allow(clippy::too_many_lines)]
pub fn proc_macro_derive(input: TokenStream) -> TokenStream {
    let DeriveInput {
        attrs, ident, data, ..
    } = parse_macro_input!(input);
    let Data::Struct(DataStruct { fields, .. }) = data else {
        panic!("must be a struct")
    };
    let Fields::Named(FieldsNamed { named: fields, .. }) = fields else {
        panic!("must have named fields")
    };

    let broker_attrs = attrs
        .iter()
        .find(|attr| attr.path().is_ident("broker"))
        .expect("must have a `#[broker]` attribute")
        .parse_args_with(Punctuated::<BrokerAttrSpanned, Token![,]>::parse_terminated)
        .expect("failed to parse `broker` attribute")
        .into_pairs()
        .map(Pair::into_value)
        .map(|broker_attr_spanned| {
            (broker_attr_spanned.attr.clone(), broker_attr_spanned)
        })
        .collect::<HashMap<BrokerAttr, BrokerAttrSpanned>>();
    let broker_plan = broker_attrs
        .iter()
        .find_map(|(attr, _spanned)| {
            if let BrokerAttr::Plan(expr) = attr {
                Some(expr)
            } else {
                None
            }
        })
        .expect("#[broker] attribute must set a `plan`");
    let broker_error = broker_attrs
        .iter()
        .find_map(|(attr, _spanned)| {
            if let BrokerAttr::Error(expr) = attr {
                Some(expr)
            } else {
                None
            }
        })
        .expect("#[broker] attribute must set an `error`");

    let agent_fields = fields.iter().filter_map(|field| {
        field
            .attrs
            .iter()
            .find(|attr| attr.path().is_ident("agent"))
            .map(|attr| {
                let attrs = attr
                    .parse_args_with(
                        Punctuated::<AgentAttrSpanned, Token![,]>::parse_terminated,
                    )
                    .expect("failed to parse `agent` attribute");
                (
                    field,
                    attrs
                        .into_pairs()
                        .map(Pair::into_value)
                        .map(|agent_attr_spanned| {
                            (agent_attr_spanned.attr.clone(), agent_attr_spanned)
                        })
                        .collect::<HashMap<AgentAttr, AgentAttrSpanned>>(),
                )
            })
    });

    let constructor_name = format_ident!("new_{}", ident.to_string().to_snake_case());
    let constructor_fields = agent_fields.clone().map(
        |(Field { ident, .. }, _)| quote!(#ident: ::agentwire::agent::Cell::Vacant),
    );
    let constructor = quote! {
        macro_rules! #constructor_name {
            ($($tokens:tt)*) => {
                #ident {
                    #(#constructor_fields,)*
                    $($tokens)*
                }
            };
        }
    };

    let run_fut_name = format_ident!("Run{}", ident);
    let run_handlers = agent_fields.clone().map(|(field, _)| {
        let ident = field.ident.as_ref().unwrap();
        let handler = format_ident!("handle_{}", ident);
        quote! {
            if let Some(port) = fut.broker.#ident.enabled() {
                any_handler_enabled |= true;
                loop {
                    match ::futures::StreamExt::poll_next_unpin(port, cx) {
                        // check if message is newer than fence
                        ::std::task::Poll::Ready(Some(output)) if output.source_ts > fence => {
                            match fut.broker.#handler(fut.plan, output) {
                                ::std::result::Result::Ok(::agentwire::BrokerFlow::Break) => {
                                    return ::std::task::Poll::Ready(::std::result::Result::Ok(()));
                                }
                                ::std::result::Result::Ok(::agentwire::BrokerFlow::Continue) => {
                                    continue 'outer;
                                }
                                ::std::result::Result::Err(err) => {
                                    return ::std::task::Poll::Ready(
                                        ::std::result::Result::Err(
                                            ::agentwire::BrokerError::Handler(
                                                ::std::stringify!(#ident),
                                                err,
                                            ),
                                        ),
                                    );
                                }
                            }
                        }
                        ::std::task::Poll::Ready(::std::option::Option::Some(_)) => {
                            continue; // skip message because its older than `fence`
                        }
                        ::std::task::Poll::Ready(::std::option::Option::None) => {
                            // channel sender is dropped, which means agent terminated
                            return ::std::task::Poll::Ready(
                                ::std::result::Result::Err(
                                    ::agentwire::BrokerError::AgentTerminated(
                                        ::std::stringify!(#ident),
                                    ),
                                ),
                            );
                        }
                        ::std::task::Poll::Pending => {
                            break; // No more messages to process
                        }
                    }
                }
            }
        }
    });
    let poll_extra = broker_attrs.get(&BrokerAttr::PollExtra).map(
        |broker_attr_spanned| {
            quote_spanned! { broker_attr_spanned.span =>
                match fut.broker.poll_extra(fut.plan, cx, fence) {
                    ::std::result::Result::Ok(::std::option::Option::Some(poll)) => {
                        break poll.map(Ok);
                    }
                    ::std::result::Result::Ok(::std::option::Option::None) => {
                        continue;
                    }
                    ::std::result::Result::Err(err) => {
                        return ::std::task::Poll::Ready(::std::result::Result::Err(
                            ::agentwire::BrokerError::PollExtra(err),
                        ));
                    }
                }
            }
        },
    );
    let run = quote! {
        /// Future for [`#ident::run`].
        pub struct #run_fut_name<'a> {
            broker: &'a mut #ident,
            plan: &'a mut dyn #broker_plan,
            fence: ::std::time::Instant,
        }

        impl ::futures::future::Future for #run_fut_name<'_> {
            type Output = ::std::result::Result<(), ::agentwire::BrokerError<#broker_error>>;

            fn poll(
                mut self: ::std::pin::Pin<&mut Self>,
                cx: &mut ::std::task::Context<'_>,
            ) -> ::std::task::Poll<Self::Output> {
                let fence = self.fence;
                let fut = self.as_mut().get_mut();
                let mut any_handler_enabled = false;
                'outer: loop {
                    #(#run_handlers)*
                    #poll_extra
                    #[allow(unreachable_code)]
                    if !any_handler_enabled {
                        // Prevent infinite loop in edge case where no handlers are
                        // enabled.
                        return ::std::task::Poll::Pending;
                    }
                }

            }
        }

        impl #ident {
            /// Equivalent to [`Self::run_with_fence()`] with a fence of `Instant::now()`.
            pub fn run<'a>(&'a mut self, plan: &'a mut dyn #broker_plan) -> #run_fut_name<'a> {
                Self::run_with_fence(self, plan, ::std::time::Instant::now())
            }

            /// Runs the broker, filtering any events to only those with a timestamp
            /// newer than `fence`.
            ///
            /// Events are fed the broker's `handle_*` functions, and `plan` is passed
            /// there as an argument.
            pub fn run_with_fence<'a>(
                &'a mut self,
                plan: &'a mut dyn #broker_plan,
                fence: ::std::time::Instant,
            ) -> #run_fut_name<'a> {
                #run_fut_name {
                    broker: self,
                    plan,
                    fence,
                }
            }
        }
    };

    let methods = agent_fields.clone().map(|(field, attrs)| {
        let ident = field.ident.as_ref().unwrap();
        let enable = format_ident!("enable_{}", ident);
        let try_enable = format_ident!("try_enable_{}", ident);
        let disable = format_ident!("disable_{}", ident);
        let init = format_ident!("init_{}", ident);
        let (init, init_async) = if let Some(attr) = attrs.get(&AgentAttr::InitAsync) {
        let span = attr.span;
            let init = quote_spanned! { span =>
                match self.#init().await {
                    ::std::result::Result::Ok(agent) => agent,
                    ::std::result::Result::Err(err) => {
                        return ::std::result::Result::Err(
                            ::agentwire::BrokerError::Init(::std::stringify!(#ident), err),
                        );
                    }
                }
            };
            (init, quote_spanned!(span => async))
        } else if let Some(attr) = attrs.get(&AgentAttr::Init) {
            (quote_spanned!(attr.span => self.#init()), quote_spanned!(attr.span => ))
        } else {
            (quote_spanned!(field.span() => Default::default()), quote!())
        };
        let constructor = if let Some(attr) = attrs.get(&AgentAttr::Process) {
            let logger = if let Some(logger) = attrs
                .iter()
                .find_map(|(attr, _)| if let AgentAttr::Logger(expr) = attr{ Some(expr) } else { None })
            {
                quote_spanned!(attr.span => #logger)
            } else {
                quote_spanned!(attr.span => ::agentwire::agent::process::default_logger)
            };
            quote_spanned!(attr.span => ::agentwire::agent::Process::spawn_process(#init, #logger))
        } else if let Some(attr) = attrs.get(&AgentAttr::Thread) {
            quote_spanned! { attr.span =>
                match ::agentwire::agent::Thread::spawn_thread(#init) {
                    ::std::result::Result::Ok(cell) => cell,
                    ::std::result::Result::Err(err) => {
                        return ::std::result::Result::Err(
                            ::agentwire::BrokerError::SpawnThread(::std::stringify!(#ident), err)
                        );
                    }
                }
            }
        } else if let Some(attr) = attrs.get(&AgentAttr::Task) {
            quote_spanned!(attr.span => ::agentwire::agent::Task::spawn_task(#init))
        } else {
            return syn::Error::new(field.span(), "must have `task`, `thread`, or `process` tag").to_compile_error();
        };

        quote_spanned! { field.span() =>
            #[allow(missing_docs)]
            pub #init_async fn #enable(
                &mut self,
            ) -> ::std::result::Result<(), ::agentwire::BrokerError<#broker_error>> {
                match ::std::mem::replace(&mut self.#ident, ::agentwire::agent::Cell::Vacant) {
                    ::agentwire::agent::Cell::Vacant => {
                        self.#ident = ::agentwire::agent::Cell::Enabled(#constructor);
                    }
                    ::agentwire::agent::Cell::Enabled(agent)
                    | ::agentwire::agent::Cell::Disabled(agent) => {
                        self.#ident = ::agentwire::agent::Cell::Enabled(agent);
                    }
                }
                ::std::result::Result::Ok(())
            }

            #[allow(missing_docs)]
            pub fn #try_enable(&mut self) {
                match ::std::mem::replace(&mut self.#ident, ::agentwire::agent::Cell::Vacant) {
                    ::agentwire::agent::Cell::Vacant => {}
                    ::agentwire::agent::Cell::Enabled(agent)
                    | ::agentwire::agent::Cell::Disabled(agent) => {
                        self.#ident = ::agentwire::agent::Cell::Enabled(agent);
                    }
                }
            }

            #[allow(missing_docs)]
            pub fn #disable(&mut self) {
                match ::std::mem::replace(&mut self.#ident, ::agentwire::agent::Cell::Vacant) {
                    ::agentwire::agent::Cell::Vacant => {}
                    ::agentwire::agent::Cell::Enabled(agent)
                    | ::agentwire::agent::Cell::Disabled(agent) => {
                        self.#ident = ::agentwire::agent::Cell::Disabled(agent);
                    }
                }
            }
        }
    });

    let disable_agents = agent_fields.map(|(field, _)| {
        let disable = format_ident!("disable_{}", field.ident.as_ref().unwrap());
        quote_spanned!(field.span() => #disable)
    });

    let expanded = quote! {
        #constructor
        #run

        impl #ident {
            #(#methods)*

            #[allow(missing_docs)]
            pub fn disable_agents(&mut self) {
                #(self.#disable_agents();)*
            }
        }
    };
    expanded.into()
}
