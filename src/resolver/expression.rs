use std::str;

use asnom::structures::{Tag, OctetString, ExplicitTag};

use super::properties::{Property, PropertySet, PropertyRef, PropertyValue, parse_prop_ref };
use super::errors::{ResolveError, ExpressionError};
use super::ldap_parser;

// Expression resolution result enum
#[derive(Debug, Clone, PartialEq)]
pub enum ResolveResult {
    True,
    False(Vec<String>), // List of props which couldn't be resolved
    Undefined(Vec<String>), // List of props which couldn't be resolved
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
    // (DONE) It may be useful to return list of properties which couldn't be resolved 
    // (DONE) Properties of some simple types plus binary operators.  
    // TODO: Handling of Version simple type, need to implement operators
    // TODO: Handling of List type, and equals operator (ignore other comparison operators)
    // TODO: wildcard matching of property values
    // TODO: wildcard matching of value-less properties
    // TODO: aspects
    // TODO: finalize and review the matching relation implementations
    pub fn resolve<'a>(&'a self, property_set : &'a PropertySet) -> ResolveResult {
        match self {
            Expression::Equals(attr, val) => {
                self.resolve_with_function(attr, val, property_set, |prop_value : &PropertyValue, val : &str| -> bool { prop_value.equals(val) } )
            },
            Expression::Less(attr, val) => {
                self.resolve_with_function(attr, val, property_set, |prop_value : &PropertyValue, val : &str| -> bool { prop_value.less(val) } )
            },
            Expression::LessEqual(attr, val) => {
                self.resolve_with_function(attr, val, property_set, |prop_value : &PropertyValue, val : &str| -> bool { prop_value.less_equal(val) } )
            },
            Expression::Greater(attr, val) => {
                self.resolve_with_function(attr, val, property_set, |prop_value : &PropertyValue, val : &str| -> bool { prop_value.greater(val) } )
            },
            Expression::GreaterEqual(attr, val) => {
                self.resolve_with_function(attr, val, property_set, |prop_value : &PropertyValue, val : &str| -> bool { prop_value.greater_equal(val) } )
            },
            // other binary operators here if needed...
            Expression::And(inner_expressions) => {
                self.resolve_and(inner_expressions, property_set)
            },
            Expression::Or(inner_expressions) => {
                self.resolve_or(inner_expressions, property_set)
            },
            Expression::Not(inner_expression) => {
                match inner_expression.resolve(property_set) {
                    ResolveResult::True => ResolveResult::False(vec![]),
                    ResolveResult::False(_) => ResolveResult::True,
                    ResolveResult::Undefined(un_props) => ResolveResult::Undefined(un_props),
                    ResolveResult::Err(err) => ResolveResult::Err(err)
                }
            },
            Expression::Present(attr) => {
                self.resolve_present(attr, property_set)
            },
            _ => ResolveResult::Err(ResolveError::new(&format!("Unexpected Expression type {:?}", self)))
        }
    }

    fn resolve_with_function<'a>(&'a self, attr : &'a PropertyRef, val : &str, property_set : &'a PropertySet, oper_function : impl Fn(&PropertyValue, &str) -> bool) -> ResolveResult  {
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
                                if oper_function(value, val) {
                                    ResolveResult::True
                                }
                                else
                                {
                                    ResolveResult::False(vec![])
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
                                            ResolveResult::False(vec![])
                                        }
                                    },
                                    None => {
                                        ResolveResult::Undefined(vec![name.to_string()])
                                    }
                                }
                            },
                        }

                    },
                    Property::Implicit(_name) => {
                        ResolveResult::Undefined(vec![name.to_string()])
                    }
                }
            },
            None => {
                ResolveResult::Undefined(vec![name.to_string()])
            }
        }

    }

    fn resolve_and<'a>(&'a self, seq : &'a Vec<Box<Expression>>, property_set : &'a PropertySet) -> ResolveResult {
        let mut all_un_props = vec![];
        for exp in seq {
            match exp.resolve(property_set) {
                ResolveResult::True => { /* do nothing, keep iterating */ },
                ResolveResult::False(mut un_props) => { 
                        all_un_props.append(& mut un_props);
                        return ResolveResult::False(un_props) 
                    },
                ResolveResult::Undefined(mut un_props) => { 
                        all_un_props.append(& mut un_props);
                        return ResolveResult::Undefined(all_un_props) 
                    },
                ResolveResult::Err(err) => { return ResolveResult::Err(err) }
            }
        }

        ResolveResult::True
    }

    fn resolve_or<'a>(&'a self, seq : &'a Vec<Box<Expression>>, property_set : &'a PropertySet) -> ResolveResult {
        let mut all_un_props = vec![];
        for exp in seq {
            match exp.resolve(property_set) {
                ResolveResult::True => { return ResolveResult::True },
                ResolveResult::False(mut un_props) => { 
                        all_un_props.append(& mut un_props);
                    /* keep iterating */ 
                    },
                ResolveResult::Undefined(mut un_props) => { 
                        all_un_props.append(& mut un_props);
                        return ResolveResult::Undefined(all_un_props) 
                    },
                ResolveResult::Err(err) => { return ResolveResult::Err(err) }
            }
        }

        ResolveResult::False(all_un_props)
    }

    fn resolve_equals<'a>(&'a self, attr : &'a PropertyRef, val : &str, property_set : &PropertySet) -> ResolveResult {
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
                                    ResolveResult::False(vec![])
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
                                            ResolveResult::False(vec![])
                                        }
                                    },
                                    None => {
                                        ResolveResult::Undefined(vec![name.to_string()])
                                    }
                                }
                            },
                        }

                    },
                    Property::Implicit(_name) => {
                        ResolveResult::Undefined(vec![name.to_string()])
                    }
                }
            },
            None => {
                ResolveResult::Undefined(vec![name.to_string()])
            }
        }
    }

    // Resolve property/aspect presence
    fn resolve_present(&self, attr : &PropertyRef, property_set : &PropertySet) -> ResolveResult {
        match attr {
            // for value reference - only check if property exists in PropertySet
            PropertyRef::Value(name) => {
                match property_set.properties.get(&name[..]) {
                    Some(_value) => {
                        ResolveResult::True
                    },
                    None => {
                        ResolveResult::False(vec![name.to_string()])
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
                                        ResolveResult::False(vec![format!("{}[{}]", name, aspect)])
                                    }
                                }
                            },
                            Property::Implicit(_name) => { // no aspects for implicit properties
                                ResolveResult::False(vec![format!("{}[{}]", name, aspect)])
                            }
                        }
                    },
                    None => {
                        ResolveResult::False(vec![format!("{}[{}]", name, aspect)])
                    }
                }
            }
        }
    }


}


// #region Expression building

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
                ldap_parser::TAG_EQUAL | ldap_parser::TAG_LESS | ldap_parser::TAG_LESS_EQUAL | ldap_parser::TAG_GREATER | ldap_parser::TAG_GREATER_EQUAL => {
                    build_simple_expression(seq.id, &seq.inner)
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
            let prop_ref = match parse_prop_ref(result.0) {
                            Ok(prop_ref) => prop_ref,
                            Err(prop_err) => { return Err(ExpressionError::new(&format!("Error parsing property reference {}: {}", result.0, prop_err))) }
                        };
            match expr_type {
                ldap_parser::TAG_EQUAL => 
                    Ok(Expression::Equals(
                        prop_ref, 
                        String::from(result.1))),
                ldap_parser::TAG_GREATER => 
                    Ok(Expression::Greater(
                        prop_ref, 
                        String::from(result.1))),
                ldap_parser::TAG_GREATER_EQUAL => 
                    Ok(Expression::GreaterEqual(
                        prop_ref, 
                        String::from(result.1))),
                ldap_parser::TAG_LESS => 
                    Ok(Expression::Less(
                        prop_ref, 
                        String::from(result.1))),
                ldap_parser::TAG_LESS_EQUAL => 
                    Ok(Expression::LessEqual(
                        prop_ref, 
                        String::from(result.1))),
                // add other binary operators handling here
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

// #endregion