//! The parser: tokens to a syntax tree, with each name resolved against the enclosing `repeat`s
//! and nesting counted the way the spec counts it.

use alloc::borrow::ToOwned;
use alloc::boxed::Box;
use alloc::vec::Vec;

use super::lex::{self, Kind, Token};
use super::{BinaryOp, Expr, Expression, Function, LogicOp, Name, Step, UnaryOp};
use crate::budget::{self, Budget};
use crate::json::Value;

/// Why an expression cannot be loaded.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[non_exhaustive]
pub enum ParseError {
    /// The source breaks the grammar, calls an unknown function or passes it the wrong number of
    /// arguments, or reads a name that nothing binds.
    Invalid {
        /// The byte offset in the source where the problem starts.
        offset: usize,
        /// What is wrong there.
        reason: &'static str,
    },
    /// The source is longer, or nests deeper, than the budget allows.
    Budget(Budget),
}

/// The names the enclosing `repeat`s bind, innermost first. The default binds nothing.
#[derive(Debug, Clone, Copy, Default)]
pub struct Names<'n> {
    innermost: Option<&'n NameFrame<'n>>,
}

/// The two names one `repeat` binds.
#[derive(Debug)]
pub struct NameFrame<'n> {
    item: &'n str,
    index: &'n str,
    outer: Names<'n>,
}

impl<'n> NameFrame<'n> {
    /// A `repeat` inside `outer` that binds its item as `item` and its index as `index`.
    pub fn new(item: &'n str, index: &'n str, outer: Names<'n>) -> NameFrame<'n> {
        NameFrame { item, index, outer }
    }

    /// The names in scope inside this `repeat`.
    pub fn names(&'n self) -> Names<'n> {
        Names {
            innermost: Some(self),
        }
    }
}

impl Names<'_> {
    /// What `word` refers to, when a `repeat` binds it; inner names shadow outer ones.
    fn find(self, word: &str) -> Option<Name> {
        let mut frame = self.innermost;
        let mut depth = 0_usize;
        while let Some(current) = frame {
            if current.item == word {
                return Some(Name::Item(depth));
            }
            if current.index == word {
                return Some(Name::Index(depth));
            }
            frame = current.outer.innermost;
            depth = depth.saturating_add(1);
        }
        None
    }
}

/// Parses `source`, resolving its names against `names`.
///
/// # Errors
/// [`ParseError::Budget`] for a source over the expression length or nesting budget, and
/// [`ParseError::Invalid`] for one that breaks the grammar, calls an unknown function or passes
/// it the wrong number of arguments, or reads a name that is neither `state`, `env` nor bound.
pub fn parse(source: &str, names: Names<'_>) -> Result<Expression, ParseError> {
    if source.len() > budget::EXPRESSION_LENGTH.limit {
        return Err(ParseError::Budget(budget::EXPRESSION_LENGTH));
    }
    let tokens = lex::tokenize(source)?;
    let mut parser = Parser {
        tokens: &tokens,
        pos: 0,
        names,
        frames: 0,
    };
    let (root, _) = parser.expression(0)?;
    let end = parser.peek();
    if end.kind != Kind::End {
        return Err(invalid(end.offset, "unexpected token"));
    }
    Ok(Expression {
        source: source.to_owned(),
        root,
        frames: parser.frames,
    })
}

fn invalid(offset: usize, reason: &'static str) -> ParseError {
    ParseError::Invalid { offset, reason }
}

/// A parsed expression and its nesting level: 0 for a literal or a name, and one more than its
/// deepest operand for anything that holds operands.
type Parsed = (Expr, usize);

/// The level of a construct whose operands reach `inner`, or the depth one construct further
/// down; either way, one level past the budget fails.
fn around(inner: usize) -> Result<usize, ParseError> {
    let level = inner.saturating_add(1);
    if level > budget::EXPRESSION_NESTING.limit {
        Err(ParseError::Budget(budget::EXPRESSION_NESTING))
    } else {
        Ok(level)
    }
}

/// Stands in for a token past the end, which the parser never reaches since it stops at `End`.
static END: Token = Token {
    kind: Kind::End,
    offset: 0,
};

/// The precedences of the binary operators that chain, loosest first.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Precedence {
    Equality,
    Comparison,
    Additive,
    Multiplicative,
}

impl BinaryOp {
    const fn precedence(self) -> Precedence {
        match self {
            BinaryOp::Equal | BinaryOp::NotEqual => Precedence::Equality,
            BinaryOp::Less | BinaryOp::LessEqual | BinaryOp::Greater | BinaryOp::GreaterEqual => {
                Precedence::Comparison
            }
            BinaryOp::Add | BinaryOp::Subtract => Precedence::Additive,
            BinaryOp::Multiply | BinaryOp::Divide | BinaryOp::Remainder => {
                Precedence::Multiplicative
            }
        }
    }
}

static BINARY_TOKENS: [(Kind, BinaryOp); 11] = [
    (Kind::EqualEqual, BinaryOp::Equal),
    (Kind::BangEqual, BinaryOp::NotEqual),
    (Kind::Less, BinaryOp::Less),
    (Kind::LessEqual, BinaryOp::LessEqual),
    (Kind::Greater, BinaryOp::Greater),
    (Kind::GreaterEqual, BinaryOp::GreaterEqual),
    (Kind::Plus, BinaryOp::Add),
    (Kind::Minus, BinaryOp::Subtract),
    (Kind::Star, BinaryOp::Multiply),
    (Kind::Slash, BinaryOp::Divide),
    (Kind::Percent, BinaryOp::Remainder),
];

/// A recursive-descent parser, one method per precedence. The depth passed down counts the
/// levels known to enclose the current token, so runaway nesting fails before it can exhaust
/// the stack; the level each method returns is exact.
struct Parser<'t, 'n> {
    tokens: &'t [Token],
    pos: usize,
    names: Names<'n>,
    frames: usize,
}

impl<'t> Parser<'t, '_> {
    fn peek(&self) -> &'t Token {
        self.tokens.get(self.pos).unwrap_or(&END)
    }

    fn next_token(&mut self) -> &'t Token {
        let token = self.peek();
        if token.kind != Kind::End {
            self.pos = self.pos.saturating_add(1);
        }
        token
    }

    fn eat(&mut self, kind: &Kind) -> bool {
        let found = self.peek().kind == *kind;
        if found {
            self.next_token();
        }
        found
    }

    fn expect(&mut self, kind: &Kind, reason: &'static str) -> Result<(), ParseError> {
        let offset = self.peek().offset;
        if self.eat(kind) {
            Ok(())
        } else {
            Err(invalid(offset, reason))
        }
    }

    /// `condition ? then : otherwise`, or an `||` run on its own.
    fn expression(&mut self, depth: usize) -> Result<Parsed, ParseError> {
        let (condition, condition_level) = self.logic(LogicOp::Or, depth)?;
        if !self.eat(&Kind::Question) {
            return Ok((condition, condition_level));
        }
        let inner = around(depth)?;
        let (then, then_level) = self.expression(inner)?;
        self.expect(&Kind::Colon, "expected :")?;
        let (otherwise, otherwise_level) = self.expression(inner)?;
        let level = around(condition_level.max(then_level).max(otherwise_level))?;
        let conditional = Expr::Conditional {
            condition: Box::new(condition),
            then: Box::new(then),
            otherwise: Box::new(otherwise),
        };
        Ok((conditional, level))
    }

    /// A run of `||`, or of `&&`, over operands that bind tighter.
    fn logic(&mut self, op: LogicOp, depth: usize) -> Result<Parsed, ParseError> {
        let token = match op {
            LogicOp::Or => Kind::OrOr,
            LogicOp::And => Kind::AndAnd,
        };
        let (first, first_level) = self.logic_operand(op, depth)?;
        if self.peek().kind != token {
            return Ok((first, first_level));
        }
        let inner = around(depth)?;
        let mut operands = Vec::from([first]);
        let mut level = first_level;
        while self.eat(&token) {
            let (operand, operand_level) = self.logic_operand(op, inner)?;
            level = level.max(operand_level);
            operands.push(operand);
        }
        Ok((Expr::Logic { op, operands }, around(level)?))
    }

    fn logic_operand(&mut self, op: LogicOp, depth: usize) -> Result<Parsed, ParseError> {
        match op {
            LogicOp::Or => self.logic(LogicOp::And, depth),
            LogicOp::And => self.chain(Precedence::Equality, depth),
        }
    }

    /// A run of binary operators of one precedence, over operands that bind tighter.
    fn chain(&mut self, precedence: Precedence, depth: usize) -> Result<Parsed, ParseError> {
        let (first, first_level) = self.chain_operand(precedence, depth)?;
        let Some(mut op) = self.binary_op(precedence) else {
            return Ok((first, first_level));
        };
        let inner = around(depth)?;
        let mut rest = Vec::new();
        let mut level = first_level;
        loop {
            self.next_token();
            let (operand, operand_level) = self.chain_operand(precedence, inner)?;
            level = level.max(operand_level);
            rest.push((op, operand));
            match self.binary_op(precedence) {
                Some(next) => op = next,
                None => break,
            }
        }
        let chain = Expr::Chain {
            first: Box::new(first),
            rest,
        };
        Ok((chain, around(level)?))
    }

    fn chain_operand(
        &mut self,
        precedence: Precedence,
        depth: usize,
    ) -> Result<Parsed, ParseError> {
        match precedence {
            Precedence::Equality => self.chain(Precedence::Comparison, depth),
            Precedence::Comparison => self.chain(Precedence::Additive, depth),
            Precedence::Additive => self.chain(Precedence::Multiplicative, depth),
            Precedence::Multiplicative => self.unary(depth),
        }
    }

    /// The binary operator at the current token, when it has `precedence`.
    fn binary_op(&self, precedence: Precedence) -> Option<BinaryOp> {
        let kind = &self.peek().kind;
        BINARY_TOKENS
            .iter()
            .find(|(token, _)| token == kind)
            .map(|(_, op)| *op)
            .filter(|op| op.precedence() == precedence)
    }

    fn unary(&mut self, depth: usize) -> Result<Parsed, ParseError> {
        let op = if self.eat(&Kind::Bang) {
            UnaryOp::Not
        } else if self.eat(&Kind::Minus) {
            UnaryOp::Negate
        } else {
            return self.postfix(depth);
        };
        let (operand, operand_level) = self.unary(around(depth)?)?;
        let unary = Expr::Unary {
            op,
            operand: Box::new(operand),
        };
        Ok((unary, around(operand_level)?))
    }

    /// A value followed by a run of `.name` and `[index]` steps.
    fn postfix(&mut self, depth: usize) -> Result<Parsed, ParseError> {
        let (base, base_level) = self.primary(depth)?;
        if !matches!(self.peek().kind, Kind::Dot | Kind::LeftBracket) {
            return Ok((base, base_level));
        }
        let inner = around(depth)?;
        let mut steps = Vec::new();
        let mut level = base_level;
        loop {
            if self.eat(&Kind::Dot) {
                let token = self.next_token();
                let Kind::Word(name) = &token.kind else {
                    return Err(invalid(token.offset, "expected a name after ."));
                };
                steps.push(Step::Field(name.clone()));
            } else if self.eat(&Kind::LeftBracket) {
                let (index, index_level) = self.expression(inner)?;
                level = level.max(index_level);
                self.expect(&Kind::RightBracket, "expected ]")?;
                steps.push(Step::Index(index));
            } else {
                break;
            }
        }
        let access = Expr::Access {
            base: Box::new(base),
            steps,
        };
        Ok((access, around(level)?))
    }

    fn primary(&mut self, depth: usize) -> Result<Parsed, ParseError> {
        let token = self.next_token();
        match &token.kind {
            Kind::Number(number) => Ok((Expr::Literal(Value::Number(*number)), 0)),
            Kind::String(text) => Ok((Expr::Literal(Value::String(text.clone())), 0)),
            Kind::Word(word) => self.word(word, token.offset, depth),
            Kind::LeftParen => {
                let (inner, inner_level) = self.expression(around(depth)?)?;
                self.expect(&Kind::RightParen, "expected )")?;
                Ok((inner, around(inner_level)?))
            }
            Kind::LeftBracket => {
                let (items, level) = self.items(&Kind::RightBracket, depth, "expected , or ]")?;
                Ok((Expr::List(items), level))
            }
            Kind::RightParen
            | Kind::RightBracket
            | Kind::Comma
            | Kind::Dot
            | Kind::Question
            | Kind::Colon
            | Kind::Bang
            | Kind::Minus
            | Kind::Plus
            | Kind::Star
            | Kind::Slash
            | Kind::Percent
            | Kind::Less
            | Kind::LessEqual
            | Kind::Greater
            | Kind::GreaterEqual
            | Kind::EqualEqual
            | Kind::BangEqual
            | Kind::AndAnd
            | Kind::OrOr
            | Kind::End => Err(invalid(token.offset, "expected a value")),
        }
    }

    /// A keyword, a name or, before `(`, a function call.
    fn word(&mut self, word: &str, offset: usize, depth: usize) -> Result<Parsed, ParseError> {
        if self.peek().kind == Kind::LeftParen {
            return self.call(word, offset, depth);
        }
        let expr = match word {
            "true" => Expr::Literal(Value::Bool(true)),
            "false" => Expr::Literal(Value::Bool(false)),
            "null" => Expr::Literal(Value::Null),
            "state" => Expr::Name(Name::State),
            "env" => Expr::Name(Name::Env),
            _ => Expr::Name(self.binding(word, offset)?),
        };
        Ok((expr, 0))
    }

    fn binding(&mut self, word: &str, offset: usize) -> Result<Name, ParseError> {
        let name = self.names.find(word).ok_or(ParseError::Invalid {
            offset,
            reason: "unknown name",
        })?;
        if let Name::Item(depth) | Name::Index(depth) = name {
            self.frames = self.frames.max(depth.saturating_add(1));
        }
        Ok(name)
    }

    fn call(&mut self, word: &str, offset: usize, depth: usize) -> Result<Parsed, ParseError> {
        let function = Function::from_name(word).ok_or(ParseError::Invalid {
            offset,
            reason: "unknown function",
        })?;
        self.next_token();
        let (args, level) = self.items(&Kind::RightParen, depth, "expected , or )")?;
        if args.len() != function.arity() {
            return Err(invalid(offset, "wrong number of arguments"));
        }
        Ok((Expr::Call { function, args }, level))
    }

    /// Comma-separated expressions up to `close`, the opening bracket already read, and the
    /// level of the brackets that hold them.
    fn items(
        &mut self,
        close: &Kind,
        depth: usize,
        reason: &'static str,
    ) -> Result<(Vec<Expr>, usize), ParseError> {
        let inner = around(depth)?;
        let mut items = Vec::new();
        let mut level = 0;
        if !self.eat(close) {
            loop {
                let (item, item_level) = self.expression(inner)?;
                level = level.max(item_level);
                items.push(item);
                if !self.eat(&Kind::Comma) {
                    self.expect(close, reason)?;
                    break;
                }
            }
        }
        Ok((items, around(level)?))
    }
}
