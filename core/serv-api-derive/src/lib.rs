extern crate proc_macro;
#[macro_use]
extern crate strum_macros;

mod component;
mod service;

use crate::component::Component;
use crate::service::Service;
use proc_macro::TokenStream;
use proc_macro2::Span;
use quote::quote;
use std::convert::TryFrom;
use syn::export::ToTokens;
use syn::{parse_macro_input, Error, Path, Result};

#[proc_macro_attribute]
pub fn services(_: TokenStream, item: TokenStream) -> TokenStream {
    let parsed = parse_macro_input!(item as syn::Item);
    match parsed {
        syn::Item::Enum(item) => {
            let (services, errors): (Vec<_>, Vec<Result<_>>) = item
                .variants
                .into_iter()
                .map(Service::try_from)
                .partition(Result::is_ok);

            let services: Vec<Service> = services.into_iter().map(Result::unwrap).collect();
            let errors: Vec<Error> = errors.into_iter().map(Result::unwrap_err).collect();

            match errors.is_empty() {
                true => define_services(services).into(),
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

fn define_errors(errors: Vec<Error>) -> proc_macro2::TokenStream {
    let mut error_stream = proc_macro2::TokenStream::new();
    errors
        .into_iter()
        .map(|e| e.to_compile_error())
        .for_each(|e| error_stream.extend(e.into_iter()));
    error_stream
}

fn define_services(services: Vec<Service>) -> proc_macro2::TokenStream {
    let db = define_db_services(&services);
    let cli = define_cli_services(&services);
    let gsb = define_gsb_services(&services);
    let rest = define_rest_services(&services);

    quote! {
        #[doc(hidden)]
        mod services {
            use super::*;

            #cli
            #db
            #gsb
            #rest
        }
    }
}

fn define_db_services(services: &Vec<Service>) -> proc_macro2::TokenStream {
    let mut inner = proc_macro2::TokenStream::new();
    for service in services.iter() {
        if !service.supports(component::Component::Db) {
            continue;
        }

        let path = quote_as_trait(&service.path);
        inner.extend(quote! {
            #path::db(&db).await?;
        });
    }

    quote! {
        #[doc(hidden)]
        pub async fn db(db: &DbExecutor) -> anyhow::Result<()> {
            #inner;
            Ok(())
        }
    }
}

fn define_cli_services(services: &Vec<Service>) -> proc_macro2::TokenStream {
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
            #[structopt(setting = clap::AppSettings::DeriveDisplayOrder)]
            #name(#path::Cli),
        });
        variants_match.extend(quote! {
            CliCommands::#name(c) => c.run_command(ctx).await,
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
        pub enum CliCommands {
            #variants
        }

        impl CliCommands {
            pub async fn run_command(self, ctx: &CliCtx) -> anyhow::Result<CommandOutput> {
                #variants_match
            }
        }
    }
}

fn define_gsb_services(services: &Vec<Service>) -> proc_macro2::TokenStream {
    let mut inner = proc_macro2::TokenStream::new();
    for service in services.iter() {
        if !service.supports(Component::Gsb) {
            continue;
        }

        let path = quote_as_trait(&service.path);
        inner.extend(quote! {
            #path::gsb(&db).await?;
        });
    }

    quote! {
        pub async fn gsb(db: &DbExecutor) -> anyhow::Result<()>  {
            #inner
            Ok(())
        }
    }
}

fn define_rest_services(services: &Vec<Service>) -> proc_macro2::TokenStream {
    let mut inner = proc_macro2::TokenStream::new();
    for service in services.iter() {
        if !service.supports(Component::Rest) {
            continue;
        }

        let path = quote_as_trait(&service.path);
        inner.extend(quote! {
            app = match #path::rest(&db) {
                Some(scope) => app.service(scope),
                None => app,
            };
        });
    }

    quote! {
        pub fn rest<T, B>(mut app: actix_web::App<T, B>, db: &DbExecutor) -> actix_web::App<T, B>
        where
            B : actix_web::body::MessageBody,
            T : actix_service::ServiceFactory<
                Config = (),
                Request = actix_web::dev::ServiceRequest,
                Response = actix_web::dev::ServiceResponse<B>,
                Error = actix_web::error::Error,
                InitError = (),
            >,
        {
            #inner
            app
        }
    }
}
