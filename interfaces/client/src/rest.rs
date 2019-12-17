//! Macro to facilitate REST API async bindings
#[macro_export]
/// Facilitates defining of REST interfaces by generating boilerplate code from concise definition.
///
/// Macro defines basic `struct` with given name and implements `new` factory fn along with
/// some helper functions. Struct has single field with [`WebClient`](./web/struct.WebClient.html).
///
/// For every async fn declared it automatically injects arguments with `#[path]` marker
/// into `rest_url` from the first line of the body. It also automatically appends this
/// url with all arguments marked with `#[query]`.
///
/// Current limitations and restrictions:
///   * first statement in fn body has to be strictly compatible with matcher
///     ```no-run
///     let <var_name> = <http_method>(<url>).<send_method>[(<args>)].<response_extractor>();
///     ```
///     where
///       - `<http_method>` is lower case: eg. `get`, `post` or others available for [awc::Client](
///         https://docs.rs/awc/0.2.8/awc/struct.Client.html).
///       - `<send_method>` is `send` for no body (no args) or `send_json` with argument(s)
///       - `<response_extractor>` is
///          - [`body`](https://docs.rs/awc/0.2.8/awc/struct.ClientResponse.html#method.body)
///          - or [`json`](https://docs.rs/awc/0.2.8/awc/struct.ClientResponse.html#method.json)
///   * rest of the fn body has to be single token (eg. `response`) or token tree in brackets
///     (eg. `{ Ok(()) }`).<br> This is a [`tt` fragment specifier](
///     https://doc.rust-lang.org/reference/macros-by-example.html#metavariables) limitation.
///   * `url` must not start with `/` (a [Url](
///     https://docs.rs/url/2.1.0/url/struct.Url.html#method.join) lib limitation)
///   * `url` must end with `/` to properly append `#[query]` arguments (also a Url lib limit).
macro_rules! rest_interface {
    {
        $(#[doc = $interface_doc:expr])*
        impl $interface_name:ident {
            $(
                $(#[doc = $api_doc:expr])*
                pub async fn $fn_name:ident (
                    &self
                    $(, $arg:ident : $arg_t:ty )*
                    $(, #[path] $argp:ident : $argp_t:ty )*
                    $(, #[query] $argq:ident : $argq_t:ty )*
                ) -> Result<$ret:ty> {
                    let $response:ident =
                        $http_method:ident($rest_url:expr)
                        .$send_method:ident $send_args:tt
                        .$response_extractor:ident();
                    $body:tt
                }
            )+
        }
    }
    => {
        use futures::compat::Future01CompatExt;
        use std::sync::Arc;
        use url::Url;

        use crate::web::{WebClient, QueryParamsBuilder};

        $(#[doc = $interface_doc])*
        pub struct $interface_name {
            client: Arc<WebClient>,
        }

        impl $interface_name {
            pub fn new(client: Arc<WebClient>) -> Self {
                Self { client }
            }

            // TODO: ask @mfranciszkiewicz if it is needed
            // TODO: maybe its better to implement it one level up
            pub fn replace_client(&mut self, client: WebClient) {
                std::mem::replace(&mut self.client, Arc::new(client));
            }


            fn url<T: Into<String>>(&self, suffix: T) -> Url {
                self.client.configuration.endpoint_url(suffix)
            }

            $(
                $(#[doc = $api_doc])*
                #[doc = "<br><br>Uses `"]
//                #[doc = stringify!($http_method)]
                #[doc = $rest_url]
                #[doc = "` REST URL."]
                pub async fn $fn_name (
                    &self
                    $(, $arg : $arg_t )*
                    $(, $argp : $argp_t )*
                    $(, $argq : $argq_t )*
                ) -> Result<$ret> {
                    let mut url = self.url(format!( $rest_url $(, $argp = $argp)* ));
                    let query = QueryParamsBuilder::new()
                        $(.put(stringify!($argq), $argq))*
                        .build();
                    if query.len() > 1 {
                        url = url.join(&query)?
                    }
                    println!("doing {} on {}", stringify!($http_method), url);
                    let $response = self.client.awc
                        .$http_method(url.as_str())
                        .$send_method $send_args
                        .compat()
                        .await?
                        .$response_extractor()
                        .compat()
                        .await
                        .map_err(crate::Error::from);
                    $body
                }
            )+
        }
    };
}

macro_rules! url_format {
    ($path:expr $(, $var:ident )* ) => (
        format!($path $(, $var=$var)*)
    )
}
