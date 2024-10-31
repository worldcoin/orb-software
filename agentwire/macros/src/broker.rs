use heck::ToSnakeCase as _;
use proc_macro::TokenStream;
use quote::{format_ident, quote};
use std::collections::HashSet;
use syn::{
    parse::{Parse, ParseStream, Result},
    parse_macro_input,
    punctuated::{Pair, Punctuated},
    Data, DataStruct, DeriveInput, Expr, Field, Fields, FieldsNamed, Ident, Path, Token,
};

#[derive(PartialEq, Eq, Hash)]
enum AgentAttr {
    Task,
    Thread,
    Process,
    Init,
    InitAsync,
    Logger(Expr),
}

impl Parse for AgentAttr {
    fn parse(input: ParseStream) -> Result<Self> {
        let ident = input.parse::<Ident>()?;
        match ident.to_string().as_str() {
            "task" => Ok(Self::Task),
            "thread" => Ok(Self::Thread),
            "process" => Ok(Self::Process),
            "init" => Ok(Self::Init),
            "init_async" => Ok(Self::InitAsync),
            "logger" => {
                input.parse::<Token![=]>()?;
                Ok(Self::Logger(input.parse()?))
            }
            ident => panic!("Unknown #[agent] option: {ident}"),
        }
    }
}

#[derive(PartialEq, Eq, Hash)]
enum BrokerAttr {
    Plan(Path),
    Error(Path),
    PollExtra,
}

impl Parse for BrokerAttr {
    fn parse(input: ParseStream) -> Result<Self> {
        let ident = input.parse::<Ident>()?;
        match ident.to_string().as_str() {
            "plan" => {
                input.parse::<Token![=]>()?;
                Ok(Self::Plan(input.parse()?))
            }
            "error" => {
                input.parse::<Token![=]>()?;
                Ok(Self::Error(input.parse()?))
            }
            "poll_extra" => Ok(Self::PollExtra),
            ident => panic!("Unknown #[broker] option: {ident}"),
        }
    }
}

#[allow(clippy::too_many_lines)]
pub fn proc_macro_derive(input: TokenStream) -> TokenStream {
    let DeriveInput { attrs, ident, data, .. } = parse_macro_input!(input);
    let Data::Struct(DataStruct { fields, .. }) = data else { panic!("must be a struct") };
    let Fields::Named(FieldsNamed { named: fields, .. }) = fields else {
        panic!("must have named fields")
    };

    let broker_attrs = attrs
        .iter()
        .find(|attr| attr.path().is_ident("broker"))
        .expect("must have a `#[broker]` attribute")
        .parse_args_with(Punctuated::<BrokerAttr, Token![,]>::parse_terminated)
        .expect("failed to parse `broker` attribute")
        .into_pairs()
        .map(Pair::into_value)
        .collect::<HashSet<_>>();
    let broker_plan = broker_attrs
        .iter()
        .find_map(|attr| if let BrokerAttr::Plan(expr) = attr { Some(expr) } else { None })
        .expect("#[broker] attribute must set a `plan`");
    let broker_error = broker_attrs
        .iter()
        .find_map(|attr| if let BrokerAttr::Error(expr) = attr { Some(expr) } else { None })
        .expect("#[broker] attribute must set an `error`");

    let agent_fields = fields.iter().filter_map(|field| {
        field.attrs.iter().find(|attr| attr.path().is_ident("agent")).map(|attr| {
            let attrs = attr
                .parse_args_with(Punctuated::<AgentAttr, Token![,]>::parse_terminated)
                .expect("failed to parse `agent` attribute");
            (field, attrs.into_pairs().map(Pair::into_value).collect::<HashSet<_>>())
        })
    });

    let constructor_name = format_ident!("new_{}", ident.to_string().to_snake_case());
    let constructor_fields = agent_fields
        .clone()
        .map(|(Field { ident, .. }, _)| quote!(#ident: ::agentwire::agent::Cell::Vacant));
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
                loop {
                    match ::futures::StreamExt::poll_next_unpin(port, cx) {
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
                            continue;
                        }
                        ::std::task::Poll::Ready(::std::option::Option::None) => {
                            return ::std::task::Poll::Ready(
                                ::std::result::Result::Err(
                                    ::agentwire::BrokerError::AgentTerminated(
                                        ::std::stringify!(#ident),
                                    ),
                                ),
                            );
                        }
                        ::std::task::Poll::Pending => {
                            break;
                        }
                    }
                }
            }
        }
    });
    let poll_extra = broker_attrs.contains(&BrokerAttr::PollExtra).then(|| {
        quote! {
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
    });
    let run = quote! {
        #[allow(missing_docs)]
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
                'outer: loop {
                    #(#run_handlers)*
                    #poll_extra
                }
            }
        }

        impl #ident {
            #[allow(missing_docs)]
            pub fn run<'a>(&'a mut self, plan: &'a mut dyn #broker_plan) -> #run_fut_name<'a> {
                Self::run_with_fence(self, plan, ::std::time::Instant::now())
            }

            #[allow(missing_docs)]
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
        let (init, init_async) = if attrs.contains(&AgentAttr::InitAsync) {
            let init = quote! {
                match self.#init().await {
                    ::std::result::Result::Ok(agent) => agent,
                    ::std::result::Result::Err(err) => {
                        return ::std::result::Result::Err(
                            ::agentwire::BrokerError::Init(::std::stringify!(#ident), err),
                        );
                    }
                }
            };
            (init, quote!(async))
        } else if attrs.contains(&AgentAttr::Init) {
            (quote!(self.#init()), quote!())
        } else {
            (quote!(Default::default()), quote!())
        };
        let constructor = if attrs.contains(&AgentAttr::Process) {
            let logger = if let Some(logger) = attrs
                .iter()
                .find_map(|attr| if let AgentAttr::Logger(expr) = attr { Some(expr) } else { None })
            {
                quote!(#logger)
            } else {
                quote!(::agentwire::agent::process::default_logger)
            };
            quote!(::agentwire::agent::Process::spawn_process(#init, #logger))
        } else if attrs.contains(&AgentAttr::Thread) {
            quote! {
                match ::agentwire::agent::Thread::spawn_thread(#init) {
                    ::std::result::Result::Ok(cell) => cell,
                    ::std::result::Result::Err(err) => {
                        return ::std::result::Result::Err(
                            ::agentwire::BrokerError::SpawnThread(::std::stringify!(#ident), err)
                        );
                    }
                }
            }
        } else if attrs.contains(&AgentAttr::Task) {
            quote!(::agentwire::agent::Task::spawn_task(#init))
        } else {
            panic!("must have `task`, `thread`, or `process` tag");
        };

        quote! {
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
        quote!(#disable)
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
