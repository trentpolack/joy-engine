// Copyright (c) 2026 Trent Polack. All Rights Reserved.
// Licensed under the MIT License.

//! The particle script language: an original, bounded numeric language for `.joyfx` spawn and update programs.
//!
//! Sources compile once into a typed syntax tree whose names resolve to numeric storage slots; nothing is evaluated as
//! host code. Statements assign particle fields, read-only inputs, or block-scoped locals (`let`), and branch with
//! `if`/`else`. Values are scalars, XYZ vectors, or RGBA colors with component access (`position.x`, `color.a`).
//!
//! Unary minus binds tighter than `^` (`-a^b` is `(-a)^b`), `&&` and `||` short-circuit, and operands evaluate left
//! to right, so seeded `rand()` streams are stable across versions. Conditions treat zero and NaN as false. Every
//! evaluated expression level costs one instruction unit per component. Budgets: 12,000 characters, 2,000 tokens,
//! 32 nested blocks, and 64 nested expression levels (blocks count toward the expression limit). Every intermediate
//! expression result must be finite and within ±1 billion. Diagnostics carry one-based lines and character columns.

use std::collections::{
    HashMap,
    HashSet,
};

use indexmap::IndexMap;
use joy_core::RandomSource;
use joy_core::math::deterministic;

use crate::constants::ParticleValueKind;

/// Writable per-particle scalar fields, in storage order.
pub const PARTICLE_FIELDS: [&str; 24] = [
    "x",
    "y",
    "z",
    "vx",
    "vy",
    "vz",
    "ax",
    "ay",
    "az",
    "drag",
    "size",
    "rotation",
    "spin",
    "r",
    "g",
    "b",
    "alpha",
    "lightIntensity",
    "lightRange",
    "lifetime",
    "u0",
    "u1",
    "u2",
    "u3",
];

/// Read-only script inputs supplied by the simulation, in storage order after [`PARTICLE_FIELDS`].
pub const PARTICLE_INPUTS: [&str; 13] = [
    "dt", "age", "t", "index", "time", "originX", "originY", "originZ", "parentVx", "parentVy", "parentVz", "pi", "tau",
];

/// Built-in functions and their accepted argument counts, shared with authoring tools.
pub const PARTICLE_FUNCTION_ARITIES: [(&str, &[usize]); 21] = [
    ("sin", &[1]),
    ("cos", &[1]),
    ("tan", &[1]),
    ("abs", &[1]),
    ("sqrt", &[1]),
    ("floor", &[1]),
    ("ceil", &[1]),
    ("round", &[1]),
    ("exp", &[1]),
    ("log", &[1]),
    ("min", &[2]),
    ("max", &[2]),
    ("pow", &[2]),
    ("atan2", &[2]),
    ("clamp", &[3]),
    ("mix", &[3]),
    ("smoothstep", &[3]),
    ("rand", &[0, 2]),
    ("noise", &[1]),
    ("vector", &[3]),
    ("rgba", &[4]),
];

/// Built-in container names and the scalar fields or inputs they expose (`position.x` is `x`).
pub const PARTICLE_CONTAINERS: [(&str, &[&str]); 6] = [
    ("position", &["x", "y", "z"]),
    ("velocity", &["vx", "vy", "vz"]),
    ("acceleration", &["ax", "ay", "az"]),
    ("color", &["r", "g", "b", "alpha"]),
    ("origin", &["originX", "originY", "originZ"]),
    ("parentVelocity", &["parentVx", "parentVy", "parentVz"]),
];

/// Maximum source length in characters.
pub const MAXIMUM_SCRIPT_LENGTH: usize = 12_000;

/// Maximum token count per source.
pub const MAXIMUM_SCRIPT_TOKENS: usize = 2_000;

/// Maximum nested block depth.
const MAXIMUM_BLOCK_DEPTH: u32 = 32;

/// Maximum combined block and expression nesting depth.
const MAXIMUM_EXPRESSION_DEPTH: u32 = 64;

/// Largest magnitude any intermediate script value may reach.
const MAXIMUM_SCRIPT_MAGNITUDE: f64 = 1e9;

/// Spelling of the synthetic end-of-source token, as it appears in diagnostics.
const END_TOKEN: &str = "<end>";

/// Storage slot of the first read-only input (`dt`); fields occupy the slots before it.
pub(crate) const INPUT_SLOT_BASE: usize = PARTICLE_FIELDS.len();

/// Number of fixed field and input slots that precede parameter and local storage.
pub(crate) const FIXED_SLOT_COUNT: usize = PARTICLE_FIELDS.len() + PARTICLE_INPUTS.len();

/// A script compilation or evaluation failure with its source location.
///
/// `Display` reads `"{message} (line {line}, column {column})"`. Lines and columns are one-based, and columns count
/// characters, so editors can place diagnostics directly.
#[derive(Clone, Debug, PartialEq, Eq, thiserror::Error)]
#[error("{message} (line {line}, column {column})")]
pub struct ParticleScriptError {
    /// What went wrong, without the location suffix.
    pub message: String,
    /// One-based source line.
    pub line: u32,
    /// One-based character column.
    pub column: u32,
}

/// Remaining instruction units shared by every script run in one simulation step.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct ParticleScriptBudget {
    /// Units left; evaluation fails once this drops below zero.
    pub remaining: i64,
}

/// A compiled spawn or update program. Immutable and shareable; evaluation state is supplied per run.
#[derive(Clone, Debug)]
pub struct ParticleScript {
    statements: Vec<Statement>,
    slot_names: Vec<String>,
    parameter_slots: Vec<usize>,
}

/// Compile a script with read-only scalar `parameters` and named read-only `containers`; the TypeScript
/// `compileParticleScript`. See [`ParticleScript::compile`].
pub fn compile_particle_script(
    source: &str,
    parameters: &[&str],
    containers: &[(&str, ParticleValueKind)],
) -> Result<ParticleScript, ParticleScriptError> {
    ParticleScript::compile(source, parameters, containers)
}

impl ParticleScript {
    /// Compile `source`.
    ///
    /// `parameters` are read-only scalar names (effect parameter components such as `speed` or `windX`); they take
    /// precedence over the built-in container names. `containers` name read-only vector or color views over parameter
    /// components (`wind` reads `windX`, `windY`, `windZ`; a color `tint` reads `tint.r` through `tint.a`). Fails with
    /// the first diagnostic in source order; no partial program is returned.
    pub fn compile(
        source: &str,
        parameters: &[&str],
        containers: &[(&str, ParticleValueKind)],
    ) -> Result<Self, ParticleScriptError> {
        let mut parser = Parser::new(source, parameters, containers)?;
        let statements = parser.block(false)?;
        let parameter_slots = parameters.iter().map(|name| parser.slot(name)).collect();
        Ok(Self { statements, slot_names: parser.slot_names, parameter_slots })
    }

    /// Run against a named numeric record, as the TypeScript `runParticleScript` does with its `values` object.
    ///
    /// Names absent from `values` read as NaN (TypeScript's `undefined`), which fails at the first expression that
    /// observes them. Fields, inputs, and locals written by the script are stored back into `values`, including writes
    /// made before a failure. `random` supplies `rand()`; `budget` is decremented by the instructions evaluated.
    pub fn run(
        &self,
        values: &mut IndexMap<String, f64>,
        random: &mut dyn RandomSource,
        budget: &mut ParticleScriptBudget,
    ) -> Result<(), ParticleScriptError> {
        let mut slots: Vec<f64> =
            self.slot_names.iter().map(|name| values.get(name).copied().unwrap_or(f64::NAN)).collect();
        let result = self.execute(&mut slots, random, &mut budget.remaining);
        for (name, value) in self.slot_names.iter().zip(slots) {
            if !value.is_nan() || values.contains_key(name) {
                values.insert(name.clone(), value);
            }
        }
        result
    }

    /// Storage slots this program reads or writes: fields, inputs, parameters, then locals.
    pub(crate) fn slot_count(&self) -> usize {
        self.slot_names.len()
    }

    /// Slot of each compile-time parameter, in the order they were supplied.
    pub(crate) fn parameter_slots(&self) -> &[usize] {
        &self.parameter_slots
    }

    /// Evaluate against slot storage of at least [`slot_count`](Self::slot_count) values.
    pub(crate) fn execute(
        &self,
        slots: &mut [f64],
        random: &mut dyn RandomSource,
        budget: &mut i64,
    ) -> Result<(), ParticleScriptError> {
        let mut machine = Machine { slots, random, budget };
        run_statements(&self.statements, &mut machine)
    }
}

// ---------------------------------------------------------------------------------------------------------------------
// Syntax tree
// ---------------------------------------------------------------------------------------------------------------------

/// A source location for runtime diagnostics.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
struct Location {
    line: u32,
    column: u32,
}

/// A statically resolved name: its value kind, storage slots, and writability.
#[derive(Clone, Debug)]
struct Reference {
    kind: ParticleValueKind,
    slots: Vec<usize>,
    writable: bool,
}

#[derive(Clone, Debug)]
enum Statement {
    If { condition: Expr, yes: Vec<Statement>, no: Vec<Statement> },
    Assign { location: Location, operator: Option<BinaryOperator>, expression: Expr, target: Reference },
}

/// One parsed expression level: charged against the budget and checked for finite results when evaluated.
#[derive(Clone, Debug)]
struct Expr {
    location: Location,
    kind: ParticleValueKind,
    node: Node,
}

/// Unchecked expression structure inside one [`Expr`] level.
#[derive(Clone, Debug)]
enum Node {
    Literal(f64),
    Read(Reference),
    Unary { operator: UnaryOperator, operand: Box<Expr> },
    Group(Box<Expr>),
    Construct(Vec<Expr>),
    MixComponents(Box<[Expr; 3]>),
    Call { function: Builtin, arguments: Vec<Expr> },
    Binary { operator: BinaryOperator, left: Box<Node>, right: Box<Expr> },
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum UnaryOperator {
    Negate,
    Plus,
    Not,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum BinaryOperator {
    Or,
    And,
    Equal,
    NotEqual,
    Less,
    Greater,
    LessEqual,
    GreaterEqual,
    Add,
    Subtract,
    Multiply,
    Divide,
    Remainder,
    Power,
}

impl BinaryOperator {
    /// Parse an infix operator with its precedence; higher binds tighter.
    fn parse(text: &str) -> Option<(Self, u32)> {
        Some(match text {
            "||" => (Self::Or, 1),
            "&&" => (Self::And, 2),
            "==" => (Self::Equal, 3),
            "!=" => (Self::NotEqual, 3),
            "<" => (Self::Less, 4),
            ">" => (Self::Greater, 4),
            "<=" => (Self::LessEqual, 4),
            ">=" => (Self::GreaterEqual, 4),
            "+" => (Self::Add, 10),
            "-" => (Self::Subtract, 10),
            "*" => (Self::Multiply, 20),
            "/" => (Self::Divide, 20),
            "%" => (Self::Remainder, 20),
            "^" => (Self::Power, 40),
            _ => return None,
        })
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Builtin {
    Sin,
    Cos,
    Tan,
    Abs,
    Sqrt,
    Floor,
    Ceil,
    Round,
    Exp,
    Log,
    Min,
    Max,
    Pow,
    Atan2,
    Clamp,
    Mix,
    Smoothstep,
    Rand,
    Noise,
}

impl Builtin {
    /// Resolve a scalar built-in and its accepted argument counts (`vector` and `rgba` are constructors, not built-ins).
    fn parse(name: &str) -> Option<(Self, &'static [usize])> {
        let function = match name {
            "sin" => Self::Sin,
            "cos" => Self::Cos,
            "tan" => Self::Tan,
            "abs" => Self::Abs,
            "sqrt" => Self::Sqrt,
            "floor" => Self::Floor,
            "ceil" => Self::Ceil,
            "round" => Self::Round,
            "exp" => Self::Exp,
            "log" => Self::Log,
            "min" => Self::Min,
            "max" => Self::Max,
            "pow" => Self::Pow,
            "atan2" => Self::Atan2,
            "clamp" => Self::Clamp,
            "mix" => Self::Mix,
            "smoothstep" => Self::Smoothstep,
            "rand" => Self::Rand,
            "noise" => Self::Noise,
            _ => return None,
        };
        Some((function, arities(name)))
    }

    /// Apply to already evaluated scalar arguments; `rand` draws from `random` after its arguments evaluate.
    fn call(self, arguments: &[f64], random: &mut dyn RandomSource) -> f64 {
        let argument = |index: usize| arguments.get(index).copied().unwrap_or(f64::NAN);
        match self {
            Self::Sin => deterministic::sin(argument(0)),
            Self::Cos => deterministic::cos(argument(0)),
            Self::Tan => deterministic::tan(argument(0)),
            Self::Abs => argument(0).abs(),
            Self::Sqrt => argument(0).sqrt(),
            Self::Floor => argument(0).floor(),
            Self::Ceil => argument(0).ceil(),
            Self::Round => argument(0).round(),
            Self::Exp => deterministic::exp(argument(0)),
            Self::Log => deterministic::ln(argument(0)),
            Self::Min => f64::min(argument(0), argument(1)),
            Self::Max => f64::max(argument(0), argument(1)),
            Self::Pow => deterministic::powf(argument(0), argument(1)),
            Self::Atan2 => deterministic::atan2(argument(0), argument(1)),
            Self::Clamp => f64::max(argument(1), f64::min(argument(2), argument(0))),
            Self::Mix => argument(0) + ((argument(1) - argument(0)) * argument(2)),
            Self::Smoothstep => {
                let (edge0, edge1, value) = (argument(0), argument(1), argument(2));
                let t = ((value - edge0) / (edge1 - edge0)).clamp(0.0, 1.0);
                (t * t) * (3.0 - (2.0 * t))
            }
            Self::Rand => {
                if arguments.len() == 2 {
                    let low = argument(0);
                    low + (random.next_unit() * (argument(1) - low))
                } else {
                    random.next_unit()
                }
            }
            Self::Noise => noise(argument(0)),
        }
    }
}

/// Accepted argument counts of a built-in or constructor name; empty for unknown names.
fn arities(name: &str) -> &'static [usize] {
    PARTICLE_FUNCTION_ARITIES.iter().find(|(function, _)| *function == name).map_or(&[], |(_, counts)| counts)
}

/// Smooth one-dimensional value noise in `[-1, 1]` from a sine hash; authored effects depend on its exact values.
fn noise(x: f64) -> f64 {
    let hash = |n: f64| {
        let value = deterministic::sin((n * 127.1) + 311.7) * 43758.5453;
        ((value - value.floor()) * 2.0) - 1.0
    };
    let cell = x.floor();
    let t = x - cell;
    hash(cell) + ((((hash(cell + 1.0) - hash(cell)) * t) * t) * (3.0 - (2.0 * t)))
}

// ---------------------------------------------------------------------------------------------------------------------
// Tokenizer
// ---------------------------------------------------------------------------------------------------------------------

#[derive(Clone, Copy, Debug)]
struct Token<'a> {
    text: &'a str,
    line: u32,
    column: u32,
}

impl Token<'_> {
    fn location(&self) -> Location {
        Location { line: self.line, column: self.column }
    }

    fn is_end(&self) -> bool {
        self.text == END_TOKEN
    }
}

fn error_at(message: impl Into<String>, line: u32, column: u32) -> ParticleScriptError {
    ParticleScriptError { message: message.into(), line, column }
}

fn error(message: impl Into<String>, token: &Token<'_>) -> ParticleScriptError {
    error_at(message, token.line, token.column)
}

/// Whether a condition value counts as true: anything but zero and NaN.
fn is_true(value: f64) -> bool {
    value != 0.0 && !value.is_nan()
}

fn located_error(message: impl Into<String>, location: Location) -> ParticleScriptError {
    error_at(message, location.line, location.column)
}

/// Split source into tokens with one-based lines and character columns, ending with the synthetic `<end>` token.
fn tokenize(source: &str) -> Result<Vec<Token<'_>>, ParticleScriptError> {
    if source.chars().count() > MAXIMUM_SCRIPT_LENGTH {
        return Err(error_at("Script exceeds 12,000 characters", 1, 1));
    }

    let mut tokens = Vec::new();
    let mut offset = 0;
    let mut line = 1_u32;
    let mut column = 1_u32;
    while let Some(character) = source[offset..].chars().next() {
        // Comments run to the end of the line; the newline itself is handled as whitespace.
        if character == '#' || source[offset..].starts_with("//") {
            while let Some(comment) = source[offset..].chars().next() {
                if comment == '\n' {
                    break;
                }
                offset += comment.len_utf8();
                column += 1;
            }
            continue;
        }
        if character.is_whitespace() {
            if character == '\n' {
                line += 1;
                column = 1;
            } else {
                column += 1;
            }
            offset += character.len_utf8();
            continue;
        }

        let Some(length) = match_token(&source.as_bytes()[offset..]) else {
            return Err(error_at(format!("Unexpected character '{character}'"), line, column));
        };
        tokens.push(Token { text: &source[offset..offset + length], line, column });
        if tokens.len() > MAXIMUM_SCRIPT_TOKENS {
            return Err(error("Script exceeds 2,000 tokens", &tokens[tokens.len() - 1]));
        }
        offset += length;
        // Tokens are ASCII, so their byte length is their character count.
        column += length as u32;
    }
    tokens.push(Token { text: END_TOKEN, line, column });
    Ok(tokens)
}

/// Byte length of the token at the start of `rest`, following the TypeScript pattern's alternation order: numbers,
/// identifiers, two-character operators, then single-character punctuation.
fn match_token(rest: &[u8]) -> Option<usize> {
    let digits = |from: usize| rest[from..].iter().take_while(|byte| byte.is_ascii_digit()).count();
    let first = *rest.first()?;

    // Numbers: `1`, `1.`, `1.5`, `.5`, with an optional exponent that must contain digits.
    let mantissa = if first.is_ascii_digit() {
        let whole = digits(0);
        if rest.get(whole) == Some(&b'.') { whole + 1 + digits(whole + 1) } else { whole }
    } else if first == b'.' && rest.get(1).is_some_and(u8::is_ascii_digit) {
        1 + digits(1)
    } else {
        0
    };
    if mantissa > 0 {
        let mut length = mantissa;
        if matches!(rest.get(length), Some(b'e' | b'E')) {
            let sign = usize::from(matches!(rest.get(length + 1), Some(b'+' | b'-')));
            let exponent = digits(length + 1 + sign);
            if exponent > 0 {
                length += 1 + sign + exponent;
            }
        }
        return Some(length);
    }

    if first.is_ascii_alphabetic() || first == b'_' {
        return Some(1 + rest[1..].iter().take_while(|byte| byte.is_ascii_alphanumeric() || **byte == b'_').count());
    }

    let second = rest.get(1).copied();
    if (b"+-*/=!<>".contains(&first) && second == Some(b'='))
        || (first == b'&' && second == Some(b'&'))
        || (first == b'|' && second == Some(b'|'))
    {
        return Some(2);
    }
    b"+-*/%^=<>!().,{};".contains(&first).then_some(1)
}

/// Parse a numeric token; trailing-dot forms such as `1.` and `1.e3` are valid, as in JavaScript's `Number()`.
fn parse_number(text: &str) -> f64 {
    let normalized = match text.find('.') {
        Some(dot) if !text[dot + 1..].starts_with(|character: char| character.is_ascii_digit()) => {
            format!("{}0{}", &text[..=dot], &text[dot + 1..])
        }
        _ => text.to_owned(),
    };
    normalized.parse().unwrap_or(f64::NAN)
}

// ---------------------------------------------------------------------------------------------------------------------
// Parser
// ---------------------------------------------------------------------------------------------------------------------

struct Parser<'a> {
    tokens: Vec<Token<'a>>,
    cursor: usize,
    // Blocks and expression levels share one depth counter, as in TypeScript.
    depth: u32,
    // Compile-time names in scope; locals leave scope at the end of their block.
    references: HashMap<String, Reference>,
    // Built-in container names a `let` may still shadow in this scope.
    shadowable: HashSet<String>,
    // Storage keys (the TypeScript `values` record keys) mapped to slots; never scoped.
    slots: HashMap<String, usize>,
    slot_names: Vec<String>,
}

impl<'a> Parser<'a> {
    fn new(
        source: &'a str,
        parameters: &[&str],
        containers: &[(&str, ParticleValueKind)],
    ) -> Result<Self, ParticleScriptError> {
        let tokens = tokenize(source)?;
        let mut parser = Self {
            tokens,
            cursor: 0,
            depth: 0,
            references: HashMap::new(),
            shadowable: HashSet::new(),
            slots: HashMap::new(),
            slot_names: Vec::new(),
        };

        // Fields and inputs occupy the fixed slots, in that order.
        for name in PARTICLE_FIELDS.iter().chain(PARTICLE_INPUTS.iter()) {
            let slot = parser.slot(name);
            let writable = PARTICLE_FIELDS.contains(name);
            parser
                .references
                .insert((*name).to_owned(), Reference { kind: ParticleValueKind::Scalar, slots: vec![slot], writable });
        }
        for (name, fields) in PARTICLE_CONTAINERS {
            let kind = if name == "color" { ParticleValueKind::Color } else { ParticleValueKind::Vector };
            let slots = fields.iter().map(|field| parser.slot(field)).collect();
            let writable = name != "origin" && name != "parentVelocity";
            parser.references.insert(name.to_owned(), Reference { kind, slots, writable });
        }

        // Version-1 parameter names keep precedence over the newer built-in container names.
        for name in parameters {
            let slot = parser.slot(name);
            parser.references.insert(
                (*name).to_owned(),
                Reference { kind: ParticleValueKind::Scalar, slots: vec![slot], writable: false },
            );
        }
        parser.shadowable = PARTICLE_CONTAINERS
            .iter()
            .map(|(name, _)| *name)
            .filter(|name| !parameters.contains(name) && !containers.iter().any(|(container, _)| container == name))
            .map(str::to_owned)
            .collect();
        for (name, kind) in containers {
            let fields: Vec<String> = if *kind == ParticleValueKind::Vector {
                ["X", "Y", "Z"].iter().map(|axis| format!("{name}{axis}")).collect()
            } else {
                ["r", "g", "b", "a"].iter().map(|channel| format!("{name}.{channel}")).collect()
            };
            let slots = fields.iter().map(|field| parser.slot(field)).collect();
            parser.references.insert((*name).to_owned(), Reference { kind: *kind, slots, writable: false });
        }
        Ok(parser)
    }

    /// The storage slot for a key, allocating the next slot on first use.
    fn slot(&mut self, name: &str) -> usize {
        if let Some(slot) = self.slots.get(name) {
            return *slot;
        }
        let slot = self.slot_names.len();
        self.slots.insert(name.to_owned(), slot);
        self.slot_names.push(name.to_owned());
        slot
    }

    fn current(&self) -> Token<'a> {
        self.tokens.get(self.cursor).copied().unwrap_or(self.tokens[self.tokens.len() - 1])
    }

    fn take(&mut self, value: &str) -> bool {
        if self.current().text != value {
            return false;
        }
        self.cursor += 1;
        true
    }

    fn expect(&mut self, value: &str) -> Result<(), ParticleScriptError> {
        if self.take(value) {
            return Ok(());
        }
        let current = self.current();
        Err(error(format!("Expected '{value}', found '{}'", current.text), &current))
    }

    /// Parse statements until `}` (nested) or the end of the source, restoring the enclosing scope afterwards.
    fn block(&mut self, nested: bool) -> Result<Vec<Statement>, ParticleScriptError> {
        self.depth += 1;
        if self.depth > MAXIMUM_BLOCK_DEPTH {
            return Err(error("Script nesting limit exceeded", &self.current()));
        }
        let outer_references = self.references.clone();
        let outer_shadowable = self.shadowable.clone();

        let mut statements = Vec::new();
        while !self.current().is_end() && self.current().text != "}" {
            if self.take(";") {
                continue;
            }
            statements.push(self.statement()?);
        }
        if nested {
            self.expect("}")?;
        } else if !self.current().is_end() {
            return Err(error("Unexpected closing brace", &self.current()));
        }

        self.references = outer_references;
        self.shadowable = outer_shadowable;
        self.depth -= 1;
        Ok(statements)
    }

    fn statement(&mut self) -> Result<Statement, ParticleScriptError> {
        let token = self.current();
        if self.take("if") {
            self.expect("(")?;
            let condition = self.expression(0)?;
            require_kind(&condition, ParticleValueKind::Scalar, &token)?;
            self.expect(")")?;
            self.expect("{")?;
            let yes = self.block(true)?;
            let mut no = Vec::new();
            if self.take("else") {
                self.expect("{")?;
                no = self.block(true)?;
            }
            return Ok(Statement::If { condition, yes, no });
        }

        let declare = self.take("let");
        let name_token = self.current();
        let name = name_token.text;
        if !is_identifier(name) {
            return Err(error("Expected a field or local variable", &name_token));
        }
        self.cursor += 1;
        let already_defined = self.references.contains_key(name) && !self.shadowable.contains(name);
        if declare && (already_defined || ["let", "if", "else"].contains(&name)) {
            return Err(error(format!("Name '{name}' is already defined"), &name_token));
        }
        let target = if declare { None } else { Some(self.reference(&name_token)?) };
        if target.as_ref().is_some_and(|target| !target.writable) {
            return Err(error(format!("Field '{name}' is not writable"), &name_token));
        }

        let operator_token = self.current();
        let operator = match operator_token.text {
            "=" => None,
            "+=" => Some(BinaryOperator::Add),
            "-=" => Some(BinaryOperator::Subtract),
            "*=" => Some(BinaryOperator::Multiply),
            "/=" => Some(BinaryOperator::Divide),
            _ => return Err(error("Expected an assignment", &operator_token)),
        };
        if declare && operator.is_some() {
            return Err(error("Expected an assignment", &operator_token));
        }
        self.cursor += 1;

        let expression = self.expression(0)?;
        let target = match target {
            Some(target) => target,
            None => {
                let slots = if expression.kind == ParticleValueKind::Scalar {
                    vec![self.slot(name)]
                } else {
                    let components = expression.kind.component_names();
                    components.iter().map(|component| self.slot(&format!("{name}.{component}"))).collect()
                };
                Reference { kind: expression.kind, slots, writable: true }
            }
        };
        match operator {
            None => require_kind(&expression, target.kind, &token)?,
            Some(operator) => {
                let symbol = &operator_token.text[..1];
                let result = binary_kind(symbol, operator, target.kind, expression.kind, &token)?;
                if result != target.kind {
                    return Err(error("Assignment changes the value type", &token));
                }
            }
        }
        if declare {
            self.references.insert(name.to_owned(), target.clone());
            self.shadowable.remove(name);
        }
        Ok(Statement::Assign { location: token.location(), operator, expression, target })
    }

    /// Resolve a declared name and an optional valid component letter.
    fn reference(&mut self, token: &Token<'a>) -> Result<Reference, ParticleScriptError> {
        let Some(reference) = self.references.get(token.text).cloned() else {
            return Err(error(format!("Unknown numeric value '{}' or field is not writable", token.text), token));
        };
        if !self.take(".") {
            return Ok(reference);
        }
        let component = self.current();
        let Some(index) = reference.kind.component_names().iter().position(|name| *name == component.text) else {
            return Err(error(format!("Invalid {} component '{}'", reference.kind, component.text), &component));
        };
        self.cursor += 1;
        Ok(Reference {
            kind: ParticleValueKind::Scalar,
            slots: vec![reference.slots[index]],
            writable: reference.writable,
        })
    }

    /// Parse one precedence-climbing expression level; operands of `&&` and `||` stay lazy.
    fn expression(&mut self, minimum: u32) -> Result<Expr, ParticleScriptError> {
        self.depth += 1;
        if self.depth > MAXIMUM_EXPRESSION_DEPTH {
            return Err(error("Expression nesting limit exceeded", &self.current()));
        }
        let token = self.current();
        if token.is_end() {
            // TypeScript fails here with a host TypeError; Rust reports a located script error instead.
            return Err(error("Unexpected end of script", &token));
        }
        self.cursor += 1;

        let (mut node, mut kind) = match token.text {
            "-" | "+" | "!" => {
                let operand = self.expression(60)?;
                if token.text == "!" {
                    require_kind(&operand, ParticleValueKind::Scalar, &token)?;
                }
                let operator = match token.text {
                    "-" => UnaryOperator::Negate,
                    "+" => UnaryOperator::Plus,
                    _ => UnaryOperator::Not,
                };
                let kind = operand.kind;
                (Node::Unary { operator, operand: Box::new(operand) }, kind)
            }
            "(" => {
                let inner = self.expression(0)?;
                self.expect(")")?;
                let kind = inner.kind;
                (Node::Group(Box::new(inner)), kind)
            }
            text if starts_number(text) => {
                let value = parse_number(text);
                if !is_script_finite(value) {
                    return Err(error("Numeric result must be finite and within ±1 billion", &token));
                }
                (Node::Literal(value), ParticleValueKind::Scalar)
            }
            _ if self.take("(") => {
                let mut arguments = Vec::new();
                if !self.take(")") {
                    loop {
                        arguments.push(self.expression(0)?);
                        if !self.take(",") {
                            break;
                        }
                    }
                    self.expect(")")?;
                }
                self.function_expression(&token, arguments)?
            }
            _ => {
                let reference = self.reference(&token)?;
                let kind = reference.kind;
                (Node::Read(reference), kind)
            }
        };

        while let Some((operator, precedence)) = BinaryOperator::parse(self.current().text) {
            if precedence < minimum {
                break;
            }
            let operator_token = self.current();
            self.cursor += 1;
            let right_minimum = precedence + u32::from(operator != BinaryOperator::Power);
            let right = self.expression(right_minimum)?;
            kind = binary_kind(operator_token.text, operator, kind, right.kind, &operator_token)?;
            node = Node::Binary { operator, left: Box::new(node), right: Box::new(right) };
        }
        self.depth -= 1;
        Ok(Expr { location: token.location(), kind, node })
    }

    /// Type-check a call to a constructor (`vector`, `rgba`), component `mix`, or scalar built-in.
    fn function_expression(
        &mut self,
        token: &Token<'a>,
        arguments: Vec<Expr>,
    ) -> Result<(Node, ParticleValueKind), ParticleScriptError> {
        let name = token.text;
        if name == "vector" || name == "rgba" {
            let kind = if name == "vector" { ParticleValueKind::Vector } else { ParticleValueKind::Color };
            if !arities(name).contains(&arguments.len()) {
                return Err(error(format!("Wrong argument count for {name}"), token));
            }
            for argument in &arguments {
                require_kind(argument, ParticleValueKind::Scalar, token)?;
            }
            return Ok((Node::Construct(arguments), kind));
        }

        if name == "mix" && arguments.len() == 3 && arguments[0].kind != ParticleValueKind::Scalar {
            let kind = arguments[0].kind;
            require_kind(&arguments[1], kind, token)?;
            require_kind(&arguments[2], ParticleValueKind::Scalar, token)?;
            // Exactly three arguments were checked above, so the conversion cannot fail.
            let Ok(components) = <[Expr; 3]>::try_from(arguments) else {
                return Err(error(format!("Unknown function or argument count: {name}"), token));
            };
            return Ok((Node::MixComponents(Box::new(components)), kind));
        }

        let Some((function, _)) = Builtin::parse(name).filter(|(_, counts)| counts.contains(&arguments.len())) else {
            return Err(error(format!("Unknown function or argument count: {name}"), token));
        };
        for argument in &arguments {
            require_kind(argument, ParticleValueKind::Scalar, token)?;
        }
        Ok((Node::Call { function, arguments }, ParticleValueKind::Scalar))
    }
}

fn is_identifier(name: &str) -> bool {
    let mut characters = name.chars();
    characters.next().is_some_and(|first| first.is_ascii_alphabetic() || first == '_')
        && characters.all(|character| character.is_ascii_alphanumeric() || character == '_')
}

fn starts_number(text: &str) -> bool {
    let bytes = text.as_bytes();
    bytes.first().is_some_and(u8::is_ascii_digit)
        || (bytes.first() == Some(&b'.') && bytes.get(1).is_some_and(u8::is_ascii_digit))
}

fn require_kind(expression: &Expr, kind: ParticleValueKind, token: &Token<'_>) -> Result<(), ParticleScriptError> {
    if expression.kind != kind {
        return Err(error(format!("Expected {kind}, found {}", expression.kind), token));
    }
    Ok(())
}

/// Result kind of an infix operator: scalars combine freely, like kinds add and subtract, and vectors and colors scale
/// by scalars.
fn binary_kind(
    symbol: &str,
    operator: BinaryOperator,
    left: ParticleValueKind,
    right: ParticleValueKind,
    token: &Token<'_>,
) -> Result<ParticleValueKind, ParticleScriptError> {
    use BinaryOperator::{
        Add,
        Divide,
        Multiply,
        Subtract,
    };
    if left == ParticleValueKind::Scalar && right == ParticleValueKind::Scalar {
        return Ok(ParticleValueKind::Scalar);
    }
    if matches!(operator, Add | Subtract) && left == right {
        return Ok(left);
    }
    if matches!(operator, Multiply | Divide) && right == ParticleValueKind::Scalar {
        return Ok(left);
    }
    if operator == Multiply && left == ParticleValueKind::Scalar {
        return Ok(right);
    }
    Err(error(format!("Operator '{symbol}' does not accept {left} and {right}"), token))
}

fn is_script_finite(value: f64) -> bool {
    value.is_finite() && value.abs() <= MAXIMUM_SCRIPT_MAGNITUDE
}

// ---------------------------------------------------------------------------------------------------------------------
// Evaluation
// ---------------------------------------------------------------------------------------------------------------------

/// A scalar, vector, or color value: `length` components of `components`.
#[derive(Clone, Copy, Debug)]
struct Value {
    length: usize,
    components: [f64; 4],
}

impl Value {
    fn scalar(value: f64) -> Self {
        Self { length: 1, components: [value, 0.0, 0.0, 0.0] }
    }

    fn first(&self) -> f64 {
        self.components[0]
    }

    fn slice(&self) -> &[f64] {
        &self.components[..self.length]
    }
}

/// Borrowed evaluation state for one run.
struct Machine<'r> {
    slots: &'r mut [f64],
    random: &'r mut dyn RandomSource,
    budget: &'r mut i64,
}

impl Machine<'_> {
    fn read(&self, reference: &Reference) -> Value {
        let read = |slot: usize| self.slots.get(slot).copied().unwrap_or(f64::NAN);
        if reference.kind == ParticleValueKind::Scalar {
            return Value::scalar(read(reference.slots[0]));
        }
        let mut value = Value { length: reference.slots.len().min(4), components: [0.0; 4] };
        for (component, slot) in value.components.iter_mut().zip(&reference.slots) {
            *component = read(*slot);
        }
        value
    }
}

fn run_statements(statements: &[Statement], machine: &mut Machine<'_>) -> Result<(), ParticleScriptError> {
    for statement in statements {
        match statement {
            Statement::If { condition, yes, no } => {
                let value = evaluate(condition, machine)?;
                run_statements(if is_true(value.first()) { yes } else { no }, machine)?;
            }
            Statement::Assign { location, operator, expression, target } => {
                // Evaluate first so swaps and self-references cannot observe partial writes.
                let value = evaluate(expression, machine)?;
                let result = match operator {
                    None => value,
                    Some(operator) => calculate(*operator, machine.read(target), value),
                };
                for (component, slot) in result.slice().iter().zip(&target.slots) {
                    if !is_script_finite(*component) {
                        return Err(located_error("Numeric result must be finite and within ±1 billion", *location));
                    }
                    if let Some(storage) = machine.slots.get_mut(*slot) {
                        *storage = *component;
                    }
                }
            }
        }
    }
    Ok(())
}

/// Evaluate one expression level: charge its cost, evaluate, and require finite components.
fn evaluate(expression: &Expr, machine: &mut Machine<'_>) -> Result<Value, ParticleScriptError> {
    *machine.budget -= expression.kind.cost();
    if *machine.budget < 0 {
        return Err(located_error("Effect instruction budget exceeded", expression.location));
    }
    let value = evaluate_node(&expression.node, machine)?;
    if value.slice().iter().any(|component| !is_script_finite(*component)) {
        return Err(located_error("Numeric result must be finite and within ±1 billion", expression.location));
    }
    Ok(value)
}

fn evaluate_node(node: &Node, machine: &mut Machine<'_>) -> Result<Value, ParticleScriptError> {
    Ok(match node {
        Node::Literal(value) => Value::scalar(*value),
        Node::Read(reference) => machine.read(reference),
        Node::Unary { operator, operand } => {
            let value = evaluate(operand, machine)?;
            match operator {
                UnaryOperator::Negate => calculate(BinaryOperator::Multiply, value, Value::scalar(-1.0)),
                UnaryOperator::Plus => value,
                UnaryOperator::Not => Value::scalar(if is_true(value.first()) { 0.0 } else { 1.0 }),
            }
        }
        Node::Group(inner) => evaluate(inner, machine)?,
        Node::Construct(arguments) => {
            let mut value = Value { length: arguments.len().min(4), components: [0.0; 4] };
            for (component, argument) in value.components.iter_mut().zip(arguments) {
                *component = evaluate(argument, machine)?.first();
            }
            value
        }
        Node::MixComponents(arguments) => {
            let [a, b, t] = arguments.as_ref();
            let a = evaluate(a, machine)?;
            let b = evaluate(b, machine)?;
            let t = evaluate(t, machine)?;
            let difference = calculate(BinaryOperator::Subtract, b, a);
            calculate(BinaryOperator::Add, a, calculate(BinaryOperator::Multiply, difference, t))
        }
        Node::Call { function, arguments } => {
            let mut values = [0.0; 3];
            for (value, argument) in values.iter_mut().zip(arguments) {
                *value = evaluate(argument, machine)?.first();
            }
            Value::scalar(function.call(&values[..arguments.len().min(3)], machine.random))
        }
        Node::Binary { operator, left, right } => {
            let value = evaluate_node(left, machine)?;
            match operator {
                BinaryOperator::And => {
                    if is_true(value.first()) {
                        let right = evaluate(right, machine)?;
                        Value::scalar(if is_true(right.first()) { 1.0 } else { 0.0 })
                    } else {
                        Value::scalar(0.0)
                    }
                }
                BinaryOperator::Or => {
                    if is_true(value.first()) {
                        Value::scalar(1.0)
                    } else {
                        let right = evaluate(right, machine)?;
                        Value::scalar(if is_true(right.first()) { 1.0 } else { 0.0 })
                    }
                }
                _ => {
                    let right = evaluate(right, machine)?;
                    calculate(*operator, value, right)
                }
            }
        }
    })
}

/// Component-wise arithmetic on copied tuples; a scalar operand broadcasts across the other's components.
fn calculate(operator: BinaryOperator, left: Value, right: Value) -> Value {
    if left.length > 1 {
        let mut result = left;
        for index in 0..left.length {
            let other = if right.length > 1 { right.components[index] } else { right.first() };
            result.components[index] = scalar_operation(operator, left.components[index], other);
        }
        return result;
    }
    if right.length > 1 {
        let mut result = right;
        for index in 0..right.length {
            result.components[index] = scalar_operation(operator, left.first(), right.components[index]);
        }
        return result;
    }
    Value::scalar(scalar_operation(operator, left.first(), right.first()))
}

fn scalar_operation(operator: BinaryOperator, left: f64, right: f64) -> f64 {
    let boolean = |value: bool| if value { 1.0 } else { 0.0 };
    match operator {
        BinaryOperator::Add => left + right,
        BinaryOperator::Subtract => left - right,
        BinaryOperator::Multiply => left * right,
        BinaryOperator::Divide => left / right,
        BinaryOperator::Remainder => left % right,
        BinaryOperator::Power => deterministic::powf(left, right),
        BinaryOperator::Less => boolean(left < right),
        BinaryOperator::Greater => boolean(left > right),
        BinaryOperator::LessEqual => boolean(left <= right),
        BinaryOperator::GreaterEqual => boolean(left >= right),
        BinaryOperator::Equal => boolean(left == right),
        BinaryOperator::NotEqual => boolean(left != right),
        // Logical operators short-circuit before reaching component arithmetic.
        BinaryOperator::And => boolean(is_true(left) && is_true(right)),
        BinaryOperator::Or => boolean(is_true(left) || is_true(right)),
    }
}

#[cfg(test)]
#[path = "../tests/unit/script.rs"]
mod tests;
