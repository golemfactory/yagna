//! Macro to facilitate URL formmating for REST API async bindings
macro_rules! url_format {
    {
        $path:expr $(, $var:ident )* $(, #[query] $varq:ident )*
    } => {{
        let mut url = format!( $path $(, $var=$var)* );
        let query = crate::web::QueryParamsBuilder::new()
            $( .put( stringify!($varq), $varq ) )*
            .build();
        if query.len() > 1 {
            url = format!("{}?{}", url, query)
        }
        url
    }};
}
