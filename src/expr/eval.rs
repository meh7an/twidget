//! The evaluator: a syntax tree to a JSON value, spending fuel as it goes.

use alloc::borrow::Cow;
use alloc::string::String;
use alloc::vec::Vec;
use core::cmp::Ordering;

use super::{func, BinaryOp, Expr, Expression, LogicOp, Name, Step, UnaryOp};
use crate::budget;
use crate::json::{Object, Value};
use crate::{Error, Result};

/// The environment the app passes on every call, as `TWidgetEnv` carries it.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Env {
    /// Server-synced Unix time in milliseconds, read as `env.now`.
    pub now_ms: i64,
    /// The viewer's current UTC offset in minutes, read as `env.tz_offset`.
    pub tz_offset_minutes: i32,
    /// Whether the active theme is dark, read as `env.dark`.
    pub dark: bool,
    /// Whether the layout direction is right to left, read as `env.rtl`.
    pub rtl: bool,
}

impl Env {
    /// The fixed environment of tests, goldens and the CLI's defaults: 2026-09-26 09:41:27.123
    /// UTC seen from UTC+03:30, in the light theme, left to right.
    pub const TEST: Env = Env {
        now_ms: 1_790_415_687_123,
        tz_offset_minutes: 210,
        dark: false,
        rtl: false,
    };
}

/// The values the enclosing `repeat`s bind, innermost first. The default binds nothing.
#[derive(Debug, Clone, Copy, Default)]
pub struct Bindings<'v> {
    innermost: Option<&'v Frame<'v>>,
}

/// One `repeat` item: its value and its index.
#[derive(Debug)]
pub struct Frame<'v> {
    item: &'v Value,
    index: usize,
    outer: Bindings<'v>,
}

impl<'v> Frame<'v> {
    /// The item at `index` of a `repeat` inside `outer`.
    pub fn new(item: &'v Value, index: usize, outer: Bindings<'v>) -> Frame<'v> {
        Frame { item, index, outer }
    }

    /// The bindings in scope inside this item.
    pub fn bindings(&'v self) -> Bindings<'v> {
        Bindings {
            innermost: Some(self),
        }
    }
}

impl<'v> Bindings<'v> {
    fn frame(self, depth: usize) -> Option<&'v Frame<'v>> {
        let mut frame = self.innermost?;
        for _ in 0..depth {
            frame = frame.outer.innermost?;
        }
        Some(frame)
    }

    fn len(self) -> usize {
        let mut len = 0_usize;
        let mut frame = self.innermost;
        while let Some(current) = frame {
            len = len.saturating_add(1);
            frame = current.outer.innermost;
        }
        len
    }
}

/// Evaluates the expressions of one resolve against one state and environment, with one fuel
/// counter for all of them.
#[derive(Debug)]
pub struct Evaluator<'a> {
    state: &'a Object,
    env: &'a Env,
    meter: Meter,
}

impl<'a> Evaluator<'a> {
    /// An evaluator with the whole fuel budget.
    pub fn new(state: &'a Object, env: &'a Env) -> Evaluator<'a> {
        Evaluator {
            state,
            env,
            meter: Meter {
                fuel_left: budget::EVALUATION_FUEL.limit,
                reads_now: false,
            },
        }
    }

    /// Evaluates `expression` for the `repeat` items in `bindings`. Bad data yields `null`.
    ///
    /// # Errors
    /// [`Error::Budget`] when the fuel runs out, and for every later call too, or when `range`
    /// asks for more items than a `repeat` may hold; [`Error::InvalidArgument`] when `bindings`
    /// holds fewer items than the `repeat`s the expression was parsed inside.
    pub fn evaluate(&mut self, expression: &Expression, bindings: Bindings<'_>) -> Result<Value> {
        if bindings.len() < expression.frames {
            return Err(Error::InvalidArgument {
                reason: "fewer bindings than the repeats around the expression",
            });
        }
        let context = Context {
            state: self.state,
            env: self.env,
            bindings,
        };
        eval(&expression.root, context, &mut self.meter).map(Cow::into_owned)
    }

    /// Whether any evaluation so far read `env.now`, which makes the tree time-dependent.
    pub fn reads_now(&self) -> bool {
        self.meter.reads_now
    }

    /// The fuel left for the rest of the resolve.
    pub fn fuel_left(&self) -> usize {
        self.meter.fuel_left
    }
}

/// What every evaluation in a resolve shares.
#[derive(Debug)]
pub(super) struct Meter {
    fuel_left: usize,
    reads_now: bool,
}

impl Meter {
    /// Spends `steps` of fuel. Running out fails the resolve, so the counter then stays empty.
    pub(super) fn spend(&mut self, steps: usize) -> Result<()> {
        if let Some(left) = self.fuel_left.checked_sub(steps) {
            self.fuel_left = left;
            Ok(())
        } else {
            self.fuel_left = 0;
            Err(budget::EVALUATION_FUEL.exceeded())
        }
    }
}

/// What an expression can read.
#[derive(Debug, Clone, Copy)]
pub(super) struct Context<'r> {
    pub(super) state: &'r Object,
    pub(super) env: &'r Env,
    pub(super) bindings: Bindings<'r>,
}

/// Evaluates one node. Every node costs a step; a run of operators or steps costs one per
/// operator or step, as if each were a node of its own.
pub(super) fn eval<'r>(
    expr: &'r Expr,
    context: Context<'r>,
    meter: &mut Meter,
) -> Result<Cow<'r, Value>> {
    meter.spend(1)?;
    match expr {
        Expr::Literal(value) => Ok(Cow::Borrowed(value)),
        Expr::List(items) => list(items, context, meter),
        Expr::Name(name) => Ok(finish(locate(*name, context), context, meter)),
        Expr::Access { base, steps } => access(base, steps, context, meter),
        Expr::Call { function, args } => func::call(*function, args, context, meter),
        Expr::Unary { op, operand } => {
            let value = eval(operand, context, meter)?;
            Ok(Cow::Owned(unary(*op, &value)))
        }
        Expr::Logic { op, operands } => logic(*op, operands, context, meter),
        Expr::Chain { first, rest } => chain(first, rest, context, meter),
        Expr::Conditional {
            condition,
            then,
            otherwise,
        } => {
            let test = eval(condition, context, meter)?;
            match as_bool(&test) {
                Some(true) => eval(then, context, meter),
                Some(false) => eval(otherwise, context, meter),
                None => Ok(Cow::Owned(Value::Null)),
            }
        }
    }
}

fn list<'r>(items: &'r [Expr], context: Context<'r>, meter: &mut Meter) -> Result<Cow<'r, Value>> {
    let mut values = Vec::with_capacity(items.len());
    for item in items {
        values.push(eval(item, context, meter)?.into_owned());
    }
    meter.spend(values.len())?;
    Ok(Cow::Owned(Value::Array(values)))
}

/// Where a run of steps has got to: somewhere in a value, or at the root of state or of the
/// environment, which are not stored as values.
enum Place<'r> {
    Value(Cow<'r, Value>),
    State,
    Env,
}

fn locate<'r>(name: Name, context: Context<'r>) -> Place<'r> {
    match name {
        Name::State => Place::State,
        Name::Env => Place::Env,
        Name::Item(depth) => Place::Value(match context.bindings.frame(depth) {
            Some(frame) => Cow::Borrowed(frame.item),
            None => Cow::Owned(Value::Null),
        }),
        Name::Index(depth) => Place::Value(Cow::Owned(
            context
                .bindings
                .frame(depth)
                .map_or(Value::Null, |frame| count(frame.index)),
        )),
    }
}

/// The value at `place`; reading the whole environment reads `env.now`.
fn finish<'r>(place: Place<'r>, context: Context<'r>, meter: &mut Meter) -> Cow<'r, Value> {
    match place {
        Place::Value(value) => value,
        Place::State => Cow::Owned(Value::Object(context.state.clone())),
        Place::Env => {
            meter.reads_now = true;
            let mut env = Object::default();
            for name in ENV_FIELDS {
                env.push(String::from(name), env_field(context.env, name));
            }
            Cow::Owned(Value::Object(env))
        }
    }
}

const ENV_FIELDS: [&str; 4] = ["now", "tz_offset", "dark", "rtl"];

fn env_field(env: &Env, name: &str) -> Value {
    match name {
        "now" => Value::Number(env.now_ms as f64),
        "tz_offset" => Value::Number(f64::from(env.tz_offset_minutes)),
        "dark" => Value::Bool(env.dark),
        "rtl" => Value::Bool(env.rtl),
        _ => Value::Null,
    }
}

fn access<'r>(
    base: &'r Expr,
    steps: &'r [Step],
    context: Context<'r>,
    meter: &mut Meter,
) -> Result<Cow<'r, Value>> {
    let mut place = if let Expr::Name(name) = base {
        // The name is a node of its own, even though it is read in place.
        meter.spend(1)?;
        locate(*name, context)
    } else {
        Place::Value(eval(base, context, meter)?)
    };
    for (position, step) in steps.iter().enumerate() {
        if position > 0 {
            meter.spend(1)?;
        }
        let index;
        let key = match step {
            Step::Field(name) => Key::Name(name),
            Step::Index(expr) => {
                index = eval(expr, context, meter)?;
                key_of(&index)
            }
        };
        place = step_into(place, &key, context, meter);
    }
    Ok(finish(place, context, meter))
}

/// What a step looks up: a map member or a list position.
enum Key<'k> {
    Name(&'k str),
    Position(f64),
    Invalid,
}

fn key_of(value: &Value) -> Key<'_> {
    match value {
        Value::String(name) => Key::Name(name),
        Value::Number(position) => Key::Position(*position),
        Value::Null | Value::Bool(_) | Value::Array(_) | Value::Object(_) => Key::Invalid,
    }
}

fn step_into<'r>(
    place: Place<'r>,
    key: &Key<'_>,
    context: Context<'r>,
    meter: &mut Meter,
) -> Place<'r> {
    let value = match place {
        Place::State => match key {
            Key::Name(name) => context.state.get(name).map(Cow::Borrowed),
            Key::Position(_) | Key::Invalid => None,
        },
        Place::Env => match key {
            Key::Name(name) => {
                if *name == "now" {
                    meter.reads_now = true;
                }
                Some(Cow::Owned(env_field(context.env, name)))
            }
            Key::Position(_) | Key::Invalid => None,
        },
        Place::Value(Cow::Borrowed(value)) => child(value, key).map(Cow::Borrowed),
        Place::Value(Cow::Owned(value)) => child(&value, key).cloned().map(Cow::Owned),
    };
    Place::Value(value.unwrap_or(Cow::Owned(Value::Null)))
}

fn child<'v>(value: &'v Value, key: &Key<'_>) -> Option<&'v Value> {
    match value {
        Value::Array(items) => {
            let Key::Position(position) = key else {
                return None;
            };
            items.get(position_in(*position, items.len())?)
        }
        Value::Object(members) => {
            let Key::Name(name) = key else {
                return None;
            };
            members.get(name)
        }
        Value::Null | Value::Bool(_) | Value::Number(_) | Value::String(_) => None,
    }
}

/// The list index `position` names, when it is a whole number inside the list.
fn position_in(position: f64, len: usize) -> Option<usize> {
    let inside = position.fract() == 0.0 && position >= 0.0 && position < len as f64;
    inside.then_some(position as usize)
}

/// `&&` or `||`: the right side is evaluated only when the left one is a boolean that does not
/// settle the result, and anything but booleans yields `null`.
fn logic<'r>(
    op: LogicOp,
    operands: &'r [Expr],
    context: Context<'r>,
    meter: &mut Meter,
) -> Result<Cow<'r, Value>> {
    let mut operands = operands.iter();
    let mut result = match operands.next() {
        Some(first) => as_bool(eval(first, context, meter)?.as_ref()),
        None => None,
    };
    for (position, operand) in operands.enumerate() {
        if position > 0 {
            meter.spend(1)?;
        }
        result = match (op, result) {
            (LogicOp::And, Some(true)) | (LogicOp::Or, Some(false)) => {
                as_bool(eval(operand, context, meter)?.as_ref())
            }
            (LogicOp::And, Some(false) | None) | (LogicOp::Or, Some(true) | None) => result,
        };
    }
    Ok(Cow::Owned(result.map_or(Value::Null, Value::Bool)))
}

/// Left-associative operators of one precedence, applied in order.
fn chain<'r>(
    first: &'r Expr,
    rest: &'r [(BinaryOp, Expr)],
    context: Context<'r>,
    meter: &mut Meter,
) -> Result<Cow<'r, Value>> {
    let mut result = eval(first, context, meter)?;
    for (position, (op, operand)) in rest.iter().enumerate() {
        if position > 0 {
            meter.spend(1)?;
        }
        let right = eval(operand, context, meter)?;
        result = Cow::Owned(binary(*op, &result, &right, meter)?);
    }
    Ok(result)
}

fn unary(op: UnaryOp, value: &Value) -> Value {
    match op {
        UnaryOp::Not => as_bool(value).map_or(Value::Null, |value| Value::Bool(!value)),
        UnaryOp::Negate => as_number(value).map_or(Value::Null, |value| Value::Number(-value)),
    }
}

fn binary(op: BinaryOp, left: &Value, right: &Value, meter: &mut Meter) -> Result<Value> {
    let value = match op {
        BinaryOp::Equal => Value::Bool(equal(left, right, meter)?),
        BinaryOp::NotEqual => Value::Bool(!equal(left, right, meter)?),
        BinaryOp::Less => Value::Bool(order(left, right).is_some_and(Ordering::is_lt)),
        BinaryOp::LessEqual => Value::Bool(order(left, right).is_some_and(Ordering::is_le)),
        BinaryOp::Greater => Value::Bool(order(left, right).is_some_and(Ordering::is_gt)),
        BinaryOp::GreaterEqual => Value::Bool(order(left, right).is_some_and(Ordering::is_ge)),
        BinaryOp::Add => add(left, right, meter)?,
        BinaryOp::Subtract => arithmetic(left, right, |x, y| x - y),
        BinaryOp::Multiply => arithmetic(left, right, |x, y| x * y),
        BinaryOp::Divide => arithmetic(left, right, |x, y| x / y),
        BinaryOp::Remainder => arithmetic(left, right, |x, y| x % y),
    };
    Ok(value)
}

/// Structural equality, spending a step per list element or map member compared. Values of
/// different types are unequal, and member order is not part of a map's value.
fn equal(left: &Value, right: &Value, meter: &mut Meter) -> Result<bool> {
    match left {
        Value::Null => Ok(matches!(right, Value::Null)),
        Value::Bool(x) => Ok(as_bool(right) == Some(*x)),
        Value::Number(x) => Ok(as_number(right).is_some_and(|y| *x == y)),
        Value::String(x) => Ok(as_str(right) == Some(x.as_str())),
        Value::Array(items) => {
            let Value::Array(others) = right else {
                return Ok(false);
            };
            if items.len() != others.len() {
                return Ok(false);
            }
            for (item, other) in items.iter().zip(others) {
                meter.spend(1)?;
                if !equal(item, other, meter)? {
                    return Ok(false);
                }
            }
            Ok(true)
        }
        Value::Object(members) => {
            let Value::Object(others) = right else {
                return Ok(false);
            };
            if members.len() != others.len() {
                return Ok(false);
            }
            for ((key, value), (other_key, other)) in
                sorted(members).into_iter().zip(sorted(others))
            {
                meter.spend(1)?;
                if key != other_key || !equal(value, other, meter)? {
                    return Ok(false);
                }
            }
            Ok(true)
        }
    }
}

/// A map's members by key; keys are unique, so the order is total.
fn sorted(object: &Object) -> Vec<(&str, &Value)> {
    let mut members: Vec<(&str, &Value)> = object.iter().collect();
    members.sort_unstable_by_key(|(key, _)| *key);
    members
}

/// How two numbers, or two strings by Unicode scalar values, compare; any other pair has no
/// order, so every ordering comparison of it is false.
fn order(left: &Value, right: &Value) -> Option<Ordering> {
    if let (Some(x), Some(y)) = (as_number(left), as_number(right)) {
        return x.partial_cmp(&y);
    }
    if let (Some(x), Some(y)) = (as_str(left), as_str(right)) {
        return Some(x.cmp(y));
    }
    None
}

fn arithmetic(left: &Value, right: &Value, operation: fn(f64, f64) -> f64) -> Value {
    match (as_number(left), as_number(right)) {
        (Some(x), Some(y)) => number(operation(x, y)),
        _ => Value::Null,
    }
}

/// `+`: numbers add; with a string on either side, both sides join as `string()` writes them.
fn add(left: &Value, right: &Value, meter: &mut Meter) -> Result<Value> {
    if let (Some(x), Some(y)) = (as_number(left), as_number(right)) {
        return Ok(number(x + y));
    }
    if as_str(left).is_none() && as_str(right).is_none() {
        return Ok(Value::Null);
    }
    let (Some(left), Some(right)) = (func::text(left), func::text(right)) else {
        return Ok(Value::Null);
    };
    let mut joined = String::with_capacity(left.len().saturating_add(right.len()));
    joined.push_str(&left);
    joined.push_str(&right);
    func::produced(joined, meter)
}

/// A number, or `null` for a result that is not finite.
pub(super) fn number(x: f64) -> Value {
    if x.is_finite() {
        Value::Number(x)
    } else {
        Value::Null
    }
}

/// A count as a number. Counts here never approach 2^53, where `f64` would round them.
pub(super) fn count(n: usize) -> Value {
    Value::Number(n as f64)
}

pub(super) fn as_bool(value: &Value) -> Option<bool> {
    match value {
        Value::Bool(value) => Some(*value),
        Value::Null | Value::Number(_) | Value::String(_) | Value::Array(_) | Value::Object(_) => {
            None
        }
    }
}

pub(super) fn as_number(value: &Value) -> Option<f64> {
    match value {
        Value::Number(value) => Some(*value),
        Value::Null | Value::Bool(_) | Value::String(_) | Value::Array(_) | Value::Object(_) => {
            None
        }
    }
}

pub(super) fn as_str(value: &Value) -> Option<&str> {
    match value {
        Value::String(value) => Some(value),
        Value::Null | Value::Bool(_) | Value::Number(_) | Value::Array(_) | Value::Object(_) => {
            None
        }
    }
}
