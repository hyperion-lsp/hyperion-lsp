// Copyright 2025 Adrian Helvik
// SPDX-License-Identifier: PolyForm-Shield-1.0.0

//! The Java-like expression language of the debugger's evaluate request,
//! watch expressions and breakpoint conditions: literals, names, field
//! access, method calls, indexing, arithmetic, comparison, logic, string
//! concatenation and `?:`. Parsing is pure; evaluation lives in `evaluate`.

/// Check if a formatted value string is truthy for conditional breakpoint evaluation.
pub(super) fn is_truthy(value: &str) -> bool {
    !matches!(
        value,
        "false" | "0" | "0L" | "null" | "\"\"" | "" | "0.0" | "0.0f" | "0f"
    )
}

#[derive(Debug, Clone, PartialEq)]
pub(super) enum Literal {
    Int(i32),
    Long(i64),
    Float(f32),
    Double(f64),
    Bool(bool),
    Char(u16),
    Str(String),
    Null,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum UnaryOp {
    Neg,
    Not,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum BinaryOp {
    Add,
    Sub,
    Mul,
    Div,
    Rem,
    Eq,
    Ne,
    Lt,
    Le,
    Gt,
    Ge,
    And,
    Or,
}

#[derive(Debug, Clone, PartialEq)]
pub(super) enum Expr {
    Literal(Literal),
    Name(String),
    This,
    Field(Box<Self>, String),
    /// `receiver.name(args)`, or `name(args)` with no receiver.
    Call(Option<Box<Self>>, String, Vec<Self>),
    Index(Box<Self>, Box<Self>),
    Unary(UnaryOp, Box<Self>),
    Binary(BinaryOp, Box<Self>, Box<Self>),
    Conditional(Box<Self>, Box<Self>, Box<Self>),
}

impl Expr {
    /// True if evaluating the expression may run debuggee code.
    pub(super) fn invokes_methods(&self) -> bool {
        match self {
            Self::Call(..) => true,
            Self::Literal(_) | Self::Name(_) | Self::This => false,
            Self::Field(inner, _) | Self::Unary(_, inner) => inner.invokes_methods(),
            Self::Index(a, b) | Self::Binary(_, a, b) => a.invokes_methods() || b.invokes_methods(),
            Self::Conditional(a, b, c) => {
                a.invokes_methods() || b.invokes_methods() || c.invokes_methods()
            }
        }
    }
}

#[derive(Debug, Clone, PartialEq)]
enum Token {
    Ident(String),
    Lit(Literal),
    Punct(&'static str),
}

const PUNCTUATION: &[&str] = &[
    "&&", "||", "==", "!=", "<=", ">=", "+", "-", "*", "/", "%", "<", ">", "!", "(", ")", "[", "]",
    ".", ",", "?", ":",
];

fn tokenize(input: &str) -> Result<Vec<Token>, String> {
    let chars: Vec<char> = input.chars().collect();
    let mut tokens = Vec::new();
    let mut i = 0;
    while i < chars.len() {
        let c = chars[i];
        if c.is_whitespace() {
            i += 1;
        } else if c.is_alphabetic() || c == '_' || c == '$' {
            let start = i;
            while i < chars.len()
                && (chars[i].is_alphanumeric() || chars[i] == '_' || chars[i] == '$')
            {
                i += 1;
            }
            let word: String = chars[start..i].iter().collect();
            tokens.push(match word.as_str() {
                "true" => Token::Lit(Literal::Bool(true)),
                "false" => Token::Lit(Literal::Bool(false)),
                "null" => Token::Lit(Literal::Null),
                _ => Token::Ident(word),
            });
        } else if c.is_ascii_digit()
            || (c == '.' && chars.get(i + 1).is_some_and(char::is_ascii_digit))
        {
            let (literal, next) = lex_number(&chars, i)?;
            tokens.push(Token::Lit(literal));
            i = next;
        } else if c == '"' {
            let (text, next) = lex_quoted(&chars, i, '"')?;
            tokens.push(Token::Lit(Literal::Str(text)));
            i = next;
        } else if c == '\'' {
            let (text, next) = lex_quoted(&chars, i, '\'')?;
            let mut units = text.encode_utf16();
            match (units.next(), units.next()) {
                (Some(unit), None) => tokens.push(Token::Lit(Literal::Char(unit))),
                _ => return Err("character literal must hold exactly one character".to_owned()),
            }
            i = next;
        } else {
            let rest: String = chars[i..chars.len().min(i + 2)].iter().collect();
            let Some(punct) = PUNCTUATION.iter().find(|p| rest.starts_with(**p)) else {
                return Err(format!("unexpected character '{c}'"));
            };
            tokens.push(Token::Punct(punct));
            i += punct.len();
        }
    }
    Ok(tokens)
}

fn lex_number(chars: &[char], start: usize) -> Result<(Literal, usize), String> {
    let mut i = start;
    let hex = chars[i] == '0' && matches!(chars.get(i + 1), Some('x' | 'X'));
    if hex {
        i += 2;
    }
    while i < chars.len() {
        let c = chars[i];
        let previous_is_exponent = i > start && matches!(chars[i - 1], 'e' | 'E');
        let part = if hex {
            c.is_ascii_hexdigit() || c == '_'
        } else {
            c.is_ascii_digit()
                || c == '_'
                || c == '.'
                || c == 'e'
                || c == 'E'
                || ((c == '+' || c == '-') && previous_is_exponent)
        };
        // A dot must be followed by a digit to belong to the number.
        if c == '.' && !chars.get(i + 1).is_some_and(char::is_ascii_digit) {
            break;
        }
        // `e`/`E` belong to the number only as an exponent marker.
        if !hex
            && (c == 'e' || c == 'E')
            && !chars
                .get(i + 1)
                .is_some_and(|next| next.is_ascii_digit() || *next == '+' || *next == '-')
        {
            break;
        }
        if !part {
            break;
        }
        i += 1;
    }
    let mut body: String = chars[start..i].iter().filter(|c| **c != '_').collect();
    let suffix = chars.get(i).copied();
    let literal = match suffix {
        Some('l' | 'L') => {
            i += 1;
            let value = if hex {
                i64::from_str_radix(&body[2..], 16).map_err(|e| e.to_string())?
            } else {
                body.parse::<i64>()
                    .map_err(|_| format!("bad long literal '{body}'"))?
            };
            Literal::Long(value)
        }
        Some('f' | 'F') if !hex => {
            i += 1;
            Literal::Float(
                body.parse::<f32>()
                    .map_err(|_| format!("bad float literal '{body}'"))?,
            )
        }
        Some('d' | 'D') if !hex => {
            i += 1;
            Literal::Double(
                body.parse::<f64>()
                    .map_err(|_| format!("bad double literal '{body}'"))?,
            )
        }
        _ if hex => {
            let value = i64::from_str_radix(&body[2..], 16).map_err(|e| e.to_string())?;
            Literal::Int(value as i32)
        }
        _ if body.contains(['.', 'e', 'E']) => {
            if body.starts_with('.') {
                body.insert(0, '0');
            }
            Literal::Double(
                body.parse::<f64>()
                    .map_err(|_| format!("bad number '{body}'"))?,
            )
        }
        _ => match body.parse::<i32>() {
            Ok(value) => Literal::Int(value),
            Err(_) => Literal::Long(
                body.parse::<i64>()
                    .map_err(|_| format!("bad number '{body}'"))?,
            ),
        },
    };
    Ok((literal, i))
}

fn lex_quoted(chars: &[char], start: usize, quote: char) -> Result<(String, usize), String> {
    let mut i = start + 1;
    let mut out = String::new();
    while i < chars.len() {
        let c = chars[i];
        if c == quote {
            return Ok((out, i + 1));
        }
        if c == '\\' {
            i += 1;
            let escaped = *chars.get(i).ok_or("unterminated escape sequence")?;
            match escaped {
                'n' => out.push('\n'),
                't' => out.push('\t'),
                'r' => out.push('\r'),
                '0' => out.push('\0'),
                'b' => out.push('\u{8}'),
                'f' => out.push('\u{c}'),
                'u' => {
                    let hex: String = chars
                        .get(i + 1..i + 5)
                        .ok_or("bad \\u escape")?
                        .iter()
                        .collect();
                    let code = u32::from_str_radix(&hex, 16).map_err(|_| "bad \\u escape")?;
                    out.push(char::from_u32(code).unwrap_or('\u{fffd}'));
                    i += 4;
                }
                other => out.push(other),
            }
        } else {
            out.push(c);
        }
        i += 1;
    }
    Err("unterminated string literal".to_owned())
}

pub(super) fn parse(input: &str) -> Result<Expr, String> {
    let tokens = tokenize(input)?;
    if tokens.is_empty() {
        return Err("empty expression".to_owned());
    }
    let mut parser = Parser { tokens, pos: 0 };
    let expr = parser.conditional()?;
    if let Some(token) = parser.tokens.get(parser.pos) {
        return Err(format!("unexpected {}", describe(token)));
    }
    Ok(expr)
}

fn describe(token: &Token) -> String {
    match token {
        Token::Ident(name) => format!("'{name}'"),
        Token::Lit(_) => "literal".to_owned(),
        Token::Punct(p) => format!("'{p}'"),
    }
}

struct Parser {
    tokens: Vec<Token>,
    pos: usize,
}

impl Parser {
    fn peek_punct(&self) -> Option<&'static str> {
        match self.tokens.get(self.pos) {
            Some(Token::Punct(p)) => Some(p),
            _ => None,
        }
    }

    fn eat(&mut self, punct: &str) -> bool {
        if self.peek_punct() == Some(punct) {
            self.pos += 1;
            true
        } else {
            false
        }
    }

    fn expect(&mut self, punct: &str) -> Result<(), String> {
        if self.eat(punct) {
            Ok(())
        } else {
            Err(match self.tokens.get(self.pos) {
                Some(token) => format!("expected '{punct}' but found {}", describe(token)),
                None => format!("expected '{punct}' at the end of the expression"),
            })
        }
    }

    fn conditional(&mut self) -> Result<Expr, String> {
        let condition = self.binary(0)?;
        if self.eat("?") {
            let then = self.conditional()?;
            self.expect(":")?;
            let otherwise = self.conditional()?;
            return Ok(Expr::Conditional(
                Box::new(condition),
                Box::new(then),
                Box::new(otherwise),
            ));
        }
        Ok(condition)
    }

    /// Precedence climbing over the binary operators (lowest first).
    fn binary(&mut self, level: usize) -> Result<Expr, String> {
        const LEVELS: &[&[(&str, BinaryOp)]] = &[
            &[("||", BinaryOp::Or)],
            &[("&&", BinaryOp::And)],
            &[("==", BinaryOp::Eq), ("!=", BinaryOp::Ne)],
            &[
                ("<=", BinaryOp::Le),
                (">=", BinaryOp::Ge),
                ("<", BinaryOp::Lt),
                (">", BinaryOp::Gt),
            ],
            &[("+", BinaryOp::Add), ("-", BinaryOp::Sub)],
            &[
                ("*", BinaryOp::Mul),
                ("/", BinaryOp::Div),
                ("%", BinaryOp::Rem),
            ],
        ];
        if level == LEVELS.len() {
            return self.unary();
        }
        let mut left = self.binary(level + 1)?;
        'outer: loop {
            for (text, op) in LEVELS[level] {
                if self.eat(text) {
                    let right = self.binary(level + 1)?;
                    left = Expr::Binary(*op, Box::new(left), Box::new(right));
                    continue 'outer;
                }
            }
            return Ok(left);
        }
    }

    fn unary(&mut self) -> Result<Expr, String> {
        if self.eat("!") {
            return Ok(Expr::Unary(UnaryOp::Not, Box::new(self.unary()?)));
        }
        if self.eat("-") {
            return Ok(Expr::Unary(UnaryOp::Neg, Box::new(self.unary()?)));
        }
        self.postfix()
    }

    fn postfix(&mut self) -> Result<Expr, String> {
        let mut expr = self.primary()?;
        loop {
            if self.eat(".") {
                let Some(Token::Ident(name)) = self.tokens.get(self.pos).cloned() else {
                    return Err("expected a member name after '.'".to_owned());
                };
                self.pos += 1;
                expr = if self.eat("(") {
                    Expr::Call(Some(Box::new(expr)), name, self.arguments()?)
                } else {
                    Expr::Field(Box::new(expr), name)
                };
            } else if self.eat("[") {
                let index = self.conditional()?;
                self.expect("]")?;
                expr = Expr::Index(Box::new(expr), Box::new(index));
            } else {
                return Ok(expr);
            }
        }
    }

    fn arguments(&mut self) -> Result<Vec<Expr>, String> {
        let mut args = Vec::new();
        if self.eat(")") {
            return Ok(args);
        }
        loop {
            args.push(self.conditional()?);
            if self.eat(")") {
                return Ok(args);
            }
            self.expect(",")?;
        }
    }

    fn primary(&mut self) -> Result<Expr, String> {
        let Some(token) = self.tokens.get(self.pos).cloned() else {
            return Err("unexpected end of expression".to_owned());
        };
        self.pos += 1;
        match token {
            Token::Lit(literal) => Ok(Expr::Literal(literal)),
            Token::Ident(name) if name == "this" => Ok(Expr::This),
            Token::Ident(name) => {
                if self.eat("(") {
                    Ok(Expr::Call(None, name, self.arguments()?))
                } else {
                    Ok(Expr::Name(name))
                }
            }
            Token::Punct("(") => {
                let inner = self.conditional()?;
                self.expect(")")?;
                Ok(inner)
            }
            other => Err(format!("unexpected {}", describe(&other))),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[allow(clippy::unnecessary_box_returns)]
    fn name(n: &str) -> Box<Expr> {
        Box::new(Expr::Name(n.to_owned()))
    }

    #[test]
    fn parses_calls_fields_and_indexing() {
        assert_eq!(
            parse("users.size()").unwrap(),
            Expr::Call(Some(name("users")), "size".to_owned(), vec![])
        );
        assert_eq!(
            parse("list.get(0).name").unwrap(),
            Expr::Field(
                Box::new(Expr::Call(
                    Some(name("list")),
                    "get".to_owned(),
                    vec![Expr::Literal(Literal::Int(0))]
                )),
                "name".to_owned()
            )
        );
        assert_eq!(
            parse("a[i + 1]").unwrap(),
            Expr::Index(
                name("a"),
                Box::new(Expr::Binary(
                    BinaryOp::Add,
                    name("i"),
                    Box::new(Expr::Literal(Literal::Int(1)))
                ))
            )
        );
        assert_eq!(
            parse("java.util.Collections.emptyList()").unwrap(),
            Expr::Call(
                Some(Box::new(Expr::Field(
                    Box::new(Expr::Field(name("java"), "util".to_owned())),
                    "Collections".to_owned()
                ))),
                "emptyList".to_owned(),
                vec![]
            )
        );
    }

    #[test]
    fn respects_operator_precedence_and_associativity() {
        // 1 + 2 * 3 == 7 && !done
        let expr = parse("1 + 2 * 3 == 7 && !done").unwrap();
        let Expr::Binary(BinaryOp::And, left, right) = expr else {
            panic!("expected && at the top");
        };
        assert!(matches!(*right, Expr::Unary(UnaryOp::Not, _)));
        let Expr::Binary(BinaryOp::Eq, sum, _) = *left else {
            panic!("expected == below &&");
        };
        assert!(matches!(*sum, Expr::Binary(BinaryOp::Add, _, _)));
        // Left associative: 10 - 3 - 2 == (10 - 3) - 2
        let Expr::Binary(BinaryOp::Sub, lhs, _) = parse("10 - 3 - 2").unwrap() else {
            panic!("expected subtraction");
        };
        assert!(matches!(*lhs, Expr::Binary(BinaryOp::Sub, _, _)));
    }

    #[test]
    fn lexes_java_literals() {
        let lit = |s: &str| match parse(s).unwrap() {
            Expr::Literal(l) => l,
            other => panic!("not a literal: {other:?}"),
        };
        assert_eq!(lit("42"), Literal::Int(42));
        assert_eq!(lit("1_000L"), Literal::Long(1000));
        assert_eq!(lit("0xFF"), Literal::Int(255));
        assert_eq!(lit("2.5"), Literal::Double(2.5));
        assert_eq!(lit("2.5f"), Literal::Float(2.5));
        assert_eq!(lit("1e3"), Literal::Double(1000.0));
        assert_eq!(lit("'x'"), Literal::Char('x' as u16));
        assert_eq!(lit("\"a\\n\\\"b\""), Literal::Str("a\n\"b".to_owned()));
        assert_eq!(lit("null"), Literal::Null);
        assert_eq!(lit("true"), Literal::Bool(true));
        assert_eq!(lit("3000000000"), Literal::Long(3_000_000_000));
    }

    #[test]
    fn reports_syntax_errors_instead_of_guessing() {
        assert!(parse("").is_err());
        assert!(parse("1 +").is_err());
        assert!(parse("foo(").is_err());
        assert!(parse("a b").is_err());
        assert!(parse("\"open").is_err());
        assert!(parse("x = 3").is_err());
    }

    #[test]
    fn method_calls_are_detected_for_hover_safety() {
        assert!(parse("a.b().c").unwrap().invokes_methods());
        assert!(!parse("a.b.c + 1").unwrap().invokes_methods());
    }

    #[test]
    fn truthiness_matches_conditional_breakpoint_results() {
        assert!(is_truthy("true"));
        assert!(!is_truthy("false"));
        assert!(!is_truthy("0"));
        assert!(is_truthy("7"));
        assert!(!is_truthy("null"));
    }
}
