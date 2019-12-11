#[macro_export]
macro_rules! rest_interface {
    {
        impl $interface_name:ident {
            $(
                $(#[doc = $api_doc:expr])*
                pub async fn $fn_name:ident (
                    &self,
                    $( $arg:ident : $arg_t:tt ),*
                    $( #[path] $argp:ident : $argp_t:tt ),*
                ) -> Result<$ret:ty> {
                    let $result:ident = self.client()
                        .$http_method:ident($rest_uri:expr)
                        .$send_method:ident $send_args:tt
                        .$response_extractor:ident();
                    $( $body:tt )?
                }
            )+
        }
    }
    => {
        impl $interface_name {
            $(
                $(#[doc = $api_doc])*
                #[doc = "Does `"]
//                #[doc = stringify!($http_method)]
                #[doc = "` on `"]
                #[doc = $rest_uri]
                #[doc = "` REST URI."]
                pub async fn $fn_name (
                    &self,
                    $( $arg : $arg_t ),*
                    $( $argp : $argp_t ),*
                ) -> Result<$ret> {
                    $( println!( "{}", stringify!($argp) ); )*
                    let uri = format!( $rest_uri, $($argp = $argp),* );
                    println!("doing {} on {}", stringify!($http_method), uri);
                    let $result = self.client()
                        .$http_method(self.uri(uri))
                        .$send_method $send_args
                        .compat()
                        .await?
                        .$response_extractor()
                        .compat()
                        .await?;
                    $( $body )?
                }
            )+
        }
    };
}
