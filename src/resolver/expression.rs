use std::str;

use asnom::structures::{Tag, OctetString, ExplicitTag};

use super::properties::{Property, PropertySet, PropertyRef, parse_prop_ref };
use super::errors::{ResolveError, ExpressionError};
use super::ldap_parser;

// Expression resolution result enum
#[derive(Debug, Clone, PartialEq)]
pub enum ResolveResult {
    True,
    False,
    Undefined,
    Err(ResolveError)
}



// Expression structure is the vehicle for LDAP filter expression resolution
#[derive(Clone, Debug, PartialEq)]
pub enum Expression {
    Equals(PropertyRef, String), // property ref, value
    Greater(PropertyRef, String), // property ref, value
    GreaterEqual(PropertyRef, String), // property ref, value
    Less(PropertyRef, String), // property ref, value
    LessEqual(PropertyRef, String), // property ref, value
    Present(PropertyRef), // property ref
    Or(Vec<Box<Expression>>), // operands
    And(Vec<Box<Expression>>), // operands
    Not(Box<Expression>), // operand
    Empty
}

impl Expression {
    // Implement strong resolution (ie. undefined results are propagated rather than ignored)
    // TODO: properties of different types (and respective "equals" logic) -> PropertyValue equals(), PropertySet parse_prop() - prop_parser submodule?
    // TODO: other comparison operators
    // TODO: wildcard matching of property values
    // TODO: wildcard matching of value-less properties
    // TODO: aspects
    // TODO: finalize and review the matching relation implementations
    pub fn resolve(&self, property_set : &PropertySet) -> ResolveResult {
        match self {
            Expression::Equals(attr, val) => {
                self.resolve_equals(attr, val, property_set)
            },
            // TODO other comparison operators
            Expression::And(inner_expressions) => {
                self.resolve_and(inner_expressions, property_set)
            },
            Expression::Or(inner_expressions) => {
                self.resolve_or(inner_expressions, property_set)
            },
            Expression::Not(inner_expression) => {
                match inner_expression.resolve(property_set) {
                    ResolveResult::True => ResolveResult::False,
                    ResolveResult::False => ResolveResult::True,
                    ResolveResult::Undefined => ResolveResult::Undefined,
                    ResolveResult::Err(err) => ResolveResult::Err(err)
                }
            },
            Expression::Present(attr) => {
                self.resolve_present(attr, property_set)
            },
            _ => ResolveResult::Err(ResolveError::new(&format!("Unexpected Expression type {:?}", self)))
        }
    }

    fn resolve_and(&self, seq : &Vec<Box<Expression>>, property_set : &PropertySet) -> ResolveResult {
        for exp in seq {
            match exp.resolve(property_set) {
                ResolveResult::True => { /* do nothing, keep iterating */ },
                ResolveResult::False => { return ResolveResult::False },
                ResolveResult::Undefined => { return ResolveResult::Undefined },
                ResolveResult::Err(err) => { return ResolveResult::Err(err) }
            }
        }

        ResolveResult::True
    }

    fn resolve_or(&self, seq : &Vec<Box<Expression>>, property_set : &PropertySet) -> ResolveResult {
        for exp in seq {
            match exp.resolve(property_set) {
                ResolveResult::True => { return ResolveResult::True },
                ResolveResult::False => { /* keep iterating */ },
                ResolveResult::Undefined => { return ResolveResult::Undefined },
                ResolveResult::Err(err) => { return ResolveResult::Err(err) }
            }
        }

        ResolveResult::False
    }

    fn resolve_equals(&self, attr : &PropertyRef, val : &str, property_set : &PropertySet) -> ResolveResult {
        // TODO this requires rewrite to cater for implicit properties...
        // test if property exists and then if the value matches
        let mut name = "";

        // extract referred property name
        match attr {
            PropertyRef::Value(n) => { name = n; },
            PropertyRef::Aspect(n, _a) => { name = n; },
        }

        match property_set.properties.get(name) {
            Some(prop) => {
                match prop {
                    Property::Explicit(_name, value, aspects) => {
                        // now decide if we are referring to value or aspect
                        match attr {
                            PropertyRef::Value(_n) => { 
                                // resolve against prop value
                                if value.equals(val) {
                                    ResolveResult::True
                                }
                                else
                                {
                                    ResolveResult::False
                                }
                            },
                            PropertyRef::Aspect(_n, aspect) => { 
                                println!("Resolving Equals against Aspect: {}", aspect);
                                // resolve against prop aspect
                                match aspects.get(&aspect[..]) {
                                    Some(aspect_value) => {
                                        if val == *aspect_value {
                                            ResolveResult::True
                                        }
                                        else {
                                            ResolveResult::False
                                        }
                                    },
                                    None => {
                                        ResolveResult::Undefined
                                    }
                                }
                            },
                        }

                    },
                    Property::Implicit(_name) => {
                        ResolveResult::Undefined
                    }
                }
            },
            None => {
                ResolveResult::Undefined
            }
        }
    }

    // Resolve property/aspect presence
    fn resolve_present(&self, attr : &PropertyRef, property_set : &PropertySet) -> ResolveResult {
        match attr {
            // for value reference - only check if property exists in PpropertySet
            PropertyRef::Value(name) => {
                match property_set.properties.get(&name[..]) {
                    Some(_value) => {
                        ResolveResult::True
                    },
                    None => {
                        ResolveResult::False
                    }
                }
            },
            // for aspect reference - first check if property exists, then check for aspect
            PropertyRef::Aspect(name, aspect) => {
                match property_set.properties.get(&name[..]) {
                    Some(value) => {
                        match value {
                            Property::Explicit(_name, _val, aspects) => {
                                match aspects.get(&aspect[..]) {
                                    Some(_aspect) => {
                                        ResolveResult::True
                                    },
                                    None => {
                                        ResolveResult::False
                                    }
                                }
                            },
                            Property::Implicit(_name) => { // no aspects for implicit properties
                                ResolveResult::False
                            }
                        }
                    },
                    None => {
                        ResolveResult::False
                    }
                }
            }
        }
    }


}


// Expression building

pub fn build_expression(root : &Tag) -> Result<Expression, ExpressionError> {
    match root {
        Tag::Sequence(seq) => {
            match seq.id {
                ldap_parser::TAG_AND => {
                    build_multi_expression(seq.id, &seq.inner)
                },
                ldap_parser::TAG_OR => {
                    build_multi_expression(seq.id, &seq.inner)
                },
                ldap_parser::TAG_EQUAL => {
                    build_simple_expression(ldap_parser::TAG_EQUAL, &seq.inner)
                },
                _ => { Err(ExpressionError::new(&format!("Unknown sequence type {}", seq.id)))}
            }
        },
        Tag::ExplicitTag(exp_tag) => {
            build_expression_from_explicit_tag(exp_tag)
        },
        Tag::OctetString(oct_string) => {
            build_expression_from_octet_string(oct_string)
        },
        _ => { Err(ExpressionError::new(&format!("Unexpected tag type"))) }

    }
}

fn build_expression_from_explicit_tag(exp_tag : &ExplicitTag) -> Result<Expression, ExpressionError> {
    match exp_tag.id {
        ldap_parser::TAG_NOT =>
        {
            match build_expression(&exp_tag.inner) {
                Ok(inner_expression) => Ok(Expression::Not(Box::new(inner_expression))),
                Err(err) => Err(err)
            }
        },
        _ => Err(ExpressionError::new(&format!("Unexpected tag type {}", exp_tag.id)))
    }
}

fn build_expression_from_octet_string(oct_string : &OctetString) -> Result<Expression, ExpressionError> {
    match oct_string.id {
        ldap_parser::TAG_PRESENT => 
            {
                match str::from_utf8(&oct_string.inner) {
                    Ok(s) => Ok(Expression::Present(
                        match parse_prop_ref(s) {
                            Ok(prop_ref) => prop_ref,
                            Err(prop_err) => { return Err(ExpressionError::new(&format!("Error parsing property reference {}: {}", s, prop_err))) }
                        }
                        )),
                    Err(_err) => Err(ExpressionError::new("Parsing UTF8 from byte array failed"))
                }
            }
        _ => Err(ExpressionError::new(&format!("Unexpected tag type {}", oct_string.id)))
    }
}


fn build_multi_expression(expr_type: u64, sequence : &Vec<Tag>) -> Result<Expression, ExpressionError> {
    let mut expr_vec = vec![];

    for tag in sequence {
        match build_expression(tag) {
            Ok(expr) => { expr_vec.push(Box::new(expr)); },
            Err(err) => { return Err(err) }
        }
    }

    match expr_type {
        ldap_parser::TAG_AND => Ok(Expression::And(expr_vec)),
        ldap_parser::TAG_OR => Ok(Expression::Or(expr_vec)),
        _ => Err(ExpressionError::new(&format!("Unknown expression type {}", expr_type)))
    }
        
}

fn build_simple_expression(expr_type: u64, sequence : &Vec<Tag>) -> Result<Expression, ExpressionError> {
    match extract_two_octet_strings(sequence) {
        Ok(result) => 
        {
            match expr_type {
                ldap_parser::TAG_EQUAL => 
                    Ok(Expression::Equals(
                        match parse_prop_ref(result.0) {
                            Ok(prop_ref) => prop_ref,
                            Err(prop_err) => { return Err(ExpressionError::new(&format!("Error parsing property reference {}: {}", result.0, prop_err))) }
                        }, 
                        String::from(result.1))),
                // TODO add other binary operators handling here
                _ => Err(ExpressionError::new(&format!("Unknown expression type {}", expr_type)))
            }
        },
        Err(err) => Err(err)
    }
}

fn extract_str_from_octet_string<'a>(tag : &'a Tag) -> Result<&'a str, ExpressionError> {
    match tag {
        Tag::OctetString(oct) => {
            match str::from_utf8(&oct.inner) {
                Ok(s) => { Ok(s) },
                Err(_) => { Err(ExpressionError::new("Parsing UTF8 from byte array failed")) }
            }
        },
        _ => { Err(ExpressionError::new("Unexpected Tag type, expected OctetString")) }
    }
}

fn extract_two_octet_strings<'a>(sequence : &'a Vec<Tag>) -> Result<(&'a str, &'a str), ExpressionError> {
    if sequence.len() >= 2 {
        let attr : &'a str;
        let val : &'a str;
        
        match extract_str_from_octet_string(&sequence[0]) {
            Ok(s) => { attr = s; },
            Err(err) => { return Err(err) }
        }

        match extract_str_from_octet_string(&sequence[1]) {
            Ok(s) => { val = s; },
            Err(err) => { return Err(err) }
        }

        Ok((attr, val))
    }
    else {
        Err(ExpressionError::new(&format!("Expected 2 tags, got {} tags", sequence.len())))
    }

}

