#[macro_export]
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
                ) -> Result<$ret:ty> {
                    let $response:ident =
                        $http_method:ident($rest_uri:expr)
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

        use super::ApiConfiguration;

        $(#[doc = $interface_doc])*
        pub struct $interface_name {
            configuration: Arc<ApiConfiguration>,
        }

        impl $interface_name {
            pub fn new(configuration: Arc<ApiConfiguration>) -> Self {
                $interface_name { configuration }
            }

            fn client(&self) -> Client {
                Client::default()
            }

            fn uri<T: Into<String>>(&self, suffix: T) -> String {
                self.configuration.api_endpoint(suffix)
            }

            $(
                $(#[doc = $api_doc])*
                #[doc = "<br><br>Uses `"]
//                #[doc = stringify!($http_method)]
                #[doc = $rest_uri]
                #[doc = "` REST URI."]
                pub async fn $fn_name (
                    &self,
                    $( $arg : $arg_t ),*
                    $( $argp : $argp_t ),*
                ) -> Result<$ret> {
                    let uri = format!( $rest_uri $(, $argp = $argp)* );
                    println!("doing {} on {}", stringify!($http_method), uri);
                    let $response = self.client()
                        .$http_method(self.uri(uri))
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
