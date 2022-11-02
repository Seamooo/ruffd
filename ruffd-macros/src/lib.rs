use convert_case::{Case, Casing};
use proc_macro::{self, TokenStream};
use proc_macro2::Span;
use proc_macro_error::{abort, proc_macro_error, Diagnostic, Level};
use quote::{quote, ToTokens};
use syn::{
    parse_macro_input, parse_quote, AttributeArgs, Fields, FnArg, GenericParam, Ident, Index,
    ItemFn, ItemStruct, Lit, Meta, NestedMeta, Pat, PatIdent, PatType, Stmt, Token, Type,
};

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
fn wrap_tuple_args(args: TokenStream) -> TokenStream {
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
        rv.inputs = parse_quote!(
            state: ::ruffd_types::ServerStateHandles<'_>,
            _scheduler_channel: ::ruffd_types::tokio::sync::mpsc::Sender<
                ::ruffd_types::ScheduledTask
            >,
            #old_inputs);
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
        quote!(Some(::ruffd_types::RpcResponseMessage::from_error(
            None, err
        )))
    } else {
        quote!(::ruffd_types::RpcResponseMessage::from_error(Some(id), err))
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
    let args = wrap_tuple_args(args);
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
    quote! {
        #[allow(dead_code)]
        mod #fn_identifier {
            use super::*;
            #inner_fn
            #create_locks_fn
            fn exec(
                state: ::ruffd_types::ServerStateHandles<'_>,
                scheduler_channel: ::ruffd_types::tokio::sync::mpsc::Sender<
                    ::ruffd_types::ScheduledTask
                >,
                #params_ident: Option<::ruffd_types::serde_json::Value>,
            ) -> ::std::pin::Pin<
                Box<
                    dyn Send + ::std::future::Future<
                        Output = Option<::ruffd_types::RpcResponseMessage>
                    > + '_
                >
            >
            {
                Box::pin(async move {
                    #params_check
                    let rv = inner(state, scheduler_channel, #inner_call_params)#inner_await;
                    match rv {
                        Ok(_) => None,
                        Err(e) => Some(
                            ::ruffd_types::RpcResponseMessage::from_error(
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

/// Macro for constructing a function to add to the request
/// registry. This constructs a module with the same identifier
/// as the input function, and exports the internal function with
/// the modified interface to work with the request registry.
///
/// # Arguments
///
/// It is possible to add arguments to this attribute that define
/// locks that will be acquired from `ruffd_types::ServerState`
/// prior to request execution. These arguments appear as tuple
/// matching patterns e.g. `#[request(mut open_buffers)]` will acquire
/// the field `open_buffers` with a write lock prior to execution
#[proc_macro_error]
#[proc_macro_attribute]
pub fn request(args: TokenStream, stream: TokenStream) -> TokenStream {
    let args = wrap_tuple_args(args);
    let state_members = make_state_members(parse_macro_input!(args as Pat));
    let create_locks_fn = make_create_locks_fn(&state_members);
    let input = parse_macro_input!(stream as ItemFn);
    let fn_details = FnDetails::from_item_fn(&input);
    let inner_fn = make_inner_fn(&input, &state_members);
    let params_check = fn_details
        .parameter
        .clone()
        .map(|x| make_params_check(x, false));
    let params_ident = if fn_details.parameter.is_some() {
        quote!(params)
    } else {
        quote!(_params)
    };
    let inner_call_params = fn_details.parameter.clone().map(|_| quote!(params));
    let inner_await = fn_details.asyncness.then(|| quote!(.await));
    let fn_identifier = fn_details.fn_identifier;
    quote! {
        #[allow(dead_code)]
        mod #fn_identifier {
            use super::*;
            #inner_fn
            #create_locks_fn
            fn exec(
                state: ::ruffd_types::ServerStateHandles<'_>,
                scheduler_channel: ::ruffd_types::tokio::sync::mpsc::Sender<
                    ::ruffd_types::ScheduledTask
                >,
                id: ::ruffd_types::lsp_types::NumberOrString,
                #params_ident: Option<::ruffd_types::serde_json::Value>,
            ) -> ::std::pin::Pin<
                Box<
                    dyn Send + ::std::future::Future<
                        Output = ::ruffd_types::RpcResponseMessage
                    > + '_
                >
            >
            {
                Box::pin(async move {
                    #params_check
                    let rv = inner(state, scheduler_channel, #inner_call_params)#inner_await;
                    match rv {
                        Ok(val) => ::ruffd_types::RpcResponseMessage::from_result(
                            id,
                            val,
                        ),
                        Err(e) => ::ruffd_types::RpcResponseMessage::from_error(
                            Some(id),
                            ::ruffd_types::RpcError::from(e)
                        ),

                    }
                })
            }

            #[allow(non_upper_case_globals)]
            pub const #fn_identifier: ::ruffd_types::Request = ::ruffd_types::Request {
                exec,
                create_locks,
            };
        }
        #[allow(unused_imports)]
        use #fn_identifier::#fn_identifier;
    }
    .into()
}

fn wrap_rw_fields(item: &mut ItemStruct, flags: &ServerStateFlags) {
    let fields = match &mut item.fields {
        Fields::Named(x) => Some(&mut x.named),
        Fields::Unnamed(x) => Some(&mut x.unnamed),
        Fields::Unit => None,
    };
    if let Some(fields) = fields {
        for field in fields.iter_mut() {
            let inner_ty = &field.ty;
            let new_ty: Type = if flags.in_ruffd_types {
                parse_quote!(::std::sync::Arc<::tokio::sync::RwLock<#inner_ty>>)
            } else {
                parse_quote!(::std::sync::Arc<::ruffd_types::tokio::sync::RwLock<#inner_ty>>)
            };
            field.ty = new_ty;
        }
    }
}

fn make_handle_struct(item: &mut ItemStruct, flags: &ServerStateFlags) {
    let ident_prefix = item.ident.to_string();
    item.ident = Ident::new(&format!("{}Handles", ident_prefix), Span::call_site());
    let guard_lifetime: GenericParam = parse_quote!('guard);
    item.generics.params.insert(0, guard_lifetime);
    item.generics.lt_token = Some(<Token![<]>::default());
    item.generics.gt_token = Some(<Token![>]>::default());
    let fields = match &mut item.fields {
        Fields::Named(x) => Some(&mut x.named),
        Fields::Unnamed(x) => Some(&mut x.unnamed),
        Fields::Unit => None,
    };
    if let Some(fields) = fields {
        for field in fields.iter_mut() {
            let inner_ty = &field.ty;
            let new_ty: Type = if flags.in_ruffd_types {
                parse_quote!(Option<crate::state::RwGuarded<'guard, #inner_ty>>)
            } else {
                parse_quote!(Option<::ruffd_types::RwGuarded<'guard, #inner_ty>>)
            };
            field.ty = new_ty;
        }
    }
    item.attrs = vec![];
}

fn make_lock_req_struct(item: &mut ItemStruct, flags: &ServerStateFlags) {
    let ident_prefix = item.ident.to_string();
    item.ident = Ident::new(&format!("{}Locks", ident_prefix), Span::call_site());
    let fields = match &mut item.fields {
        Fields::Named(x) => Some(&mut x.named),
        Fields::Unnamed(x) => Some(&mut x.unnamed),
        Fields::Unit => None,
    };
    if let Some(fields) = fields {
        for field in fields.iter_mut() {
            let inner_ty = &field.ty;
            let new_ty: Type = if flags.in_ruffd_types {
                parse_quote!(Option<crate::state::RwReq<#inner_ty>>)
            } else {
                parse_quote!(Option<::ruffd_types::RwReq<#inner_ty>>)
            };
            field.ty = new_ty;
        }
    }
    item.attrs = vec![parse_quote!(#[derive(Default)])];
}

fn make_lock_to_handle_func(item: &ItemStruct) -> impl ToTokens {
    let ident_prefix = item.ident.to_string();
    let func_ident = Ident::new(
        format!("{}_handles_from_locks", ident_prefix.to_case(Case::Snake)).as_str(),
        Span::call_site(),
    );
    let locks_ty = Ident::new(format!("{}Locks", ident_prefix).as_str(), Span::call_site());
    let handles_ty = Ident::new(
        format!("{}Handles", ident_prefix).as_str(),
        Span::call_site(),
    );
    let (statements, return_expr) = match &item.fields {
        Fields::Named(fields) => {
            let variable_idents = fields
                .named
                .iter()
                .map(|field| field.ident.as_ref().unwrap().clone())
                .collect::<Vec<_>>();
            let statements = variable_idents
                .iter()
                .map(|field_ident| {
                    quote! {
                        let #field_ident = match &locks.#field_ident {
                            Some(x) => Some(x.lock().await),
                            None => None,
                        };
                    }
                })
                .collect::<Vec<_>>();
            let variable_idents_iter = variable_idents.iter();
            let return_expr = quote! {
                #handles_ty {
                    #(#variable_idents_iter),*
                }
            };
            (statements, return_expr)
        }
        Fields::Unnamed(fields) => {
            let variable_idents = fields
                .unnamed
                .iter()
                .enumerate()
                .map(|(idx, _)| Ident::new(format!("var_{}", idx).as_str(), Span::call_site()))
                .collect::<Vec<_>>();
            let statements = variable_idents
                .iter()
                .enumerate()
                .map(|(idx, var_name)| {
                    let field_idx = Index::from(idx);
                    quote! {
                        let #var_name = match &locks.#field_idx {
                            Some(x) => Some(x.lock().await),
                            None => None,
                        };
                    }
                })
                .collect::<Vec<_>>();
            let variable_idents_iter = variable_idents.iter();
            let return_expr = quote!(#handles_ty(#(#variable_idents_iter),*));
            (statements, return_expr)
        }
        Fields::Unit => (vec![quote!()], quote!(#handles_ty())),
    };
    let statements_iter = statements.iter();
    quote! {
        pub async fn #func_ident(locks: &#locks_ty) -> #handles_ty<'_>
        {
            #(#statements_iter)*
            #return_expr
        }
    }
}

#[derive(Default)]
struct ServerStateFlags {
    in_ruffd_types: bool,
}

impl ServerStateFlags {
    fn from_attribute_args(args: &AttributeArgs) -> Self {
        let mut rv = Self::default();
        let in_ruffd_types_ident = Ident::new("in_ruffd_types", Span::call_site());
        for arg in args.iter() {
            if let NestedMeta::Meta(Meta::NameValue(name_value)) = arg {
                if name_value.path.is_ident(&in_ruffd_types_ident) {
                    if let Lit::Bool(x) = &name_value.lit {
                        rv.in_ruffd_types = x.value;
                    }
                }
            }
        }
        rv
    }
}

/// Transforms an input struct into 3 new structs and a convenience
/// async function
///
/// `<Ident>` will contain the same struct with all fields having
/// public access and wrapped by `Arc<tokio::sync::RwLock<T>>`
///
/// `<Ident>Locks` will contain fields with corresponding identifiers to
/// the original struct, wrapped with `Option<ruffd_types::state::RwReq<T>>`
///
/// `<Ident>Handles` will contain fields with corresponding identifiers to the original struct,
/// wrapped with `Option<ruffd_types::state::RwGuarded<'guard,T>>`
///
/// `<Ident:snake_case>_handles_from_locks` will construct an `<Ident>Handles`
/// type from a reference to `<Ident>Locks`
///
/// # Arguments
///
/// Use `#[server_state(in_ruffd_types = true)]` for use inside the ruffd_types crate
#[proc_macro_error]
#[proc_macro_attribute]
pub fn server_state(args: TokenStream, stream: TokenStream) -> TokenStream {
    let input_struct = parse_macro_input!(stream as ItemStruct);
    let input_args = parse_macro_input!(args as AttributeArgs);
    let flags = ServerStateFlags::from_attribute_args(&input_args);
    let lock_wrapped_struct = {
        let mut rv = input_struct.clone();
        wrap_rw_fields(&mut rv, &flags);
        rv
    };
    let handle_struct = {
        let mut rv = input_struct.clone();
        make_handle_struct(&mut rv, &flags);
        rv
    };
    let lock_req_struct = {
        let mut rv = input_struct.clone();
        make_lock_req_struct(&mut rv, &flags);
        rv
    };
    let convenience_func = make_lock_to_handle_func(&input_struct);
    quote! {
        #lock_wrapped_struct
        #handle_struct
        #lock_req_struct
        #convenience_func
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
    #[test]
    fn test_server_state() {
        let t = trybuild::TestCases::new();
        // NOTE tests do not include the flag "in_ruffd_types" as it
        // is only possible to test inside ruffd_types
        t.compile_fail("tests/server_state/*.rs");
    }
}
