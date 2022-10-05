use crate::component::{Component, COMPONENTS};
use proc_macro2::{Ident, Span};
use quote::ToTokens;
use std::collections::HashSet;
use std::convert::TryFrom;
use std::fmt::Formatter;
use syn::punctuated::Punctuated;
use syn::spanned::Spanned;
use syn::{
    Attribute, AttributeArgs, Error, Fields, FieldsUnnamed, Meta, NestedMeta, Path, Result, Type,
    Variant,
};

pub struct Service {
    pub name: Ident,
    pub path: Path,
    pub components: HashSet<Component>,
}

impl Service {
    #[inline]
    pub fn supports(&self, component: Component) -> bool {
        self.components.contains(&component)
    }

    fn parse_attrs(attrs: &[Attribute]) -> Result<HashSet<Component>> {
        let mut components = HashSet::new();

        for attr in attrs.iter() {
            let meta = Self::parse_attr(attr)?;
            let result = Self::parse_metas(meta)?;

            match Keyword::try_from(&attr.path)? {
                Keyword::Enable => components.extend(result.into_iter()),
                Keyword::Disable => {
                    let diff = COMPONENTS.difference(&result).cloned();
                    components.extend(diff);
                }
            };
        }

        Ok(components)
    }

    fn parse_attr(attr: &Attribute) -> Result<Meta> {
        let name = attr.path.to_token_stream();
        let tokens = attr.tokens.clone();
        let stream = quote::quote! {#name#tokens};

        let nested = syn::parse_macro_input::parse::<AttributeArgs>(stream.into())?;
        match nested.first() {
            Some(NestedMeta::Meta(meta)) => Ok(meta.clone()),
            _ => Err(Error::new(attr.span(), "Invalid format")),
        }
    }

    fn parse_metas(metas: Meta) -> Result<HashSet<Component>> {
        let result = match metas {
            Meta::List(list) => Self::parse_nested(list.nested)?,
            _ => return Err(Error::new(Span::call_site(), "Invalid attribute format")),
        };
        Ok(result)
    }

    fn parse_nested<T>(nested_metas: Punctuated<NestedMeta, T>) -> Result<HashSet<Component>> {
        let mut components: HashSet<Component> = HashSet::new();

        for nested_meta in nested_metas.iter() {
            let component = match nested_meta {
                NestedMeta::Meta(meta) => match meta {
                    Meta::Path(ref path) => Component::try_from(path)?,
                    Meta::List(ref list) => Component::try_from(list)?,
                    _ => return Err(Error::new(Span::call_site(), "Invalid format")),
                },
                _ => return Err(Error::new(Span::call_site(), "Invalid format")),
            };
            components.insert(component);
        }

        Ok(components)
    }

    fn parse_fields(fields: &FieldsUnnamed) -> Result<Path> {
        let field = match fields.unnamed.first() {
            Some(f) => f,
            None => return Err(Error::new(fields.span(), "Expected argument")),
        };

        match &field.ty {
            Type::Path(type_path) => Ok(type_path.path.clone()),
            _ => Err(Error::new(fields.span(), "Unsupported format")),
        }
    }
}

impl TryFrom<&Variant> for Service {
    type Error = Error;

    fn try_from(variant: &Variant) -> Result<Self> {
        let name = variant.ident.clone();
        let components = Self::parse_attrs(&variant.attrs)?;
        let path = match &variant.fields {
            Fields::Unnamed(fields) => Self::parse_fields(fields)?,
            _ => return Err(Error::new(variant.ident.span(), "Invalid format")),
        };

        Ok(Self {
            name,
            path,
            components,
        })
    }
}

impl std::fmt::Debug for Service {
    fn fmt(&self, f: &mut Formatter<'_>) -> std::result::Result<(), std::fmt::Error> {
        f.write_str(&format!(
            "Service < name: {}, path: {}, components: {:?} >",
            self.name,
            self.path.to_token_stream(),
            self.components
        ))
    }
}

#[derive(Clone, Debug)]
enum Keyword {
    Enable,
    Disable,
}

impl TryFrom<&Path> for Keyword {
    type Error = Error;

    fn try_from(path: &Path) -> Result<Self> {
        let keyword = path.to_token_stream().to_string();
        let variant = match keyword.as_str() {
            "enable" => Keyword::Enable,
            "disable" => Keyword::Disable,
            _ => {
                let span = path.span();
                let message = format!("Unknown keyword: {}", keyword);
                return Err(Error::new(span, message));
            }
        };
        Ok(variant)
    }
}
