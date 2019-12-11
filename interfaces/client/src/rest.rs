#[macro_export]
macro_rules! rest_interface {
    {
//        $(#[doc = $interface_doc:expr])*
        pub $interface_name:ident {
            $(
//                $(#[doc = $field_doc:expr])*
                #[field]
                $field:ident : $type:ty;
            )*
            
            $(
//                $(#[doc = $api_doc:expr])*
                #[ REST ( $http_method:ident, $rest_uri:expr, $response_extractor:ident ) ]
                fn $fn_name:ident (&self, $( $arg:ident : $arg_type:ty )* ) -> Result<$ret:ty> {
                    #[result] $result:ident;
                    $body:tt
                }
            )+
        }
    }
    => {
//        $(#[doc = $interface_doc])*
        pub struct $interface_name {
            $(
//                $(#[doc = $field_doc])*
                $field: $type,
            )*
        }

        impl $interface_name {
            pub fn new(
                $(
                    $field: $type,
                )*
            ) -> Self {
                $interface_name {
                    $(
                        $field,
                    )*
                }
            }

            $(
//                $(#[doc = $api_doc])*
//                #[doc = "Does `"]
//                #[doc = $http_method]
//                #[doc = "` on `"]
//                #[doc = $rest_uri]
//                #[doc = "` REST URI."]
                pub async fn $fn_name (&self, $( $arg : $arg_type )* ) -> Result<$ret> {
                    let $result = Client::default()
                        .$http_method(self.configuration.api_endpoint($rest_uri))
                        .send_json( $( &$arg )* )
                        .compat()
                        .await?
                        .$response_extractor()
                        .compat()
                        .await?;
                    $body
                }
            )+
        }

    }
}