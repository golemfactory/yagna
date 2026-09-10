use std::ops::Range;

use super::error::{ExpressionError, ResolveError};
use super::properties::{Property, PropertyRef, PropertySet};

pub(crate) type NodeId = usize;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum ComparisonOperator {
    Equal,
    NotEqual,
    Greater,
    GreaterEqual,
    Less,
    LessEqual,
}

#[derive(Clone, Debug, PartialEq)]
pub(crate) enum Node {
    Empty(bool),
    Compare {
        property: PropertyRef,
        operator: ComparisonOperator,
        value: String,
    },
    Present(PropertyRef),
    And(Range<usize>),
    Or(Range<usize>),
    Not(NodeId),
}

/// A parsed constraint expression stored as a flat arena.
///
/// Nodes refer to their children by index. Consequently parsing, evaluation,
/// cloning, formatting and destruction do not recurse through user-controlled
/// nesting.
#[derive(Clone, Debug, PartialEq)]
pub struct Expression {
    pub(crate) nodes: Vec<Node>,
    pub(crate) edges: Vec<NodeId>,
    pub(crate) root: NodeId,
}

#[derive(Default)]
pub(crate) struct ExpressionBuilder {
    nodes: Vec<Node>,
    edges: Vec<NodeId>,
}

impl ExpressionBuilder {
    pub(crate) fn node_count(&self) -> usize {
        self.nodes.len()
    }

    pub(crate) fn push_empty(&mut self, value: bool) -> NodeId {
        self.push_node(Node::Empty(value))
    }

    pub(crate) fn push_compare(
        &mut self,
        property: PropertyRef,
        operator: ComparisonOperator,
        value: String,
    ) -> NodeId {
        self.push_node(Node::Compare {
            property,
            operator,
            value,
        })
    }

    pub(crate) fn push_present(&mut self, property: PropertyRef) -> NodeId {
        self.push_node(Node::Present(property))
    }

    pub(crate) fn push_not(&mut self, child: NodeId) -> NodeId {
        self.push_node(Node::Not(child))
    }

    pub(crate) fn push_and(&mut self, children: &[NodeId]) -> NodeId {
        let range = self.push_edges(children);
        self.push_node(Node::And(range))
    }

    pub(crate) fn push_or(&mut self, children: &[NodeId]) -> NodeId {
        let range = self.push_edges(children);
        self.push_node(Node::Or(range))
    }

    fn push_node(&mut self, node: Node) -> NodeId {
        let id = self.nodes.len();
        self.nodes.push(node);
        id
    }

    fn push_edges(&mut self, children: &[NodeId]) -> Range<usize> {
        let start = self.edges.len();
        self.edges.extend_from_slice(children);
        start..self.edges.len()
    }

    fn append_expression(&mut self, expression: Expression) -> NodeId {
        let node_offset = self.nodes.len();
        let edge_offset = self.edges.len();

        self.edges
            .extend(expression.edges.into_iter().map(|id| id + node_offset));
        self.nodes
            .extend(expression.nodes.into_iter().map(|node| match node {
                Node::Empty(value) => Node::Empty(value),
                Node::Compare {
                    property,
                    operator,
                    value,
                } => Node::Compare {
                    property,
                    operator,
                    value,
                },
                Node::Present(property) => Node::Present(property),
                Node::And(range) => {
                    Node::And((range.start + edge_offset)..(range.end + edge_offset))
                }
                Node::Or(range) => Node::Or((range.start + edge_offset)..(range.end + edge_offset)),
                Node::Not(child) => Node::Not(child + node_offset),
            }));

        expression.root + node_offset
    }

    fn copy_leaf(&mut self, node: &Node) -> NodeId {
        match node {
            Node::Empty(value) => self.push_empty(*value),
            Node::Compare {
                property,
                operator,
                value,
            } => self.push_compare(property.clone(), *operator, value.clone()),
            Node::Present(property) => self.push_present(property.clone()),
            Node::And(_) | Node::Or(_) | Node::Not(_) => {
                unreachable!("only leaf expressions can be copied during evaluation")
            }
        }
    }

    fn is_empty(&self, id: NodeId) -> bool {
        matches!(self.nodes[id], Node::Empty(_))
    }

    pub(crate) fn finish(self, root: NodeId) -> Expression {
        // Evaluation can abandon already-built residual branches after a
        // short-circuit. Compacting here produces a canonical, reachable-only
        // arena and keeps structural PartialEq deterministic.
        let mut reachable = vec![false; self.nodes.len()];
        let mut pending = vec![root];

        while let Some(id) = pending.pop() {
            if reachable[id] {
                continue;
            }
            reachable[id] = true;
            match &self.nodes[id] {
                Node::And(range) | Node::Or(range) => {
                    pending.extend(self.edges[range.clone()].iter().copied());
                }
                Node::Not(child) => pending.push(*child),
                Node::Empty(_) | Node::Compare { .. } | Node::Present(_) => {}
            }
        }

        let mut nodes = Vec::with_capacity(reachable.iter().filter(|value| **value).count());
        let mut edges = Vec::new();
        let mut remap = vec![None; self.nodes.len()];

        for (old_id, node) in self.nodes.into_iter().enumerate() {
            if !reachable[old_id] {
                continue;
            }

            let remapped = match node {
                Node::Empty(value) => Node::Empty(value),
                Node::Compare {
                    property,
                    operator,
                    value,
                } => Node::Compare {
                    property,
                    operator,
                    value,
                },
                Node::Present(property) => Node::Present(property),
                Node::And(range) => {
                    let start = edges.len();
                    edges.extend(
                        self.edges[range]
                            .iter()
                            .map(|child| remap[*child].expect("children precede their parent")),
                    );
                    Node::And(start..edges.len())
                }
                Node::Or(range) => {
                    let start = edges.len();
                    edges.extend(
                        self.edges[range]
                            .iter()
                            .map(|child| remap[*child].expect("children precede their parent")),
                    );
                    Node::Or(start..edges.len())
                }
                Node::Not(child) => Node::Not(remap[child].expect("children precede their parent")),
            };

            let new_id = nodes.len();
            nodes.push(remapped);
            remap[old_id] = Some(new_id);
        }

        Expression {
            nodes,
            edges,
            root: remap[root].expect("the root is reachable"),
        }
    }
}

impl Expression {
    fn comparison(property: PropertyRef, operator: ComparisonOperator, value: String) -> Self {
        let mut builder = ExpressionBuilder::default();
        let root = builder.push_compare(property, operator, value);
        builder.finish(root)
    }

    fn group(expressions: Vec<Expression>, is_and: bool) -> Self {
        let mut builder = ExpressionBuilder::default();
        let children: Vec<_> = expressions
            .into_iter()
            .map(|expression| builder.append_expression(expression))
            .collect();
        let root = if is_and {
            builder.push_and(&children)
        } else {
            builder.push_or(&children)
        };
        builder.finish(root)
    }

    // These compatibility constructors intentionally retain the previous
    // enum-variant spelling while creating a flat expression.
    #[allow(non_snake_case)]
    pub fn Empty(value: bool) -> Self {
        let mut builder = ExpressionBuilder::default();
        let root = builder.push_empty(value);
        builder.finish(root)
    }

    #[allow(non_snake_case)]
    pub fn Equals(property: PropertyRef, value: String) -> Self {
        Self::comparison(property, ComparisonOperator::Equal, value)
    }

    #[allow(non_snake_case)]
    pub fn NotEquals(property: PropertyRef, value: String) -> Self {
        Self::comparison(property, ComparisonOperator::NotEqual, value)
    }

    #[allow(non_snake_case)]
    pub fn Greater(property: PropertyRef, value: String) -> Self {
        Self::comparison(property, ComparisonOperator::Greater, value)
    }

    #[allow(non_snake_case)]
    pub fn GreaterEqual(property: PropertyRef, value: String) -> Self {
        Self::comparison(property, ComparisonOperator::GreaterEqual, value)
    }

    #[allow(non_snake_case)]
    pub fn Less(property: PropertyRef, value: String) -> Self {
        Self::comparison(property, ComparisonOperator::Less, value)
    }

    #[allow(non_snake_case)]
    pub fn LessEqual(property: PropertyRef, value: String) -> Self {
        Self::comparison(property, ComparisonOperator::LessEqual, value)
    }

    #[allow(non_snake_case)]
    pub fn Present(property: PropertyRef) -> Self {
        let mut builder = ExpressionBuilder::default();
        let root = builder.push_present(property);
        builder.finish(root)
    }

    #[allow(non_snake_case)]
    pub fn And(expressions: Vec<Expression>) -> Self {
        Self::group(expressions, true)
    }

    #[allow(non_snake_case)]
    pub fn Or(expressions: Vec<Expression>) -> Self {
        Self::group(expressions, false)
    }

    #[allow(non_snake_case)]
    #[allow(clippy::boxed_local)] // Compatibility with the former enum variant API.
    pub fn Not(expression: Box<Expression>) -> Self {
        let mut builder = ExpressionBuilder::default();
        let child = builder.append_expression(*expression);
        let root = builder.push_not(child);
        builder.finish(root)
    }

    pub fn resolve_reduce<'a>(
        &'a self,
        property_set: &'a PropertySet,
    ) -> Result<Expression, String> {
        match self.resolve(property_set) {
            ResolveResult::True => Ok(Expression::Empty(true)),
            ResolveResult::False(_, expression) | ResolveResult::Undefined(_, expression) => {
                Ok(expression)
            }
            ResolveResult::Err(error) => Err(error.msg),
        }
    }

    pub fn to_value(&self) -> Option<bool> {
        match self.nodes[self.root] {
            Node::Empty(value) => Some(value),
            _ => None,
        }
    }

    pub fn resolve_api<'a>(
        &'a self,
        property_set: &'a PropertySet,
    ) -> Result<Option<bool>, String> {
        Ok(self.resolve_reduce(property_set)?.to_value())
    }

    pub fn property_refs(&self) -> impl IntoIterator<Item = &PropertyRef> {
        self.nodes.iter().filter_map(|node| match node {
            Node::Compare { property, .. } | Node::Present(property) => Some(property),
            Node::Empty(_) | Node::And(_) | Node::Or(_) | Node::Not(_) => None,
        })
    }

    pub fn resolve<'a>(&'a self, property_set: &'a PropertySet) -> ResolveResult<'a> {
        let mut residual = ExpressionBuilder::default();
        let mut frames = vec![EvalFrame::Eval(self.root)];
        let mut result = None;

        while let Some(frame) = frames.pop() {
            match frame {
                EvalFrame::Eval(id) => match &self.nodes[id] {
                    Node::Empty(value) => {
                        result = Some(if *value {
                            EvalResult::true_value()
                        } else {
                            EvalResult::false_value(residual.push_empty(false))
                        });
                    }
                    Node::Compare {
                        property,
                        operator,
                        value,
                    } => {
                        result = Some(self.resolve_comparison(
                            id,
                            property,
                            *operator,
                            value,
                            property_set,
                            &mut residual,
                        ));
                    }
                    Node::Present(property) => {
                        result =
                            Some(self.resolve_present(id, property, property_set, &mut residual));
                    }
                    Node::Not(child) => {
                        frames.push(EvalFrame::Not);
                        frames.push(EvalFrame::Eval(*child));
                    }
                    Node::And(range) => {
                        if range.is_empty() {
                            result = Some(EvalResult::true_value());
                        } else {
                            frames.push(EvalFrame::Group(GroupFrame::new(
                                GroupOperator::And,
                                range.clone(),
                            )));
                            frames.push(EvalFrame::Eval(self.edges[range.start]));
                        }
                    }
                    Node::Or(range) => {
                        if range.is_empty() {
                            result = Some(EvalResult::false_value(residual.push_empty(false)));
                        } else {
                            frames.push(EvalFrame::Group(GroupFrame::new(
                                GroupOperator::Or,
                                range.clone(),
                            )));
                            frames.push(EvalFrame::Eval(self.edges[range.start]));
                        }
                    }
                },
                EvalFrame::Not => {
                    let child = result
                        .take()
                        .expect("a child result precedes its continuation");
                    result = Some(match child.kind {
                        EvalKind::True => EvalResult::false_value(residual.push_empty(false)),
                        EvalKind::False => EvalResult::true_value(),
                        EvalKind::Undefined => {
                            let root =
                                residual.push_not(child.residual.expect("undefined residual"));
                            EvalResult::undefined(child.refs, root)
                        }
                    });
                }
                EvalFrame::Group(mut group) => {
                    let child = result
                        .take()
                        .expect("a child result precedes its continuation");
                    if let Some(done) = group.consume(child, &mut residual) {
                        result = Some(done);
                        continue;
                    }

                    if group.next < group.range.end {
                        let child_id = self.edges[group.next];
                        group.next += 1;
                        frames.push(EvalFrame::Group(group));
                        frames.push(EvalFrame::Eval(child_id));
                    } else {
                        result = Some(group.finish(&mut residual));
                    }
                }
            }
        }

        let result = result.expect("every expression has a root result");
        match result.kind {
            EvalKind::True => ResolveResult::True,
            EvalKind::False => ResolveResult::False(
                result.refs,
                residual.finish(result.residual.expect("false residual")),
            ),
            EvalKind::Undefined => ResolveResult::Undefined(
                result.refs,
                residual.finish(result.residual.expect("undefined residual")),
            ),
        }
    }

    fn resolve_comparison<'a>(
        &'a self,
        id: NodeId,
        property_ref: &'a PropertyRef,
        operator: ComparisonOperator,
        value: &str,
        property_set: &'a PropertySet,
        residual: &mut ExpressionBuilder,
    ) -> EvalResult<'a> {
        let name = match property_ref {
            PropertyRef::Value(name, _) | PropertyRef::Aspect(name, _, _) => name,
        };

        let Some(property) = property_set.properties.get(&name[..]) else {
            let root = residual.copy_leaf(&self.nodes[id]);
            return EvalResult::undefined(vec![property_ref], root);
        };

        match property {
            Property::Implicit(_) => {
                let root = residual.copy_leaf(&self.nodes[id]);
                EvalResult::undefined(vec![property_ref], root)
            }
            Property::Explicit(_, property_value, aspects) => match property_ref {
                PropertyRef::Value(_, implied_type) => {
                    let converted = match property_value.to_prop_ref_type(implied_type) {
                        Ok(Some(value)) => Some(value),
                        Ok(None) => None,
                        Err(_) => {
                            let root = residual.copy_leaf(&self.nodes[id]);
                            return EvalResult::undefined(Vec::new(), root);
                        }
                    };
                    let property_value = converted.as_ref().unwrap_or(property_value);
                    let matched = match operator {
                        ComparisonOperator::Equal => property_value.equals(value),
                        ComparisonOperator::NotEqual => !property_value.equals(value),
                        ComparisonOperator::Greater => property_value.greater(value),
                        ComparisonOperator::GreaterEqual => property_value.greater_equal(value),
                        ComparisonOperator::Less => property_value.less(value),
                        ComparisonOperator::LessEqual => property_value.less_equal(value),
                    };
                    if matched {
                        EvalResult::true_value()
                    } else {
                        EvalResult::false_value(residual.push_empty(false))
                    }
                }
                PropertyRef::Aspect(_, aspect, _) => match aspects.get(&aspect[..]) {
                    Some(aspect_value) => {
                        let equal = value == *aspect_value;
                        let matched = match operator {
                            ComparisonOperator::Equal => equal,
                            ComparisonOperator::NotEqual => !equal,
                            _ => false,
                        };
                        if matched {
                            EvalResult::true_value()
                        } else {
                            EvalResult::false_value(residual.push_empty(false))
                        }
                    }
                    None => {
                        let root = residual.copy_leaf(&self.nodes[id]);
                        EvalResult::undefined(vec![property_ref], root)
                    }
                },
            },
        }
    }

    fn resolve_present<'a>(
        &'a self,
        id: NodeId,
        property_ref: &'a PropertyRef,
        property_set: &'a PropertySet,
        residual: &mut ExpressionBuilder,
    ) -> EvalResult<'a> {
        match property_ref {
            PropertyRef::Value(name, _) => {
                if property_set.properties.contains_key(&name[..]) {
                    EvalResult::true_value()
                } else {
                    EvalResult::false_with_refs(vec![property_ref], residual.push_empty(false))
                }
            }
            PropertyRef::Aspect(name, aspect, _) => match property_set.properties.get(&name[..]) {
                Some(Property::Explicit(_, _, aspects)) => {
                    if aspects.contains_key(&aspect[..]) {
                        EvalResult::true_value()
                    } else {
                        EvalResult::false_with_refs(vec![property_ref], residual.push_empty(false))
                    }
                }
                Some(Property::Implicit(_)) | None => {
                    let root = residual.copy_leaf(&self.nodes[id]);
                    EvalResult::false_with_refs(vec![property_ref], root)
                }
            },
        }
    }
}

#[derive(Clone, Copy)]
enum GroupOperator {
    And,
    Or,
}

struct GroupFrame<'a> {
    operator: GroupOperator,
    range: Range<usize>,
    next: usize,
    undefined: bool,
    refs: Vec<&'a PropertyRef>,
    residuals: Vec<NodeId>,
}

impl<'a> GroupFrame<'a> {
    fn new(operator: GroupOperator, range: Range<usize>) -> Self {
        Self {
            operator,
            next: range.start + 1,
            range,
            undefined: false,
            refs: Vec::new(),
            residuals: Vec::new(),
        }
    }

    fn consume(
        &mut self,
        mut child: EvalResult<'a>,
        residual: &mut ExpressionBuilder,
    ) -> Option<EvalResult<'a>> {
        match self.operator {
            GroupOperator::And => match child.kind {
                EvalKind::True => None,
                EvalKind::False => Some(EvalResult::false_value(residual.push_empty(false))),
                EvalKind::Undefined => {
                    self.undefined = true;
                    self.refs.append(&mut child.refs);
                    self.residuals
                        .push(child.residual.expect("undefined residual"));
                    None
                }
            },
            GroupOperator::Or => match child.kind {
                EvalKind::True => Some(EvalResult::true_value()),
                EvalKind::False => {
                    self.refs.append(&mut child.refs);
                    let child = child.residual.expect("false residual");
                    if !residual.is_empty(child) {
                        self.residuals.push(child);
                    }
                    None
                }
                EvalKind::Undefined => {
                    self.undefined = true;
                    self.refs.append(&mut child.refs);
                    let child = child.residual.expect("undefined residual");
                    if !residual.is_empty(child) {
                        self.residuals.push(child);
                    }
                    None
                }
            },
        }
    }

    fn finish(self, residual: &mut ExpressionBuilder) -> EvalResult<'a> {
        match self.operator {
            GroupOperator::And if self.undefined => {
                let root = combine_residuals(residual, self.residuals, true, true);
                EvalResult::undefined(self.refs, root)
            }
            GroupOperator::And => EvalResult::true_value(),
            GroupOperator::Or if self.undefined => {
                let root = combine_residuals(residual, self.residuals, false, true);
                EvalResult::undefined(self.refs, root)
            }
            GroupOperator::Or => {
                let root = combine_residuals(residual, self.residuals, false, false);
                EvalResult::false_with_refs(self.refs, root)
            }
        }
    }
}

fn combine_residuals(
    builder: &mut ExpressionBuilder,
    residuals: Vec<NodeId>,
    is_and: bool,
    empty_value: bool,
) -> NodeId {
    match residuals.as_slice() {
        [] => builder.push_empty(empty_value),
        [single] => *single,
        _ if is_and => builder.push_and(&residuals),
        _ => builder.push_or(&residuals),
    }
}

enum EvalFrame<'a> {
    Eval(NodeId),
    Not,
    Group(GroupFrame<'a>),
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum EvalKind {
    True,
    False,
    Undefined,
}

struct EvalResult<'a> {
    kind: EvalKind,
    refs: Vec<&'a PropertyRef>,
    residual: Option<NodeId>,
}

impl<'a> EvalResult<'a> {
    fn true_value() -> Self {
        Self {
            kind: EvalKind::True,
            refs: Vec::new(),
            residual: None,
        }
    }

    fn false_value(residual: NodeId) -> Self {
        Self::false_with_refs(Vec::new(), residual)
    }

    fn false_with_refs(refs: Vec<&'a PropertyRef>, residual: NodeId) -> Self {
        Self {
            kind: EvalKind::False,
            refs,
            residual: Some(residual),
        }
    }

    fn undefined(refs: Vec<&'a PropertyRef>, residual: NodeId) -> Self {
        Self {
            kind: EvalKind::Undefined,
            refs,
            residual: Some(residual),
        }
    }
}

#[derive(Debug, Clone, PartialEq)]
pub enum ResolveResult<'a> {
    True,
    False(Vec<&'a PropertyRef>, Expression),
    Undefined(Vec<&'a PropertyRef>, Expression),
    Err(ResolveError),
}

/// Compatibility shim for callers that previously converted an ASN.1-shaped
/// parse tree into an expression. The new parser already returns the final IR.
pub fn build_expression(expression: &Expression) -> Result<Expression, ExpressionError> {
    Ok(expression.clone())
}
