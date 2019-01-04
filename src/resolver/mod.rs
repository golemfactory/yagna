pub mod ldap_parser;
pub mod match_rel;

use std::default::Default;
use asnom::common::TagClass;
use asnom::structures::{Tag, OctetString, Sequence, ExplicitTag};

use std::collections::HashMap;
use std::error;
use std::fmt;
use std::str;

#[derive(Debug, Clone, PartialEq, Default)]
pub struct PropertySet{
    pub exp_properties : HashMap<String, String>,
    pub imp_properties : Vec<String>,
}

// ResolveError

#[derive(Debug, Clone, PartialEq)]
pub struct ResolveError {
    msg : String
}

impl ResolveError {
    fn new(message : &str) -> Self 
    {
        ResolveError{ msg : String::from(message) }
    }
}

impl fmt::Display for ResolveError {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        f.write_str(&self.msg)
    }
}

impl error::Error for ResolveError {
    fn description(&self) -> &str {
        &self.msg
    }

    fn cause(&self) -> Option<&error::Error> {
        // Generic error, underlying cause isn't tracked.
        None
    }
}

// ExpressionError

#[derive(Debug, Clone, PartialEq)]
pub struct ExpressionError {
    msg : String
}

impl ExpressionError {
    fn new(message : &str) -> Self 
    {
        ExpressionError{ msg : String::from(message) }
    }
}

impl fmt::Display for ExpressionError {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        f.write_str(&self.msg)
    }
}

impl error::Error for ExpressionError {
    fn description(&self) -> &str {
        &self.msg
    }

    fn cause(&self) -> Option<&error::Error> {
        // Generic error, underlying cause isn't tracked.
        None
    }
}


#[derive(Debug, Clone, PartialEq)]
pub enum ResolveResult {
    True,
    False,
    Undefined,
    Err(ResolveError)
}

#[derive(Clone, Debug, PartialEq)]
pub enum Expression {
    Equals(String, String), // property name, value
    Greater(String, String), // property name, value
    GreaterEqual(String, String), // property name, value
    Less(String, String), // property name, value
    LessEqual(String, String), // property name, value
    Present(String), // property name
    Or(Vec<Box<Expression>>), // operands
    And(Vec<Box<Expression>>), // operands
    Not(Box<Expression>), // operand
    Empty
}

impl Expression {
    // Implement strong resolution (ie. undefined results are propagated rather than ignored)
    // TODO: handle whitespace characters in expressions
    // TODO: properties of different types (and respective "equals" logic)
    // TODO: other comparison operators
    // TODO: wildcard matching of property values
    // TODO: wildcard matching of value-less properties
    // TODO: aspects
    pub fn resolve(&self, property_set : &PropertySet) -> ResolveResult {
        match self {
            Expression::Equals(attr, val) => {
                self.resolve_equals(attr, val, property_set)
            },
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
                match property_set.exp_properties.get(attr) {
                    Some(_value) => {
                        ResolveResult::True
                    },
                    None => {
                        ResolveResult::False
                    }
                }

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

    fn resolve_equals(&self, attr : &str, val : &str, property_set : &PropertySet) -> ResolveResult {
        // test if attribute exists and then if the value matches
        match property_set.exp_properties.get(attr) {
            Some(value) => {
                if value == val {
                    ResolveResult::True
                }
                else
                {
                    ResolveResult::False
                }
            },
            None => {
                ResolveResult::Undefined
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
                    Ok(s) => Ok(Expression::Present(String::from(s))),
                    Err(err) => Err(ExpressionError::new("Parsing UTF8 from byte array failed"))
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
                    Ok(Expression::Equals(String::from(result.0), String::from(result.1))),
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