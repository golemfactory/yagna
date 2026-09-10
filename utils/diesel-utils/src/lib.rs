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
        impl ::diesel::expression::AsExpression<::diesel::sql_types::Text> for #name {
            type Expression = <String as ::diesel::expression::AsExpression<
                ::diesel::sql_types::Text,
            >>::Expression;

            fn as_expression(self) -> Self::Expression {
                <String as ::diesel::expression::AsExpression<
                    ::diesel::sql_types::Text,
                >>::as_expression(self.to_string())
            }
        }

        impl ::diesel::expression::AsExpression<
            ::diesel::sql_types::Nullable<::diesel::sql_types::Text>,
        > for #name {
            type Expression = <String as ::diesel::expression::AsExpression<
                ::diesel::sql_types::Nullable<::diesel::sql_types::Text>,
            >>::Expression;

            fn as_expression(self) -> Self::Expression {
                <String as ::diesel::expression::AsExpression<
                    ::diesel::sql_types::Nullable<::diesel::sql_types::Text>,
                >>::as_expression(self.to_string())
            }
        }

        impl ::diesel::expression::AsExpression<::diesel::sql_types::Text> for &#name {
            type Expression = <String as ::diesel::expression::AsExpression<
                ::diesel::sql_types::Text,
            >>::Expression;

            fn as_expression(self) -> Self::Expression {
                <String as ::diesel::expression::AsExpression<
                    ::diesel::sql_types::Text,
                >>::as_expression(self.to_string())
            }
        }

        impl ::diesel::expression::AsExpression<
            ::diesel::sql_types::Nullable<::diesel::sql_types::Text>,
        > for &#name {
            type Expression = <String as ::diesel::expression::AsExpression<
                ::diesel::sql_types::Nullable<::diesel::sql_types::Text>,
            >>::Expression;

            fn as_expression(self) -> Self::Expression {
                <String as ::diesel::expression::AsExpression<
                    ::diesel::sql_types::Nullable<::diesel::sql_types::Text>,
                >>::as_expression(self.to_string())
            }
        }

        impl ::diesel::expression::AsExpression<::diesel::sql_types::Text> for &&#name {
            type Expression = <String as ::diesel::expression::AsExpression<
                ::diesel::sql_types::Text,
            >>::Expression;

            fn as_expression(self) -> Self::Expression {
                <String as ::diesel::expression::AsExpression<
                    ::diesel::sql_types::Text,
                >>::as_expression(self.to_string())
            }
        }

        impl ::diesel::expression::AsExpression<
            ::diesel::sql_types::Nullable<::diesel::sql_types::Text>,
        > for &&#name {
            type Expression = <String as ::diesel::expression::AsExpression<
                ::diesel::sql_types::Nullable<::diesel::sql_types::Text>,
            >>::Expression;

            fn as_expression(self) -> Self::Expression {
                <String as ::diesel::expression::AsExpression<
                    ::diesel::sql_types::Nullable<::diesel::sql_types::Text>,
                >>::as_expression(self.to_string())
            }
        }

        impl<DB> ::diesel::deserialize::FromSql<::diesel::sql_types::Text, DB> for #name
        where
            DB: ::diesel::backend::Backend,
            String: ::diesel::deserialize::FromSql<::diesel::sql_types::Text, DB>,
        {
            fn from_sql(bytes: DB::RawValue<'_>) -> ::diesel::deserialize::Result<#name> {
                let value = <String as ::diesel::deserialize::FromSql<
                    ::diesel::sql_types::Text,
                    DB,
                >>::from_sql(bytes)?;
                Ok(value.parse()?)
            }
        }
    };
    proc_macro::TokenStream::from(generated)
}
