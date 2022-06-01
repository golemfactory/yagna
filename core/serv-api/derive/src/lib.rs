extern crate proc_macro;
#[macro_use]
extern crate strum_macros;

mod component;
mod service;

use crate::component::Component;
use crate::service::Service;
use proc_macro::TokenStream;
use proc_macro2::Span;
use quote::{quote, ToTokens};
use std::convert::TryFrom;
use syn::{parse_macro_input, AttributeArgs, Error, Path, Result};

#[proc_macro_attribute]
pub fn services(attributes: TokenStream, item: TokenStream) -> TokenStream {
    let context_ref: AttributeArgs = parse_macro_input!(attributes);

    let ctx = match context_ref.get(0) {
        Some(syn::NestedMeta::Meta(context)) => context,
        _ => {
            return Error::new(Span::call_site(), "Wrong context type spec")
                .to_compile_error()
                .into()
        }
    };

    let parsed = parse_macro_input!(item as syn::Item);
    match parsed {
        syn::Item::Enum(item) => {
            let (services, errors): (Vec<_>, Vec<Result<_>>) = item
                .variants
                .iter()
                .map(Service::try_from)
                .partition(Result::is_ok);

            let services: Vec<Service> = services.into_iter().map(Result::unwrap).collect();
            let errors: Vec<Error> = errors.into_iter().map(Result::unwrap_err).collect();

            match errors.is_empty() {
                true => define_enum(item, services, ctx).into(),
                false => define_errors(errors).into(),
            }
        }
        _ => Error::new(Span::call_site(), "Not an enum type")
            .to_compile_error()
            .into(),
    }
}

#[inline]
fn quote_as_trait(path: &Path) -> proc_macro2::TokenStream {
    let path = path.to_token_stream();
    quote! {<#path as ya_service_api_interfaces::Service>}
}

fn define_enum(
    item: syn::ItemEnum,
    services: Vec<Service>,
    context: &syn::Meta,
) -> proc_macro2::TokenStream {
    let ident = item.ident;

    let cli = define_cli_services(&item.vis, &ident, &services);
    let gsb = define_gsb_services(&services, context);
    let rest = define_rest_services(&services, context);

    quote! {
        #cli

        impl #ident {
            #gsb
            #rest
        }
    }
}

fn define_errors(errors: Vec<Error>) -> proc_macro2::TokenStream {
    let mut error_stream = proc_macro2::TokenStream::new();
    errors
        .into_iter()
        .map(|e| e.to_compile_error())
        .for_each(|e| error_stream.extend(e.into_iter()));
    error_stream
}

fn define_cli_services(
    vis: &syn::Visibility,
    ident: &syn::Ident,
    services: &Vec<Service>,
) -> proc_macro2::TokenStream {
    let mut variants = proc_macro2::TokenStream::new();
    let mut variants_match = proc_macro2::TokenStream::new();

    for service in services.iter() {
        let flattened = service
            .components
            .contains(&Component::Cli { flatten: true });
        let plain = service
            .components
            .contains(&Component::Cli { flatten: false });

        if !flattened && !plain {
            continue;
        }

        let name = service.name.clone();
        let path = quote_as_trait(&service.path);

        if flattened {
            variants.extend(quote! {
                #[structopt(flatten)]
            });
        }
        variants.extend(quote! {
            #[structopt(setting = structopt::clap::AppSettings::DeriveDisplayOrder)]
            #name(#path::Cli),
        });
        variants_match.extend(quote! {
            Self::#name(c) => c.run_command(ctx).await,
        });
    }

    let variants_match = if variants_match.is_empty() {
        quote! {
            Ok(().into())
        }
    } else {
        quote! {
            match self {
                #variants_match
            }
        }
    };

    quote! {
        #[doc(hidden)]
        #[derive(structopt::StructOpt, Debug)]
        #[structopt(setting = structopt::clap::AppSettings::DeriveDisplayOrder)]
        #vis enum #ident {
            #variants
        }

        impl #ident {
            pub async fn run_command(self, ctx: &CliCtx) -> anyhow::Result<CommandOutput> {
                #variants_match
            }
        }
    }
}

fn define_gsb_services(
    services: &Vec<Service>,
    context_path: &syn::Meta,
) -> proc_macro2::TokenStream {
    let mut inner = proc_macro2::TokenStream::new();
    for service in services.iter() {
        if !service.supports(Component::Gsb) {
            continue;
        }
        let path = &service.path;
        let service_name = format!("{}", &service.name);
        inner.extend(quote! {
            #path::gsb(context).await?;
            log::info!("{} GSB service successfully activated", #service_name);
        });
    }

    quote! {
        pub async fn gsb(context: &#context_path) -> anyhow::Result<()>  {
            #inner
            Ok(())
        }
    }
}

fn define_rest_services(
    services: &Vec<Service>,
    context_path: &syn::Meta,
) -> proc_macro2::TokenStream {
    let mut inner = proc_macro2::TokenStream::new();
    for service in services
        .iter()
        .filter(|service| service.supports(Component::Rest))
    {
        let path = &service.path;
        let service_name = format!("{}", &service.name);
        inner.extend(quote! {
            let app = app.service(#path::rest(context));
            log::debug!("{} REST scope successfully installed", #service_name);
        });
    }

    quote! {
        pub fn rest<T, B>(mut app: actix_web::App<T>, context: &#context_path) -> actix_web::App<T>
        where
            B : actix_web::body::MessageBody,
            T : actix_service::ServiceFactory<
                actix_web::dev::ServiceRequest,
                Response = actix_web::dev::ServiceResponse<B>,
                Error = actix_web::error::Error,
                InitError = (),
                Config = (),
            >,
        {
            #inner
            app
        }
    }
}
