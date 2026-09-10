use std::fmt;

use thiserror::Error;

use super::expression::{ComparisonOperator, Expression, ExpressionBuilder, NodeId};
use super::properties::parse_prop_ref;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct ParseLimits {
    pub max_input_bytes: usize,
    pub max_nodes: usize,
    pub max_depth: usize,
    pub max_atom_bytes: usize,
}

impl Default for ParseLimits {
    fn default() -> Self {
        Self {
            max_input_bytes: 64 * 1024,
            max_nodes: 4 * 1024,
            max_depth: 128,
            max_atom_bytes: 16 * 1024,
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Limit {
    InputBytes,
    Nodes,
    Depth,
    AtomBytes,
}

impl fmt::Display for Limit {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(match self {
            Limit::InputBytes => "input bytes",
            Limit::Nodes => "expression nodes",
            Limit::Depth => "expression depth",
            Limit::AtomBytes => "atom bytes",
        })
    }
}

#[derive(Clone, Debug, Error, PartialEq, Eq)]
pub enum ParseErrorKind {
    #[error("empty input")]
    EmptyInput,
    #[error("expected '{expected}'")]
    Expected { expected: &'static str },
    #[error("unexpected end of input")]
    UnexpectedEnd,
    #[error("unexpected trailing input")]
    TrailingInput,
    #[error("empty property reference")]
    EmptyProperty,
    #[error("invalid property reference")]
    InvalidProperty,
    #[error("unsupported comparison operator")]
    UnsupportedOperator,
    #[error("limit exceeded: {limit}")]
    LimitExceeded { limit: Limit },
}

#[derive(Clone, Debug, Error, PartialEq, Eq)]
#[error("{kind} at byte {offset}")]
pub struct ParseError {
    pub kind: ParseErrorKind,
    pub offset: usize,
}

impl ParseError {
    fn new(kind: ParseErrorKind, offset: usize) -> Self {
        Self { kind, offset }
    }

    fn limit(limit: Limit, offset: usize) -> Self {
        Self::new(ParseErrorKind::LimitExceeded { limit }, offset)
    }
}

#[derive(Clone, Copy)]
enum GroupOperator {
    And,
    Or,
    Not,
}

struct GroupFrame {
    operator: GroupOperator,
    children_start: usize,
    open_offset: usize,
}

pub fn parse(input: &str) -> Result<Expression, ParseError> {
    parse_with_limits(input, ParseLimits::default())
}

pub fn parse_with_limits(input: &str, limits: ParseLimits) -> Result<Expression, ParseError> {
    if input.len() > limits.max_input_bytes {
        return Err(ParseError::limit(Limit::InputBytes, limits.max_input_bytes));
    }
    if input.is_empty() {
        return Err(ParseError::new(ParseErrorKind::EmptyInput, 0));
    }

    Parser {
        input,
        bytes: input.as_bytes(),
        position: 0,
        limits,
        builder: ExpressionBuilder::default(),
        groups: Vec::new(),
        children: Vec::new(),
        completed: None,
        root: None,
    }
    .parse()
}

struct Parser<'a> {
    input: &'a str,
    bytes: &'a [u8],
    position: usize,
    limits: ParseLimits,
    builder: ExpressionBuilder,
    groups: Vec<GroupFrame>,
    children: Vec<NodeId>,
    completed: Option<NodeId>,
    root: Option<NodeId>,
}

impl Parser<'_> {
    fn parse(mut self) -> Result<Expression, ParseError> {
        loop {
            if let Some(node) = self.completed.take() {
                if !self.groups.is_empty() {
                    self.children.push(node);
                } else if self.root.replace(node).is_some() {
                    return Err(ParseError::new(
                        ParseErrorKind::TrailingInput,
                        self.position,
                    ));
                } else {
                    self.skip_whitespace();
                    if self.position != self.bytes.len() {
                        return Err(ParseError::new(
                            ParseErrorKind::TrailingInput,
                            self.position,
                        ));
                    }
                    return Ok(self.builder.finish(node));
                }
            }

            if !self.groups.is_empty() {
                self.skip_whitespace();
                let Some(group) = self.groups.last() else {
                    continue;
                };
                let group_operator = group.operator;
                let child_count = self.children.len() - group.children_start;
                match group_operator {
                    GroupOperator::Not if child_count == 1 => {
                        self.expect_close_parenthesis()?;
                        let Some(group) = self.groups.pop() else {
                            return Err(ParseError::new(
                                ParseErrorKind::UnexpectedEnd,
                                self.position,
                            ));
                        };
                        let child = self.children[group.children_start];
                        self.children.truncate(group.children_start);
                        self.ensure_node_capacity()?;
                        self.completed = Some(self.builder.push_not(child));
                    }
                    GroupOperator::And | GroupOperator::Or if self.peek() == Some(b')') => {
                        self.position += 1;
                        let Some(group) = self.groups.pop() else {
                            return Err(ParseError::new(
                                ParseErrorKind::UnexpectedEnd,
                                self.position,
                            ));
                        };
                        self.ensure_node_capacity()?;
                        let node = match group.operator {
                            GroupOperator::And => self
                                .builder
                                .push_and(&self.children[group.children_start..]),
                            GroupOperator::Or => {
                                self.builder.push_or(&self.children[group.children_start..])
                            }
                            GroupOperator::Not => {
                                return Err(ParseError::new(
                                    ParseErrorKind::Expected { expected: ")" },
                                    self.position,
                                ));
                            }
                        };
                        self.completed = Some(node);
                        self.children.truncate(group.children_start);
                    }
                    _ => self.parse_expression_start()?,
                }
            } else if self.root.is_none() {
                self.skip_whitespace();
                self.parse_expression_start()?;
            } else {
                return Err(ParseError::new(
                    ParseErrorKind::TrailingInput,
                    self.position,
                ));
            }
        }
    }

    fn parse_expression_start(&mut self) -> Result<(), ParseError> {
        if self.groups.len() + 1 > self.limits.max_depth {
            return Err(ParseError::limit(Limit::Depth, self.position));
        }
        if self.peek() != Some(b'(') {
            return Err(ParseError::new(
                ParseErrorKind::Expected { expected: "(" },
                self.position,
            ));
        }

        let open_offset = self.position;
        self.position += 1;

        // The historical grammar accepts whitespace around group operators.
        self.skip_whitespace();
        match self.peek() {
            None => Err(ParseError::new(
                ParseErrorKind::UnexpectedEnd,
                self.position,
            )),
            Some(b')') => {
                self.position += 1;
                self.ensure_node_capacity()?;
                self.completed = Some(self.builder.push_empty(true));
                Ok(())
            }
            Some(b'&') => {
                self.position += 1;
                self.groups.push(GroupFrame {
                    operator: GroupOperator::And,
                    children_start: self.children.len(),
                    open_offset,
                });
                Ok(())
            }
            Some(b'|') => {
                self.position += 1;
                self.groups.push(GroupFrame {
                    operator: GroupOperator::Or,
                    children_start: self.children.len(),
                    open_offset,
                });
                Ok(())
            }
            Some(b'!') => {
                self.position += 1;
                self.groups.push(GroupFrame {
                    operator: GroupOperator::Not,
                    children_start: self.children.len(),
                    open_offset,
                });
                Ok(())
            }
            Some(_) => self.parse_predicate(),
        }
    }

    fn parse_predicate(&mut self) -> Result<(), ParseError> {
        let property_start = self.position;
        while let Some(byte) = self.peek() {
            if matches!(byte, b'=' | b'<' | b'>' | b'~' | b')') {
                break;
            }
            self.position += 1;
        }

        if self.position == property_start {
            return Err(ParseError::new(
                ParseErrorKind::EmptyProperty,
                property_start,
            ));
        }
        self.ensure_atom_limit(property_start, self.position)?;

        let property = parse_prop_ref(&self.input[property_start..self.position])
            .map_err(|_| ParseError::new(ParseErrorKind::InvalidProperty, property_start))?;
        let operator = self.parse_operator()?;
        let value_start = self.position;

        while self.peek().is_some_and(|byte| byte != b')') {
            self.position += 1;
        }
        if self.peek().is_none() {
            return Err(ParseError::new(
                ParseErrorKind::UnexpectedEnd,
                self.position,
            ));
        }
        self.ensure_atom_limit(value_start, self.position)?;
        let value = &self.input[value_start..self.position];
        self.position += 1;

        self.ensure_node_capacity()?;
        // The legacy grammar allowed ASCII whitespace between the `*` and
        // the closing parenthesis of a presence filter.
        let legacy_presence = value
            .strip_prefix('*')
            .is_some_and(|suffix| suffix.bytes().all(|byte| byte.is_ascii_whitespace()));
        self.completed = Some(
            if operator == ComparisonOperator::Equal && legacy_presence {
                self.builder.push_present(property)
            } else {
                self.builder
                    .push_compare(property, operator, value.to_owned())
            },
        );

        Ok(())
    }

    fn parse_operator(&mut self) -> Result<ComparisonOperator, ParseError> {
        let offset = self.position;
        let remaining = &self.bytes[self.position..];
        let (operator, width) = if remaining.starts_with(b"<=") {
            (ComparisonOperator::LessEqual, 2)
        } else if remaining.starts_with(b">=") {
            (ComparisonOperator::GreaterEqual, 2)
        } else if remaining.starts_with(b"<>") {
            (ComparisonOperator::NotEqual, 2)
        } else if remaining.starts_with(b"=") {
            (ComparisonOperator::Equal, 1)
        } else if remaining.starts_with(b"<") {
            (ComparisonOperator::Less, 1)
        } else if remaining.starts_with(b">") {
            (ComparisonOperator::Greater, 1)
        } else if remaining.starts_with(b"~") {
            return Err(ParseError::new(ParseErrorKind::UnsupportedOperator, offset));
        } else {
            return Err(ParseError::new(
                ParseErrorKind::Expected {
                    expected: "comparison operator",
                },
                offset,
            ));
        };
        self.position += width;
        Ok(operator)
    }

    fn expect_close_parenthesis(&mut self) -> Result<(), ParseError> {
        match self.peek() {
            Some(b')') => {
                self.position += 1;
                Ok(())
            }
            None => {
                let offset = self
                    .groups
                    .last()
                    .map(|group| group.open_offset)
                    .unwrap_or(self.position);
                Err(ParseError::new(ParseErrorKind::UnexpectedEnd, offset))
            }
            Some(_) => Err(ParseError::new(
                ParseErrorKind::Expected { expected: ")" },
                self.position,
            )),
        }
    }

    fn ensure_node_capacity(&self) -> Result<(), ParseError> {
        if self.builder.node_count() >= self.limits.max_nodes {
            Err(ParseError::limit(Limit::Nodes, self.position))
        } else {
            Ok(())
        }
    }

    fn ensure_atom_limit(&self, start: usize, end: usize) -> Result<(), ParseError> {
        if end - start > self.limits.max_atom_bytes {
            Err(ParseError::limit(Limit::AtomBytes, start))
        } else {
            Ok(())
        }
    }

    fn skip_whitespace(&mut self) {
        while self.peek().is_some_and(|byte| byte.is_ascii_whitespace()) {
            self.position += 1;
        }
    }

    fn peek(&self) -> Option<u8> {
        self.bytes.get(self.position).copied()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn rejects_trailing_input() {
        let error = parse("(a=b) trailing").unwrap_err();
        assert_eq!(error.kind, ParseErrorKind::TrailingInput);
    }

    #[test]
    fn rejects_depth_before_recursing() {
        let input = format!("{}(a=b){}", "(!".repeat(32), ")".repeat(32));
        let mut limits = ParseLimits::default();
        limits.max_depth = 16;
        let error = parse_with_limits(&input, limits).unwrap_err();
        assert_eq!(
            error.kind,
            ParseErrorKind::LimitExceeded {
                limit: Limit::Depth
            }
        );
    }

    #[test]
    fn rejects_wide_expressions_at_the_node_limit() {
        let input = format!("(&{})", "(a=b)".repeat(5_000));
        let error = parse(&input).unwrap_err();
        assert_eq!(
            error.kind,
            ParseErrorKind::LimitExceeded {
                limit: Limit::Nodes
            }
        );
    }

    #[test]
    fn deeply_nested_input_is_handled_on_a_small_stack() {
        let input = format!("{}(a=b){}", "(!".repeat(10_000), ")".repeat(10_000));
        let handle = std::thread::Builder::new()
            .stack_size(64 * 1024)
            .spawn(move || parse(&input))
            .unwrap();
        let error = handle.join().unwrap().unwrap_err();
        assert!(matches!(
            error.kind,
            ParseErrorKind::LimitExceeded {
                limit: Limit::Depth | Limit::InputBytes
            }
        ));
    }

    #[test]
    fn parsing_evaluation_and_drop_are_stack_safe() {
        let nesting = 10_000;
        let input = format!("{}(a=b){}", "(!".repeat(nesting), ")".repeat(nesting));
        let handle = std::thread::Builder::new()
            .stack_size(64 * 1024)
            .spawn(move || {
                let mut limits = ParseLimits::default();
                limits.max_nodes = nesting + 1;
                limits.max_depth = nesting + 1;
                let expression = parse_with_limits(&input, limits).unwrap();
                let properties = vec!["a=\"b\"".to_owned()];
                let property_set =
                    super::super::properties::PropertySet::from_flat_props(&properties).unwrap();
                assert!(matches!(
                    expression.resolve(&property_set),
                    super::super::expression::ResolveResult::True
                ));
            })
            .unwrap();
        handle.join().unwrap();
    }

    #[test]
    fn parses_not_equal_as_a_real_operator() {
        let expression = parse("(a<>b)").unwrap();
        assert_eq!(
            expression,
            Expression::NotEquals(
                super::super::properties::PropertyRef::Value(
                    "a".to_owned(),
                    super::super::properties::PropertyRefType::Any,
                ),
                "b".to_owned(),
            )
        );
    }

    #[test]
    fn preserves_whitespace_tolerant_presence_filter() {
        let property = super::super::properties::PropertyRef::Value(
            "golem.foo".to_owned(),
            super::super::properties::PropertyRefType::Any,
        );

        assert_eq!(
            parse("(golem.foo=* \t)").unwrap(),
            Expression::Present(property)
        );
    }
}
