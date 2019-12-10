//#[macro_export]
//macro_rules! rest_interface {
//    {
//    pub $interface_name:ident {
//        $(
//            $(#[doc = $doc:expr])*
//            $field:ident $type:ty;
//        )*
//
//        $(
//            $(#[doc = $doc:expr])*
//            #[$http_method($rest_uri:expr, $response_extractor:ident)]
//            fn $it:tt $args:tt -> Result<$ret:ty>;
//        )*
//    }
//
//    $(
//        converter $converter_name:ident $converter_method:ident;
//    )?
//    }
//    => {
//        pub struct ProviderApi {
//        configuration: Arc<ApiConfiguration>,
//        }
//
//        impl ProviderApi {
//            pub fn new(configuration: Arc<ApiConfiguration>) -> Self {
//                ProviderApi { configuration }
//            }
//
//            pub fn new(configuration: Arc<ApiConfiguration>) -> Self {
//                $interface_name { configuration }
//            }
//        }
//
//    }
//}