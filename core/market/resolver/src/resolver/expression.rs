use std::str;

use asnom::structures::{ExplicitTag, OctetString, Tag};

use super::error::{ExpressionError, ResolveError};
use super::ldap_parser;
use super::properties::{parse_prop_ref, Property, PropertyRef, PropertySet, PropertyValue};

// Expression resolution result enum
#[derive(Debug, Clone, PartialEq)]
pub enum ResolveResult<'a> {
    True,
    False(Vec<&'a PropertyRef>, Expression), // List of prop references which couldn't be resolved, Reduced expression
    Undefined(Vec<&'a PropertyRef>, Expression), // List of props which couldn't be resolved (name, aspect), Reduced expression
    Err(ResolveError),
}

// Expression structure is the vehicle for LDAP filter expression resolution
#[derive(Clone, Debug, PartialEq)]
pub enum Expression {
    Equals(PropertyRef, String),       // property ref, value
    Greater(PropertyRef, String),      // property ref, value
    GreaterEqual(PropertyRef, String), // property ref, value
    Less(PropertyRef, String),         // property ref, value
    LessEqual(PropertyRef, String),    // property ref, value
    Present(PropertyRef),              // property ref
    Or(Vec<Expression>),               // operands
    And(Vec<Expression>),              // operands
    Not(Box<Expression>),              // operand
    Empty(bool),                       // empty expression of specific logical value (true/false)
}

impl Expression {
    // Resolve the expression with a give PropertySet and return the reduced result or error message.
    pub fn resolve_reduce<'a>(
        &'a self,
        property_set: &'a PropertySet,
    ) -> Result<Expression, String> {
        match self.resolve(property_set) {
            ResolveResult::True => Ok(Expression::Empty(true)),
            ResolveResult::False(_, expr) => Ok(expr),
            ResolveResult::Undefined(_, expr) => Ok(expr),
            ResolveResult::Err(err) => Err(err.msg),
        }
    }

    // Resolve the expression to bool value if possible (ie. if an expression has a known boolean value)
    pub fn to_value(&self) -> Option<bool> {
        match self {
            Expression::Empty(val) => Some(*val),
            _ => None,
        }
    }

    // Resolve the expression, returning:
    // Ok(Some(bool)) if expression can be resolved to boolean value (ie can be reduced to Empty(bool))
    // Ok(None) if expression does not reduce to Empty(bool)
    // Err(String) in case of resolution error
    pub fn resolve_api<'a>(
        &'a self,
        property_set: &'a PropertySet,
    ) -> Result<Option<bool>, String> {
        Ok(self.resolve_reduce(property_set)?.to_value())
    }

    // Fetch all property references from the expression
    pub fn property_refs(&self) -> impl IntoIterator<Item = &PropertyRef> {
        match self {
            Expression::Equals(prop, _)
            | Expression::Greater(prop, _)
            | Expression::GreaterEqual(prop, _)
            | Expression::Less(prop, _)
            | Expression::LessEqual(prop, _)
            | Expression::Present(prop) => vec![prop],
            Expression::And(exprs) | Expression::Or(exprs) => {
                exprs.iter().flat_map(|expr| expr.property_refs()).collect()
            }
            Expression::Not(expr) => expr.property_refs().into_iter().collect(),
            Expression::Empty(_) => vec![],
        }
    }

    // TODO: Implement ultimate reduction of AND and OR expressions where only one factor remains

    // (DONE) Rework for adjusted property definition syntax (property types derived form literals)
    // (DONE) Implement strong resolution and expression 'reduce' (ie. undefined results are propagated rather than ignored)
    //       - (DONE) Refactor ResolveResult to include vector of PropertyRefs rather than plain strings...
    // (DONE) Implement handling of unreduced expressions in "matching" results.
    // (DONE) It may be useful to return list of properties which couldn't be resolved
    // (DONE) Properties of some simple types plus binary operators.
    // (DONE) Handling of Version simple type, need to implement operators.
    // (DONE) Handling of Decimal simple type
    // (DONE) Handling of List property type
    // (DONE) equals operator for List property type with single operand (ignore other comparison operators)
    // (DONE) equals operator for List property type with List operand (list equivalence operator)
    // TODO: Handling of types in constraint filter expressions
    // TODO: Implement allowed characters in property names
    // (DONE) Rework resolve so that ResolveResult is based on strs and not Strings
    // (DONE) wildcard matching of property values
    // TODO: wildcard matching of value-less properties
    //       - Implement dynamic property "handler" - via trait?
    // (DONE) aspects
    // TODO: finalize and review the matching relation implementations
    pub fn resolve<'a>(&'a self, property_set: &'a PropertySet) -> ResolveResult {
        match self {
            Expression::Equals(attr, val) => self.resolve_with_function(
                attr,
                val,
                property_set,
                |prop_value: &PropertyValue, val: &str| -> bool { prop_value.equals(val) },
            ),
            Expression::Less(attr, val) => self.resolve_with_function(
                attr,
                val,
                property_set,
                |prop_value: &PropertyValue, val: &str| -> bool { prop_value.less(val) },
            ),
            Expression::LessEqual(attr, val) => self.resolve_with_function(
                attr,
                val,
                property_set,
                |prop_value: &PropertyValue, val: &str| -> bool { prop_value.less_equal(val) },
            ),
            Expression::Greater(attr, val) => self.resolve_with_function(
                attr,
                val,
                property_set,
                |prop_value: &PropertyValue, val: &str| -> bool { prop_value.greater(val) },
            ),
            Expression::GreaterEqual(attr, val) => self.resolve_with_function(
                attr,
                val,
                property_set,
                |prop_value: &PropertyValue, val: &str| -> bool { prop_value.greater_equal(val) },
            ),
            // other binary operators here if needed...
            Expression::And(inner_expressions) => self.resolve_and(inner_expressions, property_set),
            Expression::Or(inner_expressions) => self.resolve_or(inner_expressions, property_set),
            Expression::Not(inner_expression) => match inner_expression.resolve(property_set) {
                ResolveResult::True => ResolveResult::False(vec![], Expression::Empty(false)),
                ResolveResult::False(_, _) => ResolveResult::True,
                ResolveResult::Undefined(un_props, unresolved_expr) => {
                    ResolveResult::Undefined(un_props, Expression::Not(Box::new(unresolved_expr)))
                }
                ResolveResult::Err(err) => ResolveResult::Err(err),
            },
            Expression::Present(attr) => self.resolve_present(attr, property_set),
            Expression::Empty(val) => {
                if *val {
                    ResolveResult::True
                } else {
                    ResolveResult::False(vec![], Expression::Empty(false))
                }
            }
        }
    }

    fn resolve_with_function<'a>(
        &'a self,
        prop_ref: &'a PropertyRef,
        val_string: &str,
        property_set: &'a PropertySet,
        oper_function: impl Fn(&PropertyValue, &str) -> bool,
    ) -> ResolveResult {
        // TODO this requires rewrite to cater for implicit properties...
        // test if property exists and then if the value matches

        // extract referred property name
        let name = match prop_ref {
            PropertyRef::Value(n, _) => n,
            PropertyRef::Aspect(n, _a, _) => n,
        };

        match property_set.properties.get(&name[..]) {
            Some(prop) => {
                match prop {
                    Property::Explicit(_name, value, aspects) => {
                        // now decide if we are referring to value or aspect
                        match prop_ref {
                            PropertyRef::Value(_n, impl_type) => {
                                match value.to_prop_ref_type(impl_type) {
                                    Ok(conv_result) => {
                                        let resolve_result = match conv_result {
                                            Some(val) => oper_function(&val, val_string),
                                            None => oper_function(value, val_string),
                                        };

                                        // resolve against prop value
                                        if resolve_result {
                                            ResolveResult::True
                                        } else {
                                            ResolveResult::False(vec![], Expression::Empty(false))
                                            // if resolved to false - return Empty as reduced expression
                                        }
                                    }
                                    Err(_) => {
                                        ResolveResult::Undefined(vec![], self.clone())
                                        // if resolved to undefined - return self copy as reduced expression (cannot reduce self)
                                    }
                                }
                            }
                            PropertyRef::Aspect(_n, aspect, _impl_type) => {
                                // resolve against prop aspect
                                match aspects.get(&aspect[..]) {
                                    Some(aspect_value) => {
                                        if val_string == *aspect_value {
                                            ResolveResult::True
                                        } else {
                                            ResolveResult::False(vec![], Expression::Empty(false))
                                            // if resolved to false - return Empty as reduced expression
                                        }
                                    }
                                    None => {
                                        ResolveResult::Undefined(vec![prop_ref], self.clone())
                                        // if resolved to undefined - return self copy as reduced expression (cannot reduce self)
                                    }
                                }
                            }
                        }
                    }
                    Property::Implicit(_name) => {
                        ResolveResult::Undefined(vec![prop_ref], self.clone()) // if resolved to undefined - return self copy as reduced expression (cannot reduce self)
                    }
                }
            }
            None => {
                ResolveResult::Undefined(vec![prop_ref], self.clone()) // if resolved to undefined - return self copy as reduced expression (cannot reduce self)
            }
        }
    }

    fn resolve_and<'a>(
        &'a self,
        seq: &'a Vec<Expression>,
        property_set: &'a PropertySet,
    ) -> ResolveResult {
        let mut undefined_found = false;
        let mut unresolved_refs = vec![];
        let mut unresolved_exprs = vec![]; // TODO this may be required if we want to resolve all factor expressions (instead of eager resolution)
        for exp in seq {
            match exp.resolve(property_set) {
                ResolveResult::True => { /* do nothing, keep iterating */ }
                ResolveResult::False(mut un_props, unresolved_expr) => {
                    unresolved_refs.append(&mut un_props);
                    match unresolved_expr {
                        Expression::Empty(_) => {}
                        _ => {
                            unresolved_exprs.push(unresolved_expr);
                        }
                    };
                    return ResolveResult::False(vec![], Expression::Empty(false));
                    // resolved properly to false - return Empty as unreduced expression
                }
                ResolveResult::Undefined(mut un_props, unresolved_expr) => {
                    unresolved_refs.append(&mut un_props);
                    match unresolved_expr {
                        Expression::Empty(_) => {}
                        _ => {
                            unresolved_exprs.push(unresolved_expr);
                        }
                    };
                    undefined_found = true;
                }
                ResolveResult::Err(err) => return ResolveResult::Err(err),
            }
        }

        if undefined_found {
            ResolveResult::Undefined(
                unresolved_refs,
                match unresolved_exprs.len().cmp(&1) {
                    std::cmp::Ordering::Greater => Expression::And(unresolved_exprs),
                    std::cmp::Ordering::Equal => unresolved_exprs.pop().unwrap(),
                    std::cmp::Ordering::Less => Expression::Empty(true),
                },
            )
        } else {
            ResolveResult::True
        }
    }

    fn resolve_or<'a>(
        &'a self,
        seq: &'a Vec<Expression>,
        property_set: &'a PropertySet,
    ) -> ResolveResult {
        let mut undefined_found = false;
        let mut all_un_props = vec![];
        let mut unresolved_exprs = vec![]; // TODO this may be required if we want to resolve all factor expressions (instead of eager resolution)
        for exp in seq {
            match exp.resolve(property_set) {
                ResolveResult::True => return ResolveResult::True,
                ResolveResult::False(mut un_props, unresolved_expr) => {
                    all_un_props.append(&mut un_props);
                    /* keep iterating */
                    // We accumulate the unresolved expressions in a list to return
                    match unresolved_expr {
                        Expression::Empty(_) => {}
                        _ => {
                            unresolved_exprs.push(unresolved_expr);
                        }
                    };
                }
                ResolveResult::Undefined(mut un_props, unresolved_expr) => {
                    all_un_props.append(&mut un_props);
                    match unresolved_expr {
                        Expression::Empty(_) => {}
                        _ => {
                            unresolved_exprs.push(unresolved_expr);
                        }
                    };
                    undefined_found = true;
                    //return ResolveResult::Undefined(all_un_props, Expression::Or(unresolved_exprs))
                }
                ResolveResult::Err(err) => return ResolveResult::Err(err),
            }
        }

        if undefined_found {
            ResolveResult::Undefined(
                all_un_props,
                match unresolved_exprs.len().cmp(&1) {
                    std::cmp::Ordering::Greater => Expression::Or(unresolved_exprs),
                    std::cmp::Ordering::Equal => unresolved_exprs.pop().unwrap(),
                    std::cmp::Ordering::Less => Expression::Empty(true),
                },
            )
        } else {
            ResolveResult::False(
                all_un_props,
                match unresolved_exprs.len().cmp(&1) {
                    std::cmp::Ordering::Greater => Expression::Or(unresolved_exprs),
                    std::cmp::Ordering::Equal => unresolved_exprs.pop().unwrap(),
                    std::cmp::Ordering::Less => Expression::Empty(false),
                },
            )
        }
    }

    // Resolve property/aspect presence
    fn resolve_present<'a>(
        &self,
        attr: &'a PropertyRef,
        property_set: &'a PropertySet,
    ) -> ResolveResult<'a> {
        match attr {
            // for value reference - only check if property exists in PropertySet
            PropertyRef::Value(name, _) => match property_set.properties.get(&name[..]) {
                Some(_value) => ResolveResult::True,
                None => ResolveResult::False(vec![attr], Expression::Empty(false)),
            },
            // for aspect reference - first check if property exists, then check for aspect
            PropertyRef::Aspect(name, aspect, _) => {
                match property_set.properties.get(&name[..]) {
                    Some(value) => {
                        match value {
                            Property::Explicit(_name, _val, aspects) => {
                                match aspects.get(&aspect[..]) {
                                    Some(_aspect) => ResolveResult::True,
                                    None => {
                                        ResolveResult::False(vec![attr], Expression::Empty(false))
                                    }
                                }
                            }
                            Property::Implicit(_name) => {
                                // no aspects for implicit properties
                                ResolveResult::False(vec![attr], self.clone())
                            }
                        }
                    }
                    None => ResolveResult::False(vec![attr], self.clone()),
                }
            }
        }
    }
}

// #region Expression building

pub fn build_expression(root: &Tag) -> Result<Expression, ExpressionError> {
    match root {
        Tag::Sequence(seq) => match seq.id {
            ldap_parser::TAG_AND => build_multi_expression(seq.id, &seq.inner),
            ldap_parser::TAG_OR => build_multi_expression(seq.id, &seq.inner),
            ldap_parser::TAG_EQUAL
            | ldap_parser::TAG_LESS
            | ldap_parser::TAG_LESS_EQUAL
            | ldap_parser::TAG_GREATER
            | ldap_parser::TAG_GREATER_EQUAL => build_simple_expression(seq.id, &seq.inner),
            _ => Err(ExpressionError::new(&format!(
                "Unknown sequence type {}",
                seq.id
            ))),
        },
        Tag::ExplicitTag(exp_tag) => build_expression_from_explicit_tag(exp_tag),
        Tag::OctetString(oct_string) => build_expression_from_octet_string(oct_string),
        Tag::Null(_) => Ok(Expression::Empty(true)),
        _ => Err(ExpressionError::new(&format!("Unexpected tag type"))),
    }
}

fn build_expression_from_explicit_tag(
    exp_tag: &ExplicitTag,
) -> Result<Expression, ExpressionError> {
    match exp_tag.id {
        ldap_parser::TAG_NOT => match build_expression(&exp_tag.inner) {
            Ok(inner_expression) => Ok(Expression::Not(Box::new(inner_expression))),
            Err(err) => Err(err),
        },
        _ => Err(ExpressionError::new(&format!(
            "Unexpected tag type {}",
            exp_tag.id
        ))),
    }
}

fn build_expression_from_octet_string(
    oct_string: &OctetString,
) -> Result<Expression, ExpressionError> {
    match oct_string.id {
        ldap_parser::TAG_PRESENT => match str::from_utf8(&oct_string.inner) {
            Ok(s) => Ok(Expression::Present(match parse_prop_ref(s) {
                Ok(prop_ref) => prop_ref,
                Err(prop_err) => {
                    return Err(ExpressionError::new(&format!(
                        "Error parsing property reference {}: {}",
                        s, prop_err
                    )))
                }
            })),
            Err(_err) => Err(ExpressionError::new("Parsing UTF8 from byte array failed")),
        },
        _ => Err(ExpressionError::new(&format!(
            "Unexpected tag type {}",
            oct_string.id
        ))),
    }
}

fn build_multi_expression(
    expr_type: u64,
    sequence: &Vec<Tag>,
) -> Result<Expression, ExpressionError> {
    let mut expr_vec = vec![];

    for tag in sequence {
        match build_expression(tag) {
            Ok(expr) => {
                expr_vec.push(expr);
            }
            Err(err) => return Err(err),
        }
    }

    match expr_type {
        ldap_parser::TAG_AND => Ok(Expression::And(expr_vec)),
        ldap_parser::TAG_OR => Ok(Expression::Or(expr_vec)),
        _ => Err(ExpressionError::new(&format!(
            "Unknown expression type {}",
            expr_type
        ))),
    }
}

fn build_simple_expression(
    expr_type: u64,
    sequence: &Vec<Tag>,
) -> Result<Expression, ExpressionError> {
    match extract_two_octet_strings(sequence) {
        Ok(result) => {
            let prop_ref = match parse_prop_ref(result.0) {
                Ok(prop_ref) => prop_ref,
                Err(prop_err) => {
                    return Err(ExpressionError::new(&format!(
                        "Error parsing property reference {}: {}",
                        result.0, prop_err
                    )))
                }
            };
            match expr_type {
                ldap_parser::TAG_EQUAL => Ok(Expression::Equals(prop_ref, String::from(result.1))),
                ldap_parser::TAG_GREATER => {
                    Ok(Expression::Greater(prop_ref, String::from(result.1)))
                }
                ldap_parser::TAG_GREATER_EQUAL => {
                    Ok(Expression::GreaterEqual(prop_ref, String::from(result.1)))
                }
                ldap_parser::TAG_LESS => Ok(Expression::Less(prop_ref, String::from(result.1))),
                ldap_parser::TAG_LESS_EQUAL => {
                    Ok(Expression::LessEqual(prop_ref, String::from(result.1)))
                }
                // add other binary operators handling here
                _ => Err(ExpressionError::new(&format!(
                    "Unknown expression type {}",
                    expr_type
                ))),
            }
        }
        Err(err) => Err(err),
    }
}

fn extract_str_from_octet_string<'a>(tag: &'a Tag) -> Result<&'a str, ExpressionError> {
    match tag {
        Tag::OctetString(oct) => match str::from_utf8(&oct.inner) {
            Ok(s) => Ok(s),
            Err(_) => Err(ExpressionError::new("Parsing UTF8 from byte array failed")),
        },
        _ => Err(ExpressionError::new(
            "Unexpected Tag type, expected OctetString",
        )),
    }
}

fn extract_two_octet_strings<'a>(
    sequence: &'a Vec<Tag>,
) -> Result<(&'a str, &'a str), ExpressionError> {
    if sequence.len() >= 2 {
        let attr: &'a str = extract_str_from_octet_string(&sequence[0])?;
        let val: &'a str = extract_str_from_octet_string(&sequence[1])?;

        Ok((attr, val))
    } else {
        Err(ExpressionError::new(&format!(
            "Expected 2 tags, got {} tags",
            sequence.len()
        )))
    }
}

// #endregion
