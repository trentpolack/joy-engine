// Copyright (c) 2026 Trent Polack. All Rights Reserved.
// Licensed under the MIT License.

//! Recursive-descent parser for FORM source.
//!
//! The parser builds syntax only; it never executes authored code. `use` declarations are recorded with a placeholder
//! module index that the module graph ([`crate::modules`]) fills in once it has loaded each dependency.

use crate::Diagnostic;
use crate::ast::{
    AssignmentOperator,
    ConditionalBranch,
    ExprKind,
    Expression,
    FunctionDefinition,
    FunctionParameter,
    MatchCase,
    ParameterPresentation,
    Statement,
    StmtKind,
    Token,
};
use crate::tokenizer::{
    END_TOKEN,
    is_name,
    starts_number,
    tokenize,
};
use std::collections::HashSet;
use std::rc::Rc;

const BLOCK_DEPTH_LIMIT: usize = 48;
const EXPRESSION_DEPTH_LIMIT: usize = 64;

// Parameter `meta(...)` keys: numeric bounds the evaluator reads, then presentation text editors show.
pub(crate) const PARAMETER_MINIMUM: &str = "min";
pub(crate) const PARAMETER_MAXIMUM: &str = "max";
pub(crate) const PARAMETER_STEP: &str = "step";
const PARAMETER_LABEL: &str = "label";
const PARAMETER_GROUP: &str = "group";
const PARAMETER_INFO: &str = "info";
const PARAMETER_KIND: &str = "kind";
const PARAMETER_OPTIONS: &str = "options";
const PARAMETER_METADATA_KEYS: [&str; 8] = [
    PARAMETER_MINIMUM,
    PARAMETER_MAXIMUM,
    PARAMETER_STEP,
    PARAMETER_LABEL,
    PARAMETER_GROUP,
    PARAMETER_INFO,
    PARAMETER_KIND,
    PARAMETER_OPTIONS,
];

// Presentation text limits, in characters, so inspectors can lay parameters out predictably.
const PARAMETER_LABEL_LIMIT: usize = 48;
const PARAMETER_GROUP_LIMIT: usize = 32;
const PARAMETER_INFO_LIMIT: usize = 160;
const PARAMETER_KIND_LIMIT: usize = 16;
const PARAMETER_OPTION_LIMIT: usize = 32;
const PARAMETER_OPTION_COUNT_LIMIT: usize = 32;
const INVALID_DATA_KEY: &str = "Invalid record field name (1–64 characters; prototype keys are reserved).";

/// Most members one `enum` may declare, matching the option limit editors lay out for enum parameters.
pub(crate) const ENUM_MEMBER_LIMIT: usize = PARAMETER_OPTION_COUNT_LIMIT;

/// Record field names FORM rejects. Exported records land in glTF `extras`, and these names would alter object
/// prototypes in the JavaScript viewers and pipelines that read those files.
pub(crate) const RESERVED_DATA_KEYS: [&str; 3] = ["__proto__", "prototype", "constructor"];

// Logical operators are written in capitals so they read as keywords beside lowercase names.
pub(crate) const OPERATOR_AND: &str = "AND";
pub(crate) const OPERATOR_OR: &str = "OR";
pub(crate) const OPERATOR_NOT: &str = "NOT";

/// Words the grammar owns. They cannot name variables, parameters, functions, enums, members, or modules.
pub(crate) const RESERVED_WORDS: [&str; 26] = [
    "project",
    "param",
    "meta",
    "let",
    "const",
    "if",
    "else",
    "match",
    "case",
    "repeat",
    "for",
    "foreach",
    "as",
    "wrangle",
    "in",
    "fn",
    "return",
    "ref",
    "enum",
    "use",
    OPERATOR_AND,
    OPERATOR_OR,
    OPERATOR_NOT,
    "true",
    "false",
    "null",
];

/// Parse FORM without executing authored code. Locations are one-based lines and
/// character columns, so editors place diagnostics directly.
pub(crate) fn parse(source: &str) -> Result<Vec<Statement>, Diagnostic> {
    Parser {
        tokens: tokenize(source)?,
        cursor: 0,
        depth: 0,
        in_function: false,
    }
    .block(false)
}

struct Parser {
    tokens: Vec<Token>,
    cursor: usize,
    // Blocks and expressions deliberately share the original parser's budget.
    depth: usize,
    // `return` is only meaningful inside a function body.
    in_function: bool,
}

impl Parser {
    fn current(&self) -> &Token {
        &self.tokens[self.cursor]
    }

    fn peek(&self, offset: usize) -> Option<&str> {
        self.tokens.get(self.cursor + offset).map(|token| token.text.as_str())
    }

    fn take(&mut self, text: &str) -> bool {
        if self.current().text != text {
            return false;
        }
        self.cursor += 1;
        true
    }

    fn expect(&mut self, text: &str) -> Result<(), Diagnostic> {
        if self.take(text) {
            return Ok(());
        }
        Err(Diagnostic::at(format!("Expected '{text}', found '{}'.", self.current().text), self.current()))
    }

    /// Any identifier, including grammar words: metadata keys, record keys, and member names after `.`.
    fn name(&mut self) -> Result<String, Diagnostic> {
        if !is_name(&self.current().text) {
            return Err(Diagnostic::at("Expected a variable name.", self.current()));
        }
        let name = self.current().text.clone();
        self.cursor += 1;
        Ok(name)
    }

    /// A name the script declares (a variable, parameter, function, enum, member, or module alias).
    fn declared_name(&mut self) -> Result<String, Diagnostic> {
        let token = self.current().clone();
        let name = self.name()?;
        if RESERVED_WORDS.contains(&name.as_str()) {
            return Err(Diagnostic::at(format!("'{name}' is a reserved word and cannot be used as a name."), &token));
        }
        Ok(name)
    }

    fn block(&mut self, nested: bool) -> Result<Vec<Statement>, Diagnostic> {
        self.depth += 1;
        if self.depth > BLOCK_DEPTH_LIMIT {
            return Err(Diagnostic::at("Nesting limit exceeded.", self.current()));
        }
        let mut statements = Vec::new();
        while self.current().text != END_TOKEN && self.current().text != "}" {
            if self.take(";") {
                continue;
            }
            statements.push(self.statement(nested)?);
        }
        if nested {
            self.expect("}")?;
        } else if self.current().text == "}" {
            return Err(Diagnostic::at("Unexpected '}'.", self.current()));
        }
        self.depth -= 1;
        Ok(statements)
    }

    /// A `{ … }` block; the opening brace has not been consumed yet.
    fn braced_block(&mut self) -> Result<Vec<Statement>, Diagnostic> {
        self.expect("{")?;
        self.block(true)
    }

    fn statement(&mut self, nested: bool) -> Result<Statement, Diagnostic> {
        let token = self.current().clone();
        let kind = match token.text.as_str() {
            "project" => {
                self.cursor += 1;
                return self.project(token);
            }
            "param" => {
                self.cursor += 1;
                return self.parameter(token);
            }
            "with" if self.peek(1) == Some("userData") => {
                return Err(Diagnostic::at(
                    "'with userData' blocks were removed. Store per-point values as attributes with addAttribute() and setAttribute().",
                    &token,
                ));
            }
            "wrangle" => {
                self.cursor += 1;
                let domain = self.name()?;
                self.expect("in")?;
                let expression = self.expression(0)?;
                self.expect("as")?;
                let binding = self.declared_name()?;
                StmtKind::Wrangle {
                    domain,
                    expression,
                    binding,
                    body: self.braced_block()?,
                }
            }
            "if" => {
                self.cursor += 1;
                self.conditional()?
            }
            "else" => return Err(Diagnostic::at("'else' must follow an if block or a match case.", &token)),
            "match" => {
                self.cursor += 1;
                self.match_statement()?
            }
            "let" | "const" => {
                self.cursor += 1;
                let name = self.declared_name()?;
                self.expect("=")?;
                let expression = self.expression(0)?;
                if token.text == "const" { StmtKind::Const(name, expression) } else { StmtKind::Let(name, expression) }
            }
            "repeat" => {
                return Err(Diagnostic::at(
                    "'repeat count as index' was removed; use 'for index in 0..count' instead.",
                    &token,
                ));
            }
            "for" => {
                self.cursor += 1;
                let name = self.declared_name()?;
                self.expect("in")?;
                let start = self.expression(0)?;
                if !self.take("..") {
                    return Err(Diagnostic::at(
                        "for requires a range, 'for index in start..end'; use 'foreach item in collection' for values.",
                        self.current(),
                    ));
                }
                if self.current().text == "=" {
                    return Err(Diagnostic::at(
                        "for ranges use an exclusive end: write start..end, not start..=end.",
                        self.current(),
                    ));
                }
                let end = self.expression(0)?;
                StmtKind::For {
                    name,
                    start,
                    end,
                    body: self.braced_block()?,
                }
            }
            "foreach" => {
                self.cursor += 1;
                let name = self.declared_name()?;
                self.expect("in")?;
                let collection = self.expression(0)?;
                if self.current().text == ".." {
                    return Err(Diagnostic::at(
                        "foreach visits collection values; use 'for index in start..end' for a range.",
                        self.current(),
                    ));
                }
                StmtKind::ForEach {
                    name,
                    collection,
                    body: self.braced_block()?,
                }
            }
            "fn" => {
                if nested || self.in_function {
                    return Err(Diagnostic::at("Declare functions at the top level of a script.", &token));
                }
                self.cursor += 1;
                self.function()?
            }
            "return" => {
                if !self.in_function {
                    return Err(Diagnostic::at("'return' is only allowed inside a function.", &token));
                }
                self.cursor += 1;
                let value =
                    if matches!(self.current().text.as_str(), "}" | ";") { None } else { Some(self.expression(0)?) };
                StmtKind::Return(value)
            }
            "enum" => {
                if nested || self.in_function {
                    return Err(Diagnostic::at("Declare enums at the top level of a script.", &token));
                }
                self.cursor += 1;
                self.enumeration()?
            }
            "use" => {
                if nested || self.in_function {
                    return Err(Diagnostic::at("Declare modules with 'use' at the top level of a script.", &token));
                }
                self.cursor += 1;
                let path_token = self.current().clone();
                let path: String = serde_json::from_str(&path_token.text)
                    .map_err(|_| Diagnostic::at("'use' requires a double-quoted module path.", &path_token))?;
                self.cursor += 1;
                self.expect("as")?;
                StmtKind::Use {
                    path,
                    alias: self.declared_name()?,
                    module: usize::MAX,
                }
            }
            _ => self.simple_statement()?,
        };
        Ok(Statement {
            kind,
            token,
        })
    }

    /// Assignment to a variable or a wrangle element's attribute, or a call made for its effect.
    fn simple_statement(&mut self) -> Result<StmtKind, Diagnostic> {
        let token = self.current().clone();
        if is_name(&token.text) && !RESERVED_WORDS.contains(&token.text.as_str()) {
            if let Some(operator) = self.peek(1).and_then(assignment_operator) {
                let name = self.declared_name()?;
                self.cursor += 1;
                return Ok(StmtKind::Assign {
                    name,
                    operator,
                    expression: self.expression(0)?,
                });
            }
            if self.peek(1) == Some(".")
                && let Some(operator) = self.peek(3).and_then(assignment_operator)
            {
                let binding = self.name()?;
                self.expect(".")?;
                let name = self.name()?;
                self.cursor += 1;
                return Ok(StmtKind::BindingAttribute {
                    binding,
                    name,
                    operator,
                    expression: self.expression(0)?,
                });
            }
        }
        let expression = self.expression(0)?;
        if !matches!(expression.kind, ExprKind::Call(..) | ExprKind::ModuleCall(..)) {
            return Err(Diagnostic::at("Expected a statement; a value on its own must be a function call.", &token));
        }
        Ok(StmtKind::Command(expression))
    }

    /// `if condition { … } else if condition { … } else { … }`; the `if` keyword has been consumed.
    fn conditional(&mut self) -> Result<StmtKind, Diagnostic> {
        let mut branches = vec![ConditionalBranch {
            condition: self.expression(0)?,
            body: self.braced_block()?,
        }];
        let mut otherwise = None;
        while self.take("else") {
            if self.take("if") {
                branches.push(ConditionalBranch {
                    condition: self.expression(0)?,
                    body: self.braced_block()?,
                });
                continue;
            }
            otherwise = Some(self.braced_block()?);
            break;
        }
        Ok(StmtKind::If {
            branches,
            otherwise,
        })
    }

    /// `match subject { case a, b { … } else { … } }`; the `match` keyword has been consumed.
    fn match_statement(&mut self) -> Result<StmtKind, Diagnostic> {
        let subject = self.expression(0)?;
        self.expect("{")?;
        let mut cases = Vec::new();
        let mut otherwise = None;
        loop {
            if self.take("}") {
                break;
            }
            if otherwise.is_some() {
                return Err(Diagnostic::at("A match's else block must come last.", self.current()));
            }
            if self.take("else") {
                otherwise = Some(self.braced_block()?);
                continue;
            }
            if !self.take("case") {
                return Err(Diagnostic::at(
                    format!("Expected 'case', 'else', or '}}' in match, found '{}'.", self.current().text),
                    self.current(),
                ));
            }
            let mut values = vec![self.expression(0)?];
            while self.take(",") {
                values.push(self.expression(0)?);
            }
            cases.push(MatchCase {
                values,
                body: self.braced_block()?,
            });
        }
        if cases.is_empty() {
            return Err(Diagnostic::at("A match requires at least one case.", &subject.token));
        }
        Ok(StmtKind::Match {
            subject,
            cases,
            otherwise,
        })
    }

    /// `fn name(a, ref b) { … }`; the `fn` keyword has been consumed.
    fn function(&mut self) -> Result<StmtKind, Diagnostic> {
        let name = self.declared_name()?;
        self.expect("(")?;
        let mut parameters: Vec<FunctionParameter> = Vec::new();
        if !self.take(")") {
            loop {
                let by_reference = self.take("ref");
                let parameter_token = self.current().clone();
                let parameter = self.declared_name()?;
                if parameters.iter().any(|existing| existing.name == parameter) {
                    return Err(Diagnostic::at(
                        format!("Duplicate function parameter '{parameter}'."),
                        &parameter_token,
                    ));
                }
                parameters.push(FunctionParameter {
                    name: parameter,
                    by_reference,
                });
                if !self.take(",") {
                    break;
                }
            }
            self.expect(")")?;
        }
        self.in_function = true;
        let body = self.braced_block();
        self.in_function = false;
        Ok(StmtKind::Function(Rc::new(FunctionDefinition {
            name,
            parameters,
            body: body?,
        })))
    }

    /// `enum Name { a, b, c }`; the `enum` keyword has been consumed.
    fn enumeration(&mut self) -> Result<StmtKind, Diagnostic> {
        let name = self.declared_name()?;
        let open = self.current().clone();
        self.expect("{")?;
        let mut members: Vec<String> = Vec::new();
        while !self.take("}") {
            let member_token = self.current().clone();
            let member = self.declared_name()?;
            if members.contains(&member) {
                return Err(Diagnostic::at(format!("Duplicate enum member '{member}'."), &member_token));
            }
            members.push(member);
            if !self.take(",") {
                self.expect("}")?;
                break;
            }
        }
        if members.is_empty() || members.len() > ENUM_MEMBER_LIMIT {
            return Err(Diagnostic::at(format!("An enum requires 1 to {ENUM_MEMBER_LIMIT} members."), &open));
        }
        Ok(StmtKind::Enum {
            name,
            members,
        })
    }

    fn parameter(&mut self, token: Token) -> Result<Statement, Diagnostic> {
        let name = self.declared_name()?;
        self.expect("=")?;
        let mut values = vec![self.expression(0)?];
        let mut metadata: Vec<(String, Expression)> = Vec::new();
        let mut presentation = ParameterPresentation::default();
        if self.take(",") {
            // The legacy form always has exactly four positional expressions.
            values.push(self.expression(0)?);
            for _ in 0..2 {
                self.expect(",")?;
                values.push(self.expression(0)?);
            }
        } else if self.take("meta") {
            self.expect("(")?;
            if !self.take(")") {
                let mut seen: Vec<String> = Vec::new();
                loop {
                    let key = self.name()?;
                    if !PARAMETER_METADATA_KEYS.contains(&key.as_str()) {
                        return Err(Diagnostic::at(format!("Unknown parameter metadata '{key}'."), self.current()));
                    }
                    if seen.contains(&key) {
                        return Err(Diagnostic::at(format!("Duplicate metadata '{key}'."), self.current()));
                    }
                    seen.push(key.clone());
                    self.expect("=")?;
                    match key.as_str() {
                        PARAMETER_MINIMUM | PARAMETER_MAXIMUM | PARAMETER_STEP => {
                            metadata.push((key.clone(), self.expression(0)?));
                        }
                        PARAMETER_LABEL => presentation.label = Some(self.metadata_text(PARAMETER_LABEL_LIMIT)?),
                        PARAMETER_GROUP => presentation.group = Some(self.metadata_text(PARAMETER_GROUP_LIMIT)?),
                        PARAMETER_INFO => presentation.info = Some(self.metadata_text(PARAMETER_INFO_LIMIT)?),
                        PARAMETER_KIND => presentation.kind = Some(self.metadata_text(PARAMETER_KIND_LIMIT)?),
                        _ => presentation.options = self.metadata_options()?,
                    }
                    if !self.take(",") {
                        break;
                    }
                }
                self.expect(")")?;
            }
        }
        Ok(Statement {
            kind: StmtKind::Param {
                name,
                values,
                metadata,
                presentation,
            },
            token,
        })
    }

    /// One double-quoted, single-line metadata text of at most `limit` characters.
    fn metadata_text(&mut self, limit: usize) -> Result<String, Diagnostic> {
        let token = self.current().clone();
        let text: String = serde_json::from_str(&token.text)
            .map_err(|_| Diagnostic::at("Parameter metadata text requires double quotes.", &token))?;
        if text.chars().count() > limit || text.contains(['\r', '\n']) {
            return Err(Diagnostic::at(
                format!("Parameter metadata text must be one line of at most {limit} characters."),
                &token,
            ));
        }
        self.cursor += 1;
        Ok(text)
    }

    /// A bracketed list of option labels, such as `["Low", "Medium", "High"]`: at least two, at most the option limit.
    fn metadata_options(&mut self) -> Result<Vec<String>, Diagnostic> {
        let token = self.current().clone();
        self.expect("[")?;
        let mut options = Vec::new();
        if !self.take("]") {
            loop {
                options.push(self.metadata_text(PARAMETER_OPTION_LIMIT)?);
                if !self.take(",") {
                    break;
                }
            }
            self.expect("]")?;
        }
        if options.len() < 2 || options.len() > PARAMETER_OPTION_COUNT_LIMIT {
            return Err(Diagnostic::at(
                format!("Parameter options require 2 to {PARAMETER_OPTION_COUNT_LIMIT} labels."),
                &token,
            ));
        }
        Ok(options)
    }

    fn project(&mut self, token: Token) -> Result<Statement, Diagnostic> {
        self.expect("meta")?;
        self.expect("(")?;
        let mut fields = serde_json::Map::new();
        let mut name_token = None;
        loop {
            let key_token = self.current().clone();
            let key = self.name()?;
            if !matches!(key.as_str(), "name" | "info" | "title") {
                return Err(Diagnostic::at(format!("Unknown project metadata '{key}'."), &key_token));
            }
            if fields.contains_key(&key) {
                return Err(Diagnostic::at(format!("Duplicate project metadata '{key}'."), &key_token));
            }
            self.expect("=")?;
            let value_token = self.current();
            let value: String = serde_json::from_str(&value_token.text)
                .map_err(|_| Diagnostic::at("Project metadata requires double-quoted text.", value_token))?;
            let text_limit = if key == "info" { 160 } else { 120 };
            if value.chars().count() > text_limit {
                return Err(Diagnostic::at("Project metadata text is too long.", value_token));
            }
            if key == "name" && (value.trim().is_empty() || value.contains(['\r', '\n'])) {
                return Err(Diagnostic::at("Project name requires nonempty, single-line text.", value_token));
            }
            if key == "name" {
                name_token = Some(value_token.clone());
            }
            fields.insert(key, serde_json::Value::String(value));
            self.cursor += 1;
            if !self.take(",") {
                break;
            }
        }
        self.expect(")")?;
        let Some(name) = name_token else {
            return Err(Diagnostic::at("Project metadata requires a name.", &token));
        };
        Ok(Statement {
            kind: StmtKind::Project {
                fields: serde_json::Value::Object(fields),
                name,
            },
            token,
        })
    }

    fn expression(&mut self, minimum: i32) -> Result<Expression, Diagnostic> {
        self.depth += 1;
        if self.depth > EXPRESSION_DEPTH_LIMIT {
            return Err(Diagnostic::at("Expression nesting limit exceeded.", self.current()));
        }
        let token = self.current().clone();
        self.cursor += 1;
        let mut left = match token.text.as_str() {
            "{" => Expression {
                kind: ExprKind::Record(self.record_fields()?),
                token: token.clone(),
            },
            "[" => Expression {
                kind: ExprKind::Array(self.arguments("]", false)?),
                token: token.clone(),
            },
            "true" | "false" => Expression {
                kind: ExprKind::Bool(token.text == "true"),
                token: token.clone(),
            },
            "null" => Expression {
                kind: ExprKind::Null,
                token: token.clone(),
            },
            "-" | "+" => Expression {
                kind: ExprKind::Unary(token.text.clone(), Box::new(self.expression(30)?)),
                token: token.clone(),
            },
            // NOT binds tighter than AND and OR but looser than comparisons: `NOT a < b` negates `a < b`.
            OPERATOR_NOT => Expression {
                kind: ExprKind::Unary(token.text.clone(), Box::new(self.expression(3)?)),
                token: token.clone(),
            },
            "(" => {
                let expression = self.expression(0)?;
                self.expect(")")?;
                expression
            }
            text if text.starts_with('"') => {
                let text = serde_json::from_str(text).map_err(|_| Diagnostic::at("Invalid quoted text.", &token))?;
                Expression {
                    kind: ExprKind::Text(text),
                    token: token.clone(),
                }
            }
            text if starts_number(text.as_bytes()) => Expression {
                // Tokenization already enforces the decimal/exponent grammar.
                kind: ExprKind::Number(text.parse().unwrap_or(f64::NAN)),
                token: token.clone(),
            },
            text if is_name(text) && !RESERVED_WORDS.contains(&text) => {
                let kind = if self.take("(") {
                    ExprKind::Call(text.to_owned(), self.arguments(")", true)?)
                } else {
                    ExprKind::Variable(text.to_owned())
                };
                Expression {
                    kind,
                    token: token.clone(),
                }
            }
            _ => {
                return Err(Diagnostic::at(format!("Expected an expression, found '{}'.", token.text), &token));
            }
        };

        // Postfix chains: `.member`, `.function(arguments)` on modules, and `[index]`.
        loop {
            if self.take(".") {
                let member = self.name()?;
                let kind = if self.take("(") {
                    ExprKind::ModuleCall(Box::new(left), member, self.arguments(")", true)?)
                } else {
                    ExprKind::Component(Box::new(left), member)
                };
                left = Expression {
                    kind,
                    // A component uses the start token, including an opening parenthesis.
                    token: token.clone(),
                };
                continue;
            }
            if self.current().text == "[" {
                let bracket = self.current().clone();
                self.cursor += 1;
                let index = self.expression(0)?;
                self.expect("]")?;
                left = Expression {
                    kind: ExprKind::Index(Box::new(left), Box::new(index)),
                    token: bracket,
                };
                continue;
            }
            break;
        }

        while precedence(&self.current().text) >= minimum {
            let operator = self.current().clone();
            self.cursor += 1;
            let next_minimum = precedence(&operator.text) + if operator.text == "^" { 0 } else { 1 };
            let right = self.expression(next_minimum)?;
            left = Expression {
                kind: ExprKind::Binary(operator.text.clone(), Box::new(left), Box::new(right)),
                token: operator,
            };
        }
        self.depth -= 1;
        Ok(left)
    }

    /// Comma-separated expressions up to `closing`. Call arguments may pass `ref name`.
    fn arguments(&mut self, closing: &str, allow_references: bool) -> Result<Vec<Expression>, Diagnostic> {
        let mut arguments = Vec::new();
        if !self.take(closing) {
            loop {
                if allow_references && self.current().text == "ref" {
                    let token = self.current().clone();
                    self.cursor += 1;
                    arguments.push(Expression {
                        kind: ExprKind::Reference(self.declared_name()?),
                        token,
                    });
                } else {
                    arguments.push(self.expression(0)?);
                }
                if !self.take(",") {
                    break;
                }
            }
            self.expect(closing)?;
        }
        Ok(arguments)
    }

    fn record_fields(&mut self) -> Result<Vec<(String, Expression)>, Diagnostic> {
        let mut fields: Vec<(String, Expression)> = Vec::new();
        let mut names = HashSet::new();
        if self.take("}") {
            return Ok(fields);
        }
        loop {
            let key_token = self.current().clone();
            let key: String = if key_token.text.starts_with('"') {
                let key = serde_json::from_str(&key_token.text)
                    .map_err(|error| Diagnostic::at(quoted_key_error(&key_token.text, error), &key_token))?;
                self.cursor += 1;
                key
            } else {
                self.name()?
            };
            if key.is_empty() || key.chars().count() > 64 || RESERVED_DATA_KEYS.contains(&key.as_str()) {
                return Err(Diagnostic::at(INVALID_DATA_KEY, &key_token));
            }
            if !names.insert(key.clone()) {
                return Err(Diagnostic::at(format!("Duplicate record field '{key}'."), &key_token));
            }
            self.expect(":")?;
            fields.push((key, self.expression(0)?));
            if !self.take(",") {
                break;
            }
        }
        self.expect("}")?;
        // Fields evaluate, and are stored, in authored order.
        Ok(fields)
    }
}

/// Describe a malformed quoted record key, naming the offending character's one-based
/// position within the key token. Ordinary string expressions have their own diagnostic.
fn quoted_key_error(text: &str, error: serde_json::Error) -> String {
    let diagnostic = |description: &str, byte_offset: usize| {
        let position = text[..byte_offset].chars().count() + 1;
        format!("{description} in quoted key at character {position}.")
    };
    let mut characters = text.char_indices().skip(1);
    while let Some((offset, character)) = characters.next() {
        if character <= '\u{001f}' {
            return diagnostic("Bad control character in string literal", offset);
        }
        if character != '\\' {
            continue;
        }
        let Some((escape_offset, escape)) = characters.next() else {
            break;
        };
        match escape {
            '"' | '\\' | '/' | 'b' | 'f' | 'n' | 'r' | 't' => {}
            'u' => {
                for _ in 0..4 {
                    let Some((digit_offset, digit)) = characters.next() else {
                        return diagnostic("Bad Unicode escape", text.len());
                    };
                    if !digit.is_ascii_hexdigit() {
                        return diagnostic("Bad Unicode escape", digit_offset);
                    }
                }
            }
            _ => return diagnostic("Bad escaped character", escape_offset),
        }
    }
    // A `\u` escape naming a lone surrogate has no character to stand for. Reject it
    // explicitly instead of silently replacing authored data with U+FFFD.
    format!("Invalid quoted key: {error}.")
}

/// Binary operator precedence; higher binds tighter. Logical operators bind loosest so comparisons group first.
fn precedence(operator: &str) -> i32 {
    match operator {
        OPERATOR_OR => 1,
        OPERATOR_AND => 2,
        "==" | "!=" | "<" | ">" | "<=" | ">=" => 5,
        "+" | "-" => 10,
        "*" | "/" | "%" => 20,
        "^" => 40,
        _ => -1,
    }
}

/// The assignment a statement token spells, if any.
fn assignment_operator(text: &str) -> Option<AssignmentOperator> {
    match text {
        "=" => Some(AssignmentOperator::Set),
        "+=" => Some(AssignmentOperator::Add),
        "-=" => Some(AssignmentOperator::Subtract),
        "*=" => Some(AssignmentOperator::Multiply),
        "/=" => Some(AssignmentOperator::Divide),
        _ => None,
    }
}
