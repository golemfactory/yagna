const MAX_LITERAL_BYTES: usize = 64 * 1024;
const MAX_LITERAL_NODES: usize = 4 * 1024;
const MAX_LITERAL_DEPTH: usize = 128;

#[derive(Debug, Clone, PartialEq)]
pub enum Literal<'a> {
    Str(&'a str),
    Number(&'a str),
    Decimal(&'a str),
    Bool(bool),
    Version(&'a str),
    DateTime(&'a str),
    List(Vec<Literal<'a>>),
}

pub fn parse_prop_def(input: &str) -> Result<(&str, Option<&str>), String> {
    match input.split_once('=') {
        Some((name, value)) => Ok((name, Some(value))),
        None => Ok((input, None)),
    }
}

/// Parse `[value1, value2, ...]` as used by list comparison constraints.
pub fn parse_prop_ref_as_list(input: &str) -> Result<Vec<&str>, String> {
    let Some(inner) = input
        .strip_prefix('[')
        .and_then(|value| value.strip_suffix(']'))
    else {
        return Err("expected a bracketed list".to_owned());
    };
    if inner.is_empty() {
        return Ok(Vec::new());
    }

    let mut result = Vec::new();
    for item in inner.split(',') {
        let item = item.trim();
        if item.is_empty() || item.contains(['[', ']']) {
            return Err("invalid list item".to_owned());
        }
        result.push(item);
    }
    Ok(result)
}

/// Parse a property reference of the form `name[aspect]$type`.
///
/// The aspect and the `$d`, `$v`, `$t` implied type are optional. All input is
/// consumed and malformed delimiters are reported as errors; this function
/// never falls back to a panic.
pub fn parse_prop_ref_with_aspect(
    input: &str,
) -> Result<(&str, Option<&str>, Option<&str>), String> {
    if input.is_empty() {
        return Err("empty property reference".to_owned());
    }

    let (reference, implied_type) = if input.len() >= 2 && input.as_bytes()[input.len() - 2] == b'$'
    {
        let code = &input[input.len() - 1..];
        if !matches!(code, "d" | "v" | "t") {
            return Err("unknown implied property type".to_owned());
        }
        (&input[..input.len() - 2], Some(code))
    } else {
        (input, None)
    };

    if reference.contains('$') {
        return Err("invalid implied property type suffix".to_owned());
    }

    if let Some(reference) = reference.strip_suffix(']') {
        let Some(open) = reference.rfind('[') else {
            return Err("unmatched aspect delimiter".to_owned());
        };
        let name = &reference[..open];
        let aspect = &reference[open + 1..];
        if name.is_empty()
            || aspect.is_empty()
            || name.contains(['[', ']'])
            || aspect.contains(['[', ']'])
        {
            return Err("invalid property aspect".to_owned());
        }
        Ok((name, Some(aspect), implied_type))
    } else if reference.contains(['[', ']']) {
        Err("unmatched aspect delimiter".to_owned())
    } else if reference.is_empty() {
        Err("empty property reference".to_owned())
    } else {
        Ok((reference, None, implied_type))
    }
}

pub fn parse_prop_value_literal(input: &str) -> Result<Literal<'_>, String> {
    if input.len() > MAX_LITERAL_BYTES {
        return Err("property literal exceeds the input limit".to_owned());
    }

    LiteralParser {
        input,
        bytes: input.as_bytes(),
        position: 0,
        node_count: 0,
        lists: Vec::new(),
        completed: None,
    }
    .parse()
}

struct ListFrame<'a> {
    items: Vec<Literal<'a>>,
}

struct LiteralParser<'a> {
    input: &'a str,
    bytes: &'a [u8],
    position: usize,
    node_count: usize,
    lists: Vec<ListFrame<'a>>,
    completed: Option<Literal<'a>>,
}

impl<'a> LiteralParser<'a> {
    fn parse(mut self) -> Result<Literal<'a>, String> {
        loop {
            if let Some(literal) = self.completed.take() {
                if let Some(list) = self.lists.last_mut() {
                    list.items.push(literal);
                    self.skip_whitespace();
                    match self.peek() {
                        Some(b',') => {
                            self.position += 1;
                            self.skip_whitespace();
                            if matches!(self.peek(), None | Some(b']')) {
                                return Err(self.error("expected a list item"));
                            }
                        }
                        Some(b']') => {
                            self.position += 1;
                            let Some(list) = self.lists.pop() else {
                                return Err(self.error("unexpected list terminator"));
                            };
                            self.add_node()?;
                            self.completed = Some(Literal::List(list.items));
                        }
                        _ => return Err(self.error("expected ',' or ']'")),
                    }
                } else {
                    self.skip_whitespace();
                    if self.position == self.bytes.len() {
                        return Ok(literal);
                    }
                    return Err(self.error("unexpected trailing literal input"));
                }
                continue;
            }

            self.skip_whitespace();
            if self.peek() == Some(b'[') {
                if self.lists.len() + 1 > MAX_LITERAL_DEPTH {
                    return Err(self.error("property literal exceeds the depth limit"));
                }
                self.position += 1;
                self.skip_whitespace();
                if self.peek() == Some(b']') {
                    self.position += 1;
                    self.add_node()?;
                    self.completed = Some(Literal::List(Vec::new()));
                } else {
                    self.lists.push(ListFrame { items: Vec::new() });
                }
            } else {
                let literal = self.parse_atom()?;
                self.add_node()?;
                self.completed = Some(literal);
            }
        }
    }

    fn parse_atom(&mut self) -> Result<Literal<'a>, String> {
        let start = self.position;
        match self.peek() {
            Some(b'"') => self.parse_quoted(self.position).map(Literal::Str),
            Some(prefix @ (b'd' | b't' | b'v'))
                if self.bytes.get(self.position + 1) == Some(&b'"') =>
            {
                self.position += 1;
                let value = self.parse_quoted(self.position)?;
                Ok(match prefix {
                    b'd' => Literal::Decimal(value),
                    b't' => Literal::DateTime(value),
                    b'v' => Literal::Version(value),
                    _ => return Err(self.error("unknown typed property literal")),
                })
            }
            Some(_) => {
                while self
                    .peek()
                    .is_some_and(|byte| !byte.is_ascii_whitespace() && !matches!(byte, b',' | b']'))
                {
                    self.position += 1;
                }
                let token = &self.input[start..self.position];
                match token {
                    "true" | "True" | "TRUE" => Ok(Literal::Bool(true)),
                    "false" | "False" | "FALSE" => Ok(Literal::Bool(false)),
                    _ if is_number(token.as_bytes()) => Ok(Literal::Number(token)),
                    _ => Err(self.error("unknown property literal type")),
                }
            }
            None => Err(self.error("expected a property literal")),
        }
    }

    fn parse_quoted(&mut self, quote: usize) -> Result<&'a str, String> {
        self.position = quote + 1;
        let start = self.position;
        while let Some(byte) = self.peek() {
            match byte {
                b'"' => {
                    let value = &self.input[start..self.position];
                    self.position += 1;
                    return Ok(value);
                }
                b'\\' => match self.bytes.get(self.position + 1).copied() {
                    Some(b'"' | b'/' | b'b' | b'f' | b'n' | b'r' | b't' | b'0' | b'\\') => {
                        self.position += 2;
                    }
                    Some(b'u') => {
                        let digits = self.bytes.get(self.position + 2..self.position + 6);
                        if !digits.is_some_and(|digits| digits.iter().all(u8::is_ascii_hexdigit)) {
                            return Err(self.error("invalid unicode escape"));
                        }
                        self.position += 6;
                    }
                    _ => return Err(self.error("invalid string escape")),
                },
                _ => self.position += 1,
            }
        }
        Err(self.error("unterminated string literal"))
    }

    fn add_node(&mut self) -> Result<(), String> {
        if self.node_count >= MAX_LITERAL_NODES {
            Err(self.error("property literal exceeds the node limit"))
        } else {
            self.node_count += 1;
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

    fn error(&self, message: &str) -> String {
        format!("{message} at byte {}", self.position)
    }
}

fn is_number(bytes: &[u8]) -> bool {
    let mut position = 0;
    if matches!(bytes.first(), Some(b'+' | b'-')) {
        position += 1;
    }
    let integer_start = position;
    while bytes.get(position).is_some_and(u8::is_ascii_digit) {
        position += 1;
    }
    if position == integer_start {
        return false;
    }

    if bytes.get(position) == Some(&b'.') {
        position += 1;
        let fraction_start = position;
        while bytes.get(position).is_some_and(u8::is_ascii_digit) {
            position += 1;
        }
        if position == fraction_start {
            return false;
        }
    }

    if matches!(bytes.get(position), Some(b'e' | b'E')) {
        position += 1;
        if matches!(bytes.get(position), Some(b'+' | b'-')) {
            position += 1;
        }
        let exponent_start = position;
        while bytes.get(position).is_some_and(u8::is_ascii_digit) {
            position += 1;
        }
        if position == exponent_start {
            return false;
        }
    }

    position == bytes.len()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn deeply_nested_literal_is_rejected_without_recursion() {
        let input = format!("{}0{}", "[".repeat(10_000), "]".repeat(10_000));
        let handle = std::thread::Builder::new()
            .stack_size(64 * 1024)
            .spawn(move || parse_prop_value_literal(&input).map(|_| ()))
            .unwrap();
        assert!(handle.join().unwrap().is_err());
    }
}
