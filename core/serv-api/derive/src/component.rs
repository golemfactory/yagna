use proc_macro2::Span;
use quote::ToTokens;
use std::collections::HashSet;
use std::convert::TryFrom;
use std::iter::FromIterator;
use strum::IntoEnumIterator;
use syn::spanned::Spanned;
use syn::{Error, Meta, MetaList, NestedMeta, Path, Result};

lazy_static::lazy_static! {
    pub static ref COMPONENTS: HashSet<Component> = HashSet::from_iter(Component::iter());
}

#[derive(Clone, Debug, Eq, EnumIter, Hash, PartialEq)]
pub enum Component {
    Cli { flatten: bool },
    Db,
    Gsb,
    Rest,
}

impl TryFrom<&Path> for Component {
    type Error = Error;

    fn try_from(path: &Path) -> Result<Self> {
        Self::try_from(path.to_token_stream().to_string())
    }
}

impl TryFrom<&MetaList> for Component {
    type Error = Error;

    fn try_from(list: &MetaList) -> Result<Self> {
        let span = list.span();
        let name = list.path.to_token_stream().to_string();
        let error = Error::new(span, format!("Invalid '{}' component format", name));

        match name.as_str() {
            "cli" => match list.nested.first() {
                Some(nested_meta) => match Keyword::try_from(nested_meta)? {
                    Keyword::Flatten => Ok(Component::Cli { flatten: true }),
                },
                None => Err(error),
            },
            _ => Err(error),
        }
    }
}

impl TryFrom<String> for Component {
    type Error = Error;

    fn try_from(name: String) -> Result<Self> {
        let component = match name.as_str() {
            "cli" => Component::Cli { flatten: false },
            "db" => Component::Db,
            "gsb" => Component::Gsb,
            "rest" => Component::Rest,
            _ => {
                let message = format!("Unknown component: {}", name);
                return Err(Error::new(Span::call_site(), message));
            }
        };

        Ok(component)
    }
}

#[derive(Clone, Debug)]
enum Keyword {
    Flatten,
}

impl TryFrom<&NestedMeta> for Keyword {
    type Error = Error;

    fn try_from(nested_meta: &NestedMeta) -> Result<Self> {
        let span = nested_meta.span().into();
        match nested_meta {
            NestedMeta::Meta(Meta::Path(path)) => Self::try_from(path),
            _ => Err(Error::new(span, "Invalid format")),
        }
    }
}

impl TryFrom<&Path> for Keyword {
    type Error = Error;

    #[inline]
    fn try_from(path: &Path) -> Result<Self> {
        Self::try_from(path.to_token_stream().to_string())
    }
}

impl TryFrom<String> for Keyword {
    type Error = Error;

    fn try_from(string: String) -> Result<Self> {
        match string.as_str() {
            "flatten" => Ok(Keyword::Flatten),
            _ => Err(Error::new(
                Span::call_site(),
                format!("Unknown keyword: {}", string),
            )),
        }
    }
}
