//! Expressions: parsed once when a template loads, then evaluated against state, the environment
//! and the enclosing `repeat` bindings each time the widget resolves.
//!
//! Bad data never fails an evaluation: a type mismatch, a missing field or a misused function
//! yields `null`, and the property takes its default. Only budgets fail, and every step spends
//! fuel from one counter that the whole resolve shares.

mod eval;
mod func;
mod lex;
mod parse;
mod time;

use alloc::boxed::Box;
use alloc::string::String;
use alloc::vec::Vec;

use crate::json::Value;

pub use eval::{Bindings, Env, Evaluator, Frame};
pub use parse::{parse, NameFrame, Names, ParseError};

/// A parsed expression, with every name it reads resolved against the `repeat`s around it.
#[derive(Debug, PartialEq)]
pub struct Expression {
    source: String,
    root: Expr,
    /// How many enclosing `repeat`s the expression reads from, so evaluation can check that the
    /// caller passed their bindings.
    frames: usize,
}

impl Expression {
    /// The source text, as the template wrote it.
    pub fn source(&self) -> &str {
        &self.source
    }
}

/// A syntax tree node. A run of binary operators of one precedence is one `Chain`, and a run of
/// `.name` and `[index]` steps one `Access`, as the spec's nesting rule counts them.
#[derive(Debug, PartialEq)]
enum Expr {
    Literal(Value),
    List(Vec<Expr>),
    Name(Name),
    Access {
        base: Box<Expr>,
        steps: Vec<Step>,
    },
    Call {
        function: Function,
        args: Vec<Expr>,
    },
    Unary {
        op: UnaryOp,
        operand: Box<Expr>,
    },
    /// `&&` or `||` over two or more operands, which short-circuit.
    Logic {
        op: LogicOp,
        operands: Vec<Expr>,
    },
    /// Left-associative operators of one precedence, applied in order.
    Chain {
        first: Box<Expr>,
        rest: Vec<(BinaryOp, Expr)>,
    },
    Conditional {
        condition: Box<Expr>,
        then: Box<Expr>,
        otherwise: Box<Expr>,
    },
}

/// What a name refers to. Bindings count enclosing `repeat`s outward, the innermost being 0.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Name {
    State,
    Env,
    Item(usize),
    Index(usize),
}

#[derive(Debug, PartialEq)]
enum Step {
    Field(String),
    Index(Expr),
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum UnaryOp {
    Not,
    Negate,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum LogicOp {
    And,
    Or,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum BinaryOp {
    Equal,
    NotEqual,
    Less,
    LessEqual,
    Greater,
    GreaterEqual,
    Add,
    Subtract,
    Multiply,
    Divide,
    Remainder,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Function {
    Len,
    Range,
    Floor,
    Ceil,
    Round,
    Abs,
    Min,
    Max,
    String,
    Pad,
    FormatNumber,
    FormatTime,
}

impl Function {
    const ALL: [Function; 12] = [
        Function::Len,
        Function::Range,
        Function::Floor,
        Function::Ceil,
        Function::Round,
        Function::Abs,
        Function::Min,
        Function::Max,
        Function::String,
        Function::Pad,
        Function::FormatNumber,
        Function::FormatTime,
    ];

    fn from_name(name: &str) -> Option<Function> {
        Function::ALL
            .into_iter()
            .find(|function| function.name() == name)
    }

    const fn name(self) -> &'static str {
        match self {
            Function::Len => "len",
            Function::Range => "range",
            Function::Floor => "floor",
            Function::Ceil => "ceil",
            Function::Round => "round",
            Function::Abs => "abs",
            Function::Min => "min",
            Function::Max => "max",
            Function::String => "string",
            Function::Pad => "pad",
            Function::FormatNumber => "format_number",
            Function::FormatTime => "format_time",
        }
    }

    const fn arity(self) -> usize {
        match self {
            Function::Len
            | Function::Range
            | Function::Floor
            | Function::Ceil
            | Function::Round
            | Function::Abs
            | Function::String => 1,
            Function::Min
            | Function::Max
            | Function::Pad
            | Function::FormatNumber
            | Function::FormatTime => 2,
        }
    }
}
