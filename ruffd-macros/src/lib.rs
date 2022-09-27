use proc_macro::{self, TokenStream};
use proc_macro2::Span;
use proc_macro_error::{abort, proc_macro_error, Diagnostic, Level};
use quote::{quote, ToTokens};
use syn::{parse_macro_input, parse_quote, FnArg, Ident, ItemFn, Pat, PatIdent, PatType, Stmt};

struct FnDetails {
    asyncness: bool,
    fn_identifier: Ident,
    parameter: Option<PatType>,
}

impl FnDetails {
    fn from_item_fn(input: &ItemFn) -> Self {
        let params = &input.sig.inputs;
        let mut params_iter = params.iter().cloned();
        let parameter = params_iter.next().map(|param| match param {
            FnArg::Receiver(_) => {
                abort!(Diagnostic::new(
                    Level::Error,
                    "self parameter disallowed".to_string()
                ));
            }
            FnArg::Typed(x) => x,
        });
        if params_iter.next().is_some() {
            abort!(Diagnostic::new(
                Level::Error,
                "Exactly one or zero parameters allowed".to_string()
            ));
        }
        let fn_identifier = input.sig.ident.clone();
        let asyncness = input.sig.asyncness.is_some();
        Self {
            asyncness,
            fn_identifier,
            parameter,
        }
    }
}

/// Wraps the TokenStream in parentheses such that a comma separated list of patterns
/// can be parsed as a tuple pattern
fn wrap_args(args: TokenStream) -> TokenStream {
    let args = proc_macro2::TokenStream::from(args);
    quote! {(#args)}.into()
}

/// Parses expected tuple pattern into a vector of pattern identifiers
fn make_state_members(pattern: Pat) -> Vec<PatIdent> {
    match pattern {
        Pat::Tuple(x) => x
            .elems
            .into_iter()
            .map(|x| match x {
                Pat::Ident(ident) => ident,
                _ => {
                    abort!(Diagnostic::new(
                        Level::Error,
                        "Expected identifiers only in args".to_string()
                    ))
                }
            })
            .collect(),
        _ => {
            abort!(Diagnostic::new(
                Level::Error,
                "Expected tuple destructor-like elements".to_string()
            ));
        }
    }
}

fn make_create_locks_fn(members: &[PatIdent]) -> impl ToTokens {
    let statements = {
        let statement_iter = members.iter().map(|member| -> Stmt {
            let ident = &member.ident;
            let rhs = if member.mutability.is_some() {
                quote!(::ruffd_types::RwReq::Write(state.#ident.clone()))
            } else {
                quote!(::ruffd_types::RwReq::Read(state.#ident.clone()))
            };
            parse_quote!(rv.#ident = Some(#rhs);)
        });
        quote!(#(#statement_iter)*)
    };
    quote! {
        fn create_locks(
            state: ::std::sync::Arc<::ruffd_types::tokio::sync::Mutex<::ruffd_types::ServerState>>,
        ) -> ::std::pin::Pin<
            Box<
                dyn Send + ::std::future::Future<Output = ::ruffd_types::ServerStateLocks>
            >
        >
        {
            Box::pin(async move {
                let mut rv = ::ruffd_types::ServerStateLocks::default();
                let state = state.lock().await;
                #statements
                rv
            })
        }
    }
}

/// Creates statements to setup variables corresponding to the required state members
/// inclusive of mutability
fn make_setup_state(members: &[PatIdent]) -> impl ToTokens {
    let statements = members.iter().map(|member| -> Stmt {
        let ident = &member.ident;
        let mutability = &member.mutability;
        let guard_type = if mutability.is_some() {
            quote!(Write)
        } else {
            quote!(Read)
        };
        parse_quote! {
            let #mutability #ident = match state.#ident.unwrap() {
                ::ruffd_types::RwGuarded::#guard_type(x) => x,
                _ => unreachable!(),
            };
        }
    });
    quote!(#(#statements)*)
}

/// Creates augmented inner function to execute
fn make_inner_fn(func: &ItemFn, members: &[PatIdent]) -> impl ToTokens {
    let sig = {
        let mut rv = func.sig.clone();
        rv.ident = Ident::new("inner", Span::call_site());
        let old_inputs = rv.inputs;
        rv.inputs = parse_quote!(state: ::ruffd_types::ServerStateHandles<'_>, #old_inputs);
        rv
    };
    let block = func.block.clone();
    let setup_state = make_setup_state(members);
    quote! {
        #sig {
            #setup_state
            #block
        }
    }
}

fn make_params_check(param: PatType, is_notification: bool) -> impl ToTokens {
    let error_return = if is_notification {
        quote!(Some(::ruffd_types::RpcResponse::from_error(None, err)))
    } else {
        quote!(::ruffd_types::RpcResponse::from_error(Some(id), err))
    };
    let param_type = param.ty;
    quote! {
        let params_result: Result<#param_type, ::ruffd_types::RpcError> = match params {
            None => Err(::ruffd_types::RpcErrors::INVALID_PARAMS),
            Some(x) => {
                ::ruffd_types::serde_json::from_value(x).map_err(|e| e.into())
            }
        };
        let params = match params_result {
            Err(err) => return #error_return,
            Ok(x) => x,
        };
    }
}

#[proc_macro_error]
#[proc_macro_attribute]
pub fn notification(args: TokenStream, stream: TokenStream) -> TokenStream {
    let args = wrap_args(args);
    let state_members = make_state_members(parse_macro_input!(args as Pat));
    let create_locks_fn = make_create_locks_fn(&state_members);
    let input = parse_macro_input!(stream as ItemFn);
    let fn_details = FnDetails::from_item_fn(&input);
    let inner_fn = make_inner_fn(&input, &state_members);
    let params_check = fn_details
        .parameter
        .clone()
        .map(|x| make_params_check(x, true));
    let params_ident = if fn_details.parameter.is_some() {
        quote!(params)
    } else {
        quote!(_params)
    };
    let inner_call_params = fn_details.parameter.clone().map(|_| quote!(params));
    let inner_await = fn_details.asyncness.then(|| quote!(.await));
    let fn_identifier = fn_details.fn_identifier;
    let id_check = quote! {
        if let Some(x) = id {
            return Some(::ruffd_types::RpcResponse::from_error(
                Some(x),
                ::ruffd_types::RpcErrors::INVALID_REQUEST
            ));
        }
    };
    quote! {
        #[allow(dead_code)]
        mod #fn_identifier {
            use super::*;
            #inner_fn
            #create_locks_fn
            fn exec(
                state: ::ruffd_types::ServerStateHandles<'_>,
                id: Option<::ruffd_types::lsp_types::NumberOrString>,
                #params_ident: Option<::ruffd_types::serde_json::Value>,
            ) -> ::std::pin::Pin<
                Box<
                    dyn Send + ::std::future::Future<
                        Output = Option<::ruffd_types::RpcResponse>
                    > + '_
                >
            >
            {
                Box::pin(async move {
                    #id_check
                    #params_check
                    let rv = inner(state, #inner_call_params)#inner_await;
                    match rv {
                        Ok(_) => None,
                        Err(e) => Some(
                            ::ruffd_types::RpcResponse::from_error(
                                None,
                                ::ruffd_types::RpcError::from(e)
                            )
                        )
                    }
                })
            }

            #[allow(non_upper_case_globals)]
            pub const #fn_identifier: ::ruffd_types::Notification = ::ruffd_types::Notification {
                exec,
                create_locks,
            };
        }
        #[allow(unused_imports)]
        use #fn_identifier::#fn_identifier;
    }
    .into()
}

#[cfg(test)]
mod test {
    #[test]
    fn test_notification() {
        let t = trybuild::TestCases::new();
        t.compile_fail("tests/notification/*.rs");
    }
}
