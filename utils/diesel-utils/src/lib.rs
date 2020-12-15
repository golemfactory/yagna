extern crate proc_macro;
use proc_macro2::Span;
use quote::quote;
use syn::Error;

#[proc_macro_derive(DbTextField)]
pub fn database_text_field(item: proc_macro::TokenStream) -> proc_macro::TokenStream {
    let name = match syn::parse_macro_input!(item as syn::Item) {
        syn::Item::Enum(item) => item.ident,
        syn::Item::Struct(item) => item.ident,
        _ => {
            return Error::new(
                Span::call_site(),
                "May only be applied to structs or enums.",
            )
            .to_compile_error()
            .into()
        }
    };

    let generated = quote! {
        impl<DB> ::diesel::serialize::ToSql<::diesel::sql_types::Text, DB> for #name
        where
            DB: ::diesel::backend::Backend,
            String: ::diesel::serialize::ToSql<::diesel::sql_types::Text, DB>,
        {
            fn to_sql<W: ::std::io::Write>(&self, out: &mut ::diesel::serialize::Output<W, DB>) -> ::diesel::serialize::Result {
                self.to_string().to_sql(out)
            }
        }

        impl<DB> ::diesel::deserialize::FromSql<::diesel::sql_types::Text, DB> for #name
        where
            DB: ::diesel::backend::Backend,
            String: ::diesel::deserialize::FromSql<::diesel::sql_types::Text, DB>,
        {
            fn from_sql(bytes: Option<&DB::RawValue>) -> ::diesel::deserialize::Result<#name> {
                Ok(String::from_sql(bytes)?.parse()?)
            }
        }
    };
    proc_macro::TokenStream::from(generated)
}
