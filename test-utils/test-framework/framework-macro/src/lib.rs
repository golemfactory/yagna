use proc_macro::{self, TokenStream};
use quote::{format_ident, quote};
use syn::{parse, ItemFn};

#[proc_macro_attribute]
pub fn framework_test(attr: TokenStream, input: TokenStream) -> TokenStream {
    let function = parse::<ItemFn>(input.clone()).unwrap();

    if attr.into_iter().count() > 0 {
        panic!("`framework_test` macro doesn't support wrapping other macros");
    }

    validate_function(&function);

    let name = function.sig.ident;
    let code = function.block;

    let internal_name = format_ident!("_{}_", name);
    let test_name = name.to_string();

    let tokens = quote! {
        #[serial_test::serial]
        #[test]
        fn #name () {
            async fn #internal_name (framework: YagnaFramework) -> anyhow::Result<()> {
                #code
            }

            ya_test_framework::framework::framework_setup( #internal_name, &prepare_test_dir!(), #test_name );
        }
    };

    tokens.into()
}

fn validate_function(function: &ItemFn) {
    if function.sig.asyncness.is_none() {
        panic!(
            "Framework test should be `async` function. {:?}",
            function.sig
        );
    }

    if function.sig.inputs.len() != 1 {
        panic!(
            "Framework test should have only one argument, but {} given.",
            function.sig.inputs.len()
        );
    }

    if let Some(_param) = function.sig.inputs.first() {
        // TODO: Argument should be `YagnaFramework`
    } else {
        panic!("Procedural macro implementation error.");
    }
}
