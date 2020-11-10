//! Macros for use with Tokio
extern crate proc_macro;

use proc_macro::TokenStream;

use quote::quote;

// Marks async function to be wrapped in logging calls.
//
// ## Usage
//
// ```rust
// #[api_macros::log_api_call]
// async fn api_method(serv, body, identity) {
//     ...method body...
// }
// ```
#[proc_macro_attribute]
#[cfg(not(test))] // Work around for rust-lang/rust#62127
pub fn log_api_call(attributes: TokenStream, item: TokenStream) -> TokenStream {
    let mut input = syn::parse_macro_input!(item as syn::ItemFn);
    let attrs = &input.attrs;
    let vis = &input.vis;
    let sig = &mut input.sig;
    let body = &input.block;
    let name = &sig.ident;

    if sig.asyncness.is_none() {
        return syn::Error::new_spanned(sig.fn_token, "only async fn is supported")
            .to_compile_error()
            .into();
    }

    let context_ref: syn::AttributeArgs = syn::parse_macro_input!(attributes);

    let name_opt = get_attrib_param(&context_ref, "name");

    if name_opt.is_none() {
        return syn::Error::new_spanned(sig.fn_token, "name parameter is required in macro")
            .to_compile_error()
            .into();
    }

    let id_param = get_attrib_param(&context_ref, "id");

    let body_quote = match get_attrib_param(&context_ref, "body") {
        Some(body_str) => {
            let body_ident = syn::Ident::new(&body_str, name.span());
            quote! {
                log::debug!("Body {}", serde_json::to_string_pretty(&#body_ident.0).unwrap_or(format!("{:?}", &#body_ident)) );
            }
        }
        _ => quote! {},
    };

    let path_quote = match get_attrib_param(&context_ref, "path") {
        Some(path) => {
            let path_ident = syn::Ident::new(&path, name.span());
            quote! {
                log::debug!("Path {}", serde_json::to_string(&#path_ident.as_ref()).unwrap_or(String::from("(error)")));
            }
        }
        _ => quote! {},
    };

    let query_quote = match get_attrib_param(&context_ref, "query") {
        Some(query) => {
            let query_ident = syn::Ident::new(&query, name.span());
            quote! {
                log::debug!("Query {}", serde_json::to_string(&#query_ident.0).unwrap_or(String::from("(error)")));
            }
        }
        _ => quote! {},
    };

    let log_info_quote = match id_param {
        Some(id_str) => {
            let id_ident = syn::Ident::new(&id_str, name.span());

            quote! {
                log::info!("API call: {}(), Identity: [{:?}]", #name_opt, #id_ident.identity);
            }
        }
        _ => quote! {
            log::info!("API call: {}()", #name_opt);
        },
    };

    sig.asyncness = None;

    (quote! {
        #(#attrs)*
        async #vis #sig {
            #log_info_quote
            #path_quote
            #query_quote
            #body_quote
            { #body }
        }
    })
    .into()
}

fn get_lit_value(lit: &syn::Lit) -> Option<String> {
    match lit {
        syn::Lit::Str(s) => Some(s.value()),
        _ => None,
    }
}

fn get_attrib_param(attrs: &syn::AttributeArgs, ident: &str) -> Option<String> {
    let mut result: Option<String> = None;

    for attr in attrs {
        match attr {
            syn::NestedMeta::Meta(meta) => match meta {
                syn::Meta::NameValue(mnv) => {
                    if mnv.path.is_ident(ident) {
                        result = get_lit_value(&mnv.lit);
                    }
                }
                _ => {}
            },
            _ => {}
        }
    }

    result
}
