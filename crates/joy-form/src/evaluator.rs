// Copyright (c) 2026 Trent Polack. All Rights Reserved.
// Licensed under the MIT License.

//! Statement and expression evaluation for one script run.
//!
//! Evaluation walks the main script after initializing each module it uses. Declarations (`fn`, `enum`, and `use`)
//! are hoisted per module, so they may appear anywhere at the top level. Blocks push scope frames; functions run in
//! their own frames above their module's script-level bindings.

use crate::{
    Diagnostic,
    GeometryData,
    ast::{
        ExprKind,
        Expression,
        FunctionDefinition,
        Statement,
        StmtKind,
        Token,
    },
    builder::Builder,
    language_information::is_builtin,
    math,
    modules::Program,
    parameters,
    parser::{
        OPERATOR_AND,
        OPERATOR_NOT,
        OPERATOR_OR,
    },
    procedural::Procedural,
    records,
    resources,
    scope::{
        Binding,
        BindingRole,
        Frame,
        Globals,
        Scope,
    },
    value::{
        Budget,
        EnumDefinition,
        EnumValue,
        SharedBudget,
        Value,
        arity,
        integer_range,
    },
};
use serde_json::{
    Value as Json,
    json,
};
use std::{
    cell::RefCell,
    collections::HashMap,
    rc::Rc,
};

type Result<T> = std::result::Result<T, Failure>;

/// Deepest chain of active function calls. Each call can nest blocks and expressions to their own limits, so this
/// keeps the whole evaluation within browser and worker stacks.
const CALL_DEPTH_LIMIT: usize = 32;

/// Most iterations one range or collection loop may run.
const ITERATION_LIMIT: usize = 150_000;
const MAXIMUM_SAFE_INTEGER: f64 = 9_007_199_254_740_991.0;

/// Built-in constants every module can read.
const CONSTANTS: [(&str, f64); 3] =
    [("pi", std::f64::consts::PI), ("tau", std::f64::consts::TAU), ("e", std::f64::consts::E)];

enum Failure {
    Message(String),
    Located(Diagnostic),
}
impl From<String> for Failure {
    fn from(value: String) -> Self {
        Self::Message(value)
    }
}
impl From<Diagnostic> for Failure {
    fn from(value: Diagnostic) -> Self {
        Self::Located(value)
    }
}
impl Failure {
    fn at(self, token: &Token) -> Diagnostic {
        match self {
            Self::Located(error) => error,
            Self::Message(message) => Diagnostic::at(message, token),
        }
    }
}

/// How a block finished: normally, or by a `return` carrying the function's value (`Unit` for a bare `return`).
enum Flow {
    Next,
    Return(Value),
}

pub fn evaluate(
    program: &Program,
    overrides: &Json,
    materials: Option<&Json>,
) -> std::result::Result<GeometryData, Diagnostic> {
    let root = Token {
        text: String::new(),
        line: 1,
        column: 1,
    };
    if !overrides.is_object() {
        return Err(Diagnostic::at("Parameter overrides must be an object.", &root));
    }
    let budget = Rc::new(RefCell::new(Budget::default()));
    let output = Builder::new(materials).map_err(|message| Diagnostic::at(message, &root))?;
    let mut runtime = Runtime {
        program,
        overrides,
        output,
        random: 1,
        budget: budget.clone(),
        procedural: Procedural::new(budget),
        modules: (0..program.modules.len()).map(|_| ModuleState::default()).collect(),
        call_depth: 0,
    };
    runtime.initialize(0).map_err(|error| error.at(&root))?;
    runtime.procedural.finish(&mut runtime.output).map_err(|message| Diagnostic::at(message, &root))?;
    Ok(runtime.output.data)
}

/// One module's evaluation state: its script-level bindings and hoisted functions.
#[derive(Default)]
struct ModuleState {
    globals: Globals,
    functions: HashMap<String, Rc<FunctionDefinition>>,
    initialized: bool,
}

struct Runtime<'a> {
    program: &'a Program,
    overrides: &'a Json,
    output: Builder,
    random: u32,
    budget: SharedBudget,
    procedural: Procedural,
    modules: Vec<ModuleState>,
    call_depth: usize,
}

impl Runtime<'_> {
    // ---- Modules and declarations.

    /// Hoist a module's declarations, initialize the modules it uses, then run its top-level statements once.
    fn initialize(&mut self, module: usize) -> Result<()> {
        if self.modules[module].initialized {
            return Ok(());
        }
        self.modules[module].initialized = true;
        for (name, value) in CONSTANTS {
            self.modules[module].globals.insert(
                name.into(),
                Binding {
                    value: Value::Number(value),
                    role: BindingRole::Constant,
                },
            );
        }

        let program = self.program;
        let statements = &program.modules[module].statements;
        for statement in statements {
            self.declare(module, statement).map_err(|error| Failure::Located(error.at(&statement.token)))?;
        }
        let mut scope = Scope::script(module);
        self.execute(statements, &mut scope)?;
        Ok(())
    }

    /// Register one hoisted declaration; other statements run in order later.
    fn declare(&mut self, module: usize, statement: &Statement) -> Result<()> {
        match &statement.kind {
            StmtKind::Function(definition) => {
                let name = &definition.name;
                if is_builtin(name) {
                    return Err(
                        format!("Function '{name}' would hide the built-in {name}(); choose another name.").into()
                    );
                }
                if self.modules[module].functions.contains_key(name) {
                    return Err(format!("Duplicate function '{name}'.").into());
                }
                self.modules[module].functions.insert(name.clone(), definition.clone());
            }
            StmtKind::Enum {
                name,
                members,
            } => {
                let definition = Rc::new(EnumDefinition {
                    name: name.clone(),
                    members: members.clone(),
                });
                self.declare_global(module, name, Value::EnumType(definition))?;
            }
            StmtKind::Use {
                alias,
                module: target,
                ..
            } => {
                self.initialize(*target).map_err(|error| self.in_module(*target, error, &statement.token))?;
                self.declare_global(module, alias, Value::Module(*target))?;
            }
            _ => {}
        }
        Ok(())
    }

    fn declare_global(&mut self, module: usize, name: &str, value: Value) -> Result<()> {
        let globals = &mut self.modules[module].globals;
        if globals.get(name).is_some_and(|binding| binding.role == BindingRole::Declaration) {
            return Err(format!("Duplicate declaration '{name}'.").into());
        }
        globals.insert(
            name.into(),
            Binding {
                value,
                role: BindingRole::Declaration,
            },
        );
        Ok(())
    }

    /// Report a failure inside another module at `token` in the current one, naming the module and its location.
    fn in_module(&self, module: usize, failure: Failure, token: &Token) -> Failure {
        match failure {
            Failure::Located(error) => Failure::Located(Diagnostic::at(
                format!(
                    "{} ({}:{}): {}",
                    self.program.modules[module].display_path(),
                    error.line,
                    error.column,
                    error.message
                ),
                token,
            )),
            message => message,
        }
    }

    // ---- Statements.

    fn execute(&mut self, statements: &[Statement], scope: &mut Scope) -> Result<Flow> {
        for statement in statements {
            self.spend(&statement.token)?;
            let flow =
                self.statement(statement, scope).map_err(|error| Failure::Located(error.at(&statement.token)))?;
            if let Flow::Return(_) = flow {
                return Ok(flow);
            }
        }
        Ok(Flow::Next)
    }

    /// Run `statements` in a new frame, which ends with the block.
    fn block(&mut self, statements: &[Statement], scope: &mut Scope, frame: Frame) -> Result<Flow> {
        scope.frames.push(frame);
        let flow = self.execute(statements, scope);
        scope.frames.pop();
        flow
    }

    fn statement(&mut self, statement: &Statement, scope: &mut Scope) -> Result<Flow> {
        match &statement.kind {
            StmtKind::Project {
                fields: project,
                ..
            } => {
                if !Self::is_script_level(scope) || self.output.data.project.is_some() {
                    return Err(
                        "Declare project metadata once, at the top level of the script, outside blocks and functions."
                            .to_string()
                            .into(),
                    );
                }
                self.output.data.project = Some(project.clone());
            }
            StmtKind::Wrangle {
                domain,
                expression,
                binding,
                body,
            } => {
                let value = self.expression(expression, scope)?;
                let count = self.procedural.begin_wrangle(domain, &value)?;
                let mut flow = Ok(Flow::Next);
                for index in 0..count {
                    if let Err(message) = self.budget.borrow_mut().spend(1) {
                        flow = Err(message.into());
                        break;
                    }
                    self.procedural.set_index(index);
                    let frame = Frame::from([
                        loop_binding("index", Value::Number(index as f64)),
                        loop_binding(binding, Value::Element(index)),
                    ]);
                    flow = self.block(body, scope, frame);
                    if !matches!(flow, Ok(Flow::Next)) {
                        break;
                    }
                }
                let end = self.procedural.end_wrangle();
                let flow = flow?;
                end?;
                return Ok(flow);
            }
            StmtKind::BindingAttribute {
                binding,
                name,
                operator,
                expression,
            } => {
                if !matches!(self.lookup(scope, binding), Some(Value::Element(_))) {
                    return Err(format!("Unknown wrangle binding '{binding}'.").into());
                }
                let mut value = self.expression(expression, scope)?;
                if let Some(arithmetic) = operator.arithmetic() {
                    value = math::arithmetic(arithmetic, &self.procedural.read(name)?, &value)?;
                }
                self.procedural.write(name, &value)?;
            }
            StmtKind::If {
                branches,
                otherwise,
            } => {
                for branch in branches {
                    let condition = self.expression(&branch.condition, scope)?;
                    let taken =
                        condition.condition().map_err(|message| Diagnostic::at(message, &branch.condition.token))?;
                    if taken {
                        return self.block(&branch.body, scope, Frame::new());
                    }
                }
                if let Some(body) = otherwise {
                    return self.block(body, scope, Frame::new());
                }
            }
            StmtKind::Match {
                subject,
                cases,
                otherwise,
            } => {
                let subject = self.expression(subject, scope)?;
                for case in cases {
                    for candidate in &case.values {
                        let value = self.expression(candidate, scope)?;
                        let equal = math::equals(&subject, &value)
                            .map_err(|message| Diagnostic::at(message, &candidate.token))?;
                        if equal {
                            return self.block(&case.body, scope, Frame::new());
                        }
                    }
                }
                if let Some(body) = otherwise {
                    return self.block(body, scope, Frame::new());
                }
            }
            StmtKind::Let(name, expression) => {
                let value = self.expression(expression, scope)?;
                self.bind(scope, name, value, BindingRole::Variable)?;
            }
            StmtKind::Const(name, expression) => {
                let value = self.expression(expression, scope)?;
                value.freeze()?;
                self.bind(scope, name, value, BindingRole::Constant)?;
            }
            StmtKind::Assign {
                name,
                operator,
                expression,
            } => {
                let mut value = self.expression(expression, scope)?;
                if let Some(arithmetic) = operator.arithmetic() {
                    let current = self.lookup(scope, name).ok_or_else(|| unknown_assignment(name))?;
                    value = math::arithmetic(arithmetic, &current, &value)?;
                }
                self.assign(scope, name, value)?;
            }
            StmtKind::Param {
                name,
                values,
                metadata,
                presentation,
            } => {
                if !Self::is_script_level(scope) || scope.module != 0 {
                    return Err("Declare parameters at the top level of the main script, outside blocks, functions, and modules.".to_string().into());
                }
                if self.output.data.parameters.iter().any(|p| p["name"] == name.as_str()) {
                    return Err(format!("Duplicate parameter '{name}'.").into());
                }
                let mut defaults = Vec::new();
                for expression in values {
                    defaults.push(parameters::read(&self.expression(expression, scope)?)?);
                }
                let mut evaluated_metadata = Vec::new();
                for (key, expression) in metadata {
                    evaluated_metadata.push((key.clone(), parameters::read(&self.expression(expression, scope)?)?));
                }
                let supplied = self.overrides.get(name).cloned().unwrap_or_else(|| default_override(&defaults[0]));
                let (parameter, value) =
                    parameters::create(name, &defaults, &evaluated_metadata, presentation, &supplied)?;
                if value.kind("mesh") {
                    self.track_mesh(&value.json())?;
                }
                self.output.data.parameters.push(parameter);
                self.bind(scope, name, value, BindingRole::Parameter)?;
            }
            StmtKind::For {
                name,
                start,
                end,
                body,
            } => {
                let start = range_bound(&self.expression(start, scope)?)?;
                let end = range_bound(&self.expression(end, scope)?)?;
                if end - start > ITERATION_LIMIT as i64 {
                    return Err(format!("Loop iteration limit exceeded ({ITERATION_LIMIT}).").into());
                }
                for index in start..end {
                    self.spend(&statement.token)?;
                    let frame = Frame::from([loop_binding(name, Value::Number(index as f64))]);
                    let flow = self.block(body, scope, frame)?;
                    if let Flow::Return(_) = flow {
                        return Ok(flow);
                    }
                }
            }
            StmtKind::ForEach {
                name,
                collection,
                body,
            } => {
                let collection = self.expression(collection, scope)?;
                let count = match &collection {
                    Value::Data(data) if data.is_array() => data.as_array().unwrap().len(),
                    Value::Array(components) => components.len(),
                    other => {
                        return Err(
                            format!("foreach expects a list, vector, or color; found {}.", other.describe()).into()
                        );
                    }
                };
                if count > ITERATION_LIMIT {
                    return Err(format!("Loop iteration limit exceeded ({ITERATION_LIMIT}).").into());
                }
                for index in 0..count {
                    self.spend(&statement.token)?;
                    let value = element_at(&collection, &Value::Number(index as f64))?;
                    let frame = Frame::from([loop_binding(name, value)]);
                    let flow = self.block(body, scope, frame)?;
                    if let Flow::Return(_) = flow {
                        return Ok(flow);
                    }
                }
            }
            StmtKind::Return(expression) => {
                let value = match expression {
                    Some(expression) => self.expression(expression, scope)?,
                    None => Value::Unit,
                };
                return Ok(Flow::Return(value));
            }
            StmtKind::Command(expression) => {
                self.call_value(expression, scope)?;
            }
            // Declarations are hoisted when their module initializes.
            StmtKind::Function(_)
            | StmtKind::Enum {
                ..
            }
            | StmtKind::Use {
                ..
            } => {}
        }
        Ok(Flow::Next)
    }

    // ---- Bindings.

    fn is_script_level(scope: &Scope) -> bool {
        scope.frames.is_empty() && !scope.in_function
    }

    fn lookup(&self, scope: &Scope, name: &str) -> Option<Value> {
        scope.find(name).or_else(|| self.modules[scope.module].globals.get(name)).map(|binding| binding.value.clone())
    }

    /// Declare `name` in the innermost frame, or as a script-level binding at the top level.
    fn bind(&mut self, scope: &mut Scope, name: &str, value: Value, role: BindingRole) -> Result<()> {
        let existing = match scope.frames.last() {
            Some(frame) => frame.get(name),
            None => self.modules[scope.module].globals.get(name),
        };
        if let Some(binding) =
            existing.filter(|binding| matches!(binding.role, BindingRole::Constant | BindingRole::LoopValue))
        {
            return Err(
                format!("Cannot redeclare the {} '{name}' in the same scope.", binding.role.description()).into()
            );
        }
        let binding = Binding {
            value,
            role,
        };
        match scope.frames.last_mut() {
            Some(frame) => frame.insert(name.into(), binding),
            None => self.modules[scope.module].globals.insert(name.into(), binding),
        };
        Ok(())
    }

    /// Check a reference before executing the callee, which would otherwise mutate aliased resources before copy-back.
    fn reference_value(&self, scope: &Scope, name: &str) -> Result<Value> {
        let binding = if let Some(binding) = scope.find(name) {
            binding
        } else {
            let binding = self.modules[scope.module].globals.get(name).ok_or_else(|| unknown_assignment(name))?;
            if scope.in_function {
                return Err(format!(
                    "Functions can't assign the script-level variable '{name}'; return a value or pass it with ref."
                )
                .into());
            }
            binding
        };
        if binding.role != BindingRole::Variable {
            return Err(format!("Cannot pass the {} '{name}' by ref.", binding.role.description()).into());
        }
        binding.value.ensure_mutable()?;
        Ok(binding.value.clone())
    }

    /// Replace the nearest existing variable named `name`.
    fn assign(&mut self, scope: &mut Scope, name: &str, value: Value) -> Result<()> {
        if let Some(binding) = scope.find_mut(name) {
            return replace(binding, name, value);
        }
        let in_function = scope.in_function;
        match self.modules[scope.module].globals.get_mut(name) {
            Some(_) if in_function => Err(format!(
                "Functions can't assign the script-level variable '{name}'; return a value or pass it with ref."
            )
            .into()),
            Some(binding) => replace(binding, name, value),
            None => Err(unknown_assignment(name)),
        }
    }

    // ---- Expressions.

    /// Evaluate an expression that must produce a value.
    fn expression(&mut self, expression: &Expression, scope: &mut Scope) -> Result<Value> {
        self.spend(&expression.token)?;
        let value = match &expression.kind {
            ExprKind::Number(number) => Value::Number(*number),
            ExprKind::Bool(flag) => return Ok(Value::Bool(*flag)),
            ExprKind::Null => return Ok(Value::data(Json::Null)),
            ExprKind::Text(text) => return Ok(Value::text(text)),
            ExprKind::Record(fields) => {
                let mut record = serde_json::Map::new();
                for (name, field) in fields {
                    record.insert(name.clone(), records::data_value(&self.expression(field, scope)?)?);
                }
                return Ok(Value::data(records::copy(&Json::Object(record), &self.budget)?));
            }
            ExprKind::Array(items) => {
                let mut values = Vec::new();
                for item in items {
                    values.push(records::data_value(&self.expression(item, scope)?)?);
                }
                return Ok(Value::data(records::copy(&Json::Array(values), &self.budget)?));
            }
            ExprKind::Variable(name) => self
                .lookup(scope, name)
                .ok_or_else(|| Diagnostic::at(format!("Unknown variable '{name}'."), &expression.token))?,
            ExprKind::Component(_, _) => self.component_chain(expression, scope)?,
            ExprKind::Index(target, index) => {
                let target = self.expression(target, scope)?;
                let index = self.expression(index, scope)?;
                element_at(&target, &index).map_err(|message| Diagnostic::at(message, &expression.token))?
            }
            ExprKind::Unary(operator, operand) => {
                let value = self.expression(operand, scope)?;
                if operator == OPERATOR_NOT {
                    let flag = value.condition().map_err(|_| {
                        Diagnostic::at(
                            format!("NOT needs true or false; found {}.", value.describe()),
                            &expression.token,
                        )
                    })?;
                    return Ok(Value::Bool(!flag));
                }
                let sign = if operator == "-" { -1.0 } else { 1.0 };
                match value {
                    Value::Array(values) => Value::Array(values.into_iter().map(|v| v * sign).collect()),
                    _ => Value::Number(sign * value.scalar()?),
                }
            }
            ExprKind::Binary(_, _, _) => self.binary_chain(expression, scope)?,
            ExprKind::Call(..) | ExprKind::ModuleCall(..) => {
                let value = self.call_value(expression, scope)?;
                if let Value::Unit = value {
                    return Err(Diagnostic::at(
                        format!("{}() doesn't return a value.", call_name(expression)),
                        &expression.token,
                    )
                    .into());
                }
                value
            }
            ExprKind::Reference(_) => {
                return Err(Diagnostic::at(
                    "'ref' can only pass a variable to a function parameter declared with ref.",
                    &expression.token,
                )
                .into());
            }
        };
        if matches!(&value,Value::Number(n) if !n.is_finite())
            || matches!(&value,Value::Array(v) if v.iter().any(|n|!n.is_finite()))
        {
            return Err(Diagnostic::at("Expression must produce a finite number.", &expression.token).into());
        }
        Ok(value)
    }

    // The parser intentionally accepts flat chains up to its token budget.
    // Evaluate their left spines iteratively rather than borrowing native stack.
    fn binary_chain(&mut self, expression: &Expression, scope: &mut Scope) -> Result<Value> {
        let mut frames = Vec::new();
        let mut current = expression;
        while let ExprKind::Binary(operator, left, right) = &current.kind {
            frames.push((operator.as_str(), right.as_ref(), &current.token));
            if matches!(left.kind, ExprKind::Binary(_, _, _)) {
                self.spend(&left.token)?;
            }
            current = left;
        }
        let mut value = self.expression(current, scope)?;
        while let Some((operator, right, token)) = frames.pop() {
            if operator == OPERATOR_AND || operator == OPERATOR_OR {
                let left = logical_operand(operator, &value, token)?;
                // AND stops at false and OR at true, without evaluating the right side.
                if left == (operator == OPERATOR_OR) {
                    value = Value::Bool(left);
                    continue;
                }
                let right = self.expression(right, scope)?;
                value = Value::Bool(logical_operand(operator, &right, token)?);
                continue;
            }
            let right = self.expression(right, scope)?;
            value = math::arithmetic(operator, &value, &right).map_err(|message| Diagnostic::at(message, token))?;
            if matches!(&value,Value::Number(n) if !n.is_finite())
                || matches!(&value,Value::Array(v) if v.iter().any(|n|!n.is_finite()))
            {
                return Err(Diagnostic::at("Expression must produce a finite number.", token).into());
            }
        }
        Ok(value)
    }

    fn component_chain(&mut self, expression: &Expression, scope: &mut Scope) -> Result<Value> {
        let mut frames = Vec::new();
        let mut current = expression;
        while let ExprKind::Component(parent, name) = &current.kind {
            frames.push((name.as_str(), &current.token));
            if matches!(parent.kind, ExprKind::Component(_, _)) {
                self.spend(&parent.token)?;
            }
            current = parent;
        }
        let mut value = self.expression(current, scope)?;
        while let Some((name, token)) = frames.pop() {
            value = match value {
                Value::Element(index) => {
                    if name == "index" {
                        Value::Number(index as f64)
                    } else {
                        self.procedural.read(name)?
                    }
                }
                Value::Data(record) => {
                    let field = record
                        .as_object()
                        .and_then(|map| map.get(name))
                        .ok_or_else(|| Diagnostic::at(format!("Unknown record field '{name}'."), token))?;
                    data_element(field, || record.child(name))
                }
                Value::Array(components) => {
                    let names: &[&str] = if components.len() == 3 { &["x", "y", "z"] } else { &["r", "g", "b", "a"] };
                    let index = names
                        .iter()
                        .position(|value| *value == name)
                        .ok_or_else(|| Diagnostic::at("Invalid vector or color component.", token))?;
                    Value::Number(components[index])
                }
                Value::EnumType(definition) => {
                    let index = definition.members.iter().position(|member| member == name).ok_or_else(|| {
                        Diagnostic::at(format!("Enum {} has no member '{name}'.", definition.name), token)
                    })?;
                    Value::Enum(EnumValue {
                        definition,
                        index,
                    })
                }
                Value::Module(module) => {
                    let binding = self.modules[module].globals.get(name).ok_or_else(|| {
                        Diagnostic::at(
                            format!(
                                "Module '{}' has no declaration '{name}'.",
                                self.program.modules[module].display_path()
                            ),
                            token,
                        )
                    })?;
                    binding.value.clone()
                }
                _ => return Err(Diagnostic::at("Invalid vector or color component.", token).into()),
            };
        }
        Ok(value)
    }

    // ---- Calls.

    /// Evaluate a call, which may return [`Value::Unit`] when it is used as a statement.
    fn call_value(&mut self, expression: &Expression, scope: &mut Scope) -> Result<Value> {
        match &expression.kind {
            ExprKind::Call(name, arguments) => {
                if let Some(definition) = self.modules[scope.module].functions.get(name).cloned() {
                    return self.call_function(scope.module, &definition, arguments, scope, &expression.token);
                }
                if name == "deform" {
                    return self.deform(arguments, scope);
                }
                let mut values = Vec::with_capacity(arguments.len());
                for argument in arguments {
                    values.push(self.expression(argument, scope)?);
                }
                // Built-in failures point at the call, not the statement around it.
                self.builtin(name, &values, &expression.token)
                    .map_err(|failure| Failure::Located(failure.at(&expression.token)))
            }
            ExprKind::ModuleCall(receiver, name, arguments) => {
                let module = match self.expression(receiver, scope)? {
                    Value::Module(module) => module,
                    other => {
                        return Err(Diagnostic::at(
                            format!(
                                "Only module functions are called with '.'; {} has no functions. Call {name}(value, …) instead.",
                                other.describe()
                            ),
                            &expression.token,
                        )
                        .into());
                    }
                };
                let definition = self.modules[module].functions.get(name).cloned().ok_or_else(|| {
                    Diagnostic::at(
                        format!("Module '{}' has no function '{name}'.", self.program.modules[module].display_path()),
                        &expression.token,
                    )
                })?;
                self.call_function(module, &definition, arguments, scope, &expression.token)
            }
            _ => Err(Diagnostic::at("Expected a function call.", &expression.token).into()),
        }
    }

    /// Call a script function. Arguments are copies (geometry and instance collections are duplicated); `ref`
    /// parameters edit the caller's variable, which receives the parameter's final value when the call returns.
    fn call_function(
        &mut self,
        module: usize,
        definition: &Rc<FunctionDefinition>,
        arguments: &[Expression],
        caller: &mut Scope,
        token: &Token,
    ) -> Result<Value> {
        let name = &definition.name;
        if arguments.len() != definition.parameters.len() {
            return Err(Diagnostic::at(
                format!("{name} expects {} arguments; received {}.", definition.parameters.len(), arguments.len()),
                token,
            )
            .into());
        }
        let mut frame = Frame::new();
        let mut references = Vec::new();
        for (parameter, argument) in definition.parameters.iter().zip(arguments) {
            let value = match (&argument.kind, parameter.by_reference) {
                (ExprKind::Reference(variable), true) => {
                    let value =
                        self.reference_value(caller, variable).map_err(|failure| failure.at(&argument.token))?;
                    references.push((parameter.name.clone(), variable.clone()));
                    value
                }
                (_, true) => {
                    return Err(Diagnostic::at(
                        format!(
                            "Parameter '{}' of {name} is declared ref; pass a variable with 'ref {}'.",
                            parameter.name, parameter.name
                        ),
                        &argument.token,
                    )
                    .into());
                }
                (ExprKind::Reference(_), false) => {
                    return Err(Diagnostic::at(
                        format!("Parameter '{}' of {name} is not declared ref; remove 'ref'.", parameter.name),
                        &argument.token,
                    )
                    .into());
                }
                (_, false) => {
                    let value = self.expression(argument, caller)?;
                    self.procedural.copy_argument(&value).map_err(|message| Diagnostic::at(message, &argument.token))?
                }
            };
            frame.insert(
                parameter.name.clone(),
                Binding {
                    value,
                    role: BindingRole::Variable,
                },
            );
        }

        if self.call_depth >= CALL_DEPTH_LIMIT {
            return Err(
                Diagnostic::at(format!("Function call depth limit exceeded ({CALL_DEPTH_LIMIT})."), token).into()
            );
        }
        self.spend(token)?;
        self.call_depth += 1;
        let mut scope = Scope::function(module, frame);
        let flow = self.execute(&definition.body, &mut scope);
        self.call_depth -= 1;
        let flow = match flow {
            Ok(flow) => flow,
            Err(failure) if module != caller.module => return Err(self.in_module(module, failure, token)),
            Err(failure) => return Err(failure),
        };

        for (parameter, variable) in references {
            let value = scope.frames[0].get(&parameter).expect("parameters are bound in the first frame").value.clone();
            self.assign(caller, &variable, value).map_err(|failure| failure.at(token))?;
        }
        Ok(match flow {
            Flow::Return(value) => value,
            Flow::Next => Value::Unit,
        })
    }

    /// Run the built-in `name` with evaluated arguments.
    fn builtin(&mut self, name: &str, values: &[Value], token: &Token) -> Result<Value> {
        if let Some(message) = retired_builtin(name, values) {
            return Err(Diagnostic::at(message, token).into());
        }
        if let Some(value) = math::vector_function(name, values)? {
            return Ok(value);
        }
        if let Some(value) = self.procedural.call(name, values, &mut self.output)? {
            return Ok(value);
        }
        if let Some(value) = self.resource(name, values)? {
            return Ok(value);
        }
        match name {
            "vector" | "rgba" => {
                let args = scalars(values)?;
                arity(name, args.len(), if name == "vector" { 3 } else { 4 })?;
                Ok(Value::Array(args))
            }
            "rand" => {
                arity(name, values.len(), 0)?;
                self.random = self.random.wrapping_mul(1664525).wrapping_add(1013904223);
                Ok(Value::Number(self.random as f64 / 4294967296.0))
            }
            "seed" => {
                let args = scalars(values)?;
                arity(name, args.len(), 1)?;
                if args[0].fract() != 0.0 {
                    return Err("Random seed must be a finite integer.".to_string().into());
                }
                self.random = math::to_u32(args[0]);
                Ok(Value::Unit)
            }
            "number" => {
                arity(name, values.len(), 1)?;
                match &values[0] {
                    Value::Bool(flag) => Ok(Value::Number(if *flag { 1.0 } else { 0.0 })),
                    Value::Number(number) => Ok(Value::Number(*number)),
                    other => Err(format!("number expects true/false or a number; found {}.", other.describe()).into()),
                }
            }
            "itemCount" => {
                arity(name, values.len(), 1)?;
                match &values[0] {
                    Value::Data(data) if data.is_array() => Ok(Value::Number(data.as_array().unwrap().len() as f64)),
                    Value::Data(data) if data.is_object() => Ok(Value::Number(data.as_object().unwrap().len() as f64)),
                    Value::Array(components) => Ok(Value::Number(components.len() as f64)),
                    other => Err(format!("itemCount expects a list or record; found {}.", other.describe()).into()),
                }
            }
            _ => {
                let args = scalars(values)?;
                Ok(Value::Number(
                    math::function(name, &args)?
                        .ok_or_else(|| Diagnostic::at(format!("Unknown function '{name}'."), token))?,
                ))
            }
        }
    }

    fn resource(&mut self, name: &str, values: &[Value]) -> Result<Option<Value>> {
        if name == "materialPbr" {
            arity(name, values.len(), 3)?;
            let Value::Array(color) = &values[0] else {
                return Err("materialPbr expects RGBA color, metallic, roughness.".to_string().into());
            };
            if color.len() != 4 {
                return Err("materialPbr expects RGBA color, metallic, roughness.".to_string().into());
            }
            return Ok(Some(Value::object(resources::material_pbr(color, values[1].scalar()?, values[2].scalar()?)?)));
        }
        let mesh = match name {
            "meshBox" => {
                arity(name, values.len(), 3)?;
                resources::mesh_box(values[0].scalar()?, values[1].scalar()?, values[2].scalar()?)?
            }
            "meshSphere" => {
                if values.len() != 1 && values.len() != 2 {
                    return Err("meshSphere expects radius and optional segments.".to_string().into());
                }
                resources::mesh_sphere(values[0].scalar()?, if values.len() == 2 { values[1].scalar()? } else { 20.0 })?
            }
            "transformMesh" => {
                arity(name, values.len(), 4)?;
                let mesh = values[0].shared("mesh")?;

                resources::transform_mesh(
                    &mesh.borrow(),
                    &values[1].vector()?,
                    &values[2].vector()?,
                    &values[3].vector()?,
                )?
            }
            _ => return Ok(None),
        };
        self.track_mesh(&mesh)?;
        Ok(Some(Value::object(mesh)))
    }
    fn track_mesh(&mut self, mesh: &Json) -> Result<()> {
        self.budget.borrow_mut().mesh(mesh["positions"].as_array().map_or(0, Vec::len) / 3)?;
        Ok(())
    }

    /// `deform(mesh, x, y, z)`: evaluate the three coordinate expressions per vertex with `x`, `y`, `z`, `u`, `v`,
    /// and `index` bound to that vertex.
    fn deform(&mut self, expressions: &[Expression], scope: &mut Scope) -> Result<Value> {
        arity("deform", expressions.len(), 4)?;
        let source = self.expression(&expressions[0], scope)?.shared("mesh")?;
        let source = source.borrow().clone();
        let mut result = resources::read_mesh(&source)?;
        self.track_mesh(&result)?;
        let positions = source["positions"].as_array().ok_or_else(|| "Expected a mesh value.".to_string())?;
        let uvs = source["uvs"].as_array().ok_or_else(|| "Expected a mesh value.".to_string())?;
        for index in 0..positions.len() / 3 {
            let mut frame = Frame::new();
            for (axis, name) in ["x", "y", "z"].iter().enumerate() {
                let coordinate = Value::Number(positions[index * 3 + axis].as_f64().unwrap());
                frame.insert((*name).into(), constant_binding(coordinate));
            }
            frame.insert("u".into(), constant_binding(Value::Number(uvs[index * 2].as_f64().unwrap())));
            frame.insert("v".into(), constant_binding(Value::Number(uvs[index * 2 + 1].as_f64().unwrap())));
            frame.insert("index".into(), constant_binding(Value::Number(index as f64)));
            scope.frames.push(frame);
            let mut coordinates = [0.0; 3];
            let mut failure = None;
            for (axis, coordinate) in coordinates.iter_mut().enumerate() {
                match self.expression(&expressions[axis + 1], scope).and_then(|value| Ok(value.scalar()?)) {
                    Ok(value) => *coordinate = value,
                    Err(error) => {
                        failure = Some(error);
                        break;
                    }
                }
            }
            scope.frames.pop();
            if let Some(error) = failure {
                return Err(error);
            }
            for (axis, coordinate) in coordinates.iter().enumerate() {
                result["positions"][index * 3 + axis] = json!(coordinate);
            }
        }
        result["normals"] = json!(vec![0.0; positions.len()]);
        Ok(Value::object(resources::read_mesh(&result)?))
    }

    fn spend(&self, token: &Token) -> Result<()> {
        self.budget.borrow_mut().spend(1).map_err(|message| Failure::Located(Diagnostic::at(message, token)))
    }
}

fn scalars(values: &[Value]) -> std::result::Result<Vec<f64>, String> {
    values.iter().map(Value::scalar).collect()
}

fn loop_binding(name: &str, value: Value) -> (String, Binding) {
    (
        name.into(),
        Binding {
            value,
            role: BindingRole::LoopValue,
        },
    )
}

/// Range arithmetic stays exact on native and WebAssembly targets, including negative endpoints.
fn range_bound(value: &Value) -> Result<i64> {
    let number = value.scalar()?;
    if number.fract() != 0.0 || number.abs() > MAXIMUM_SAFE_INTEGER {
        return Err("Range bounds must be integers between -9007199254740991 and 9007199254740991.".to_owned().into());
    }
    Ok(number as i64)
}

fn constant_binding(value: Value) -> Binding {
    Binding {
        value,
        role: BindingRole::Constant,
    }
}

fn replace(binding: &mut Binding, name: &str, value: Value) -> Result<()> {
    if binding.role != BindingRole::Variable {
        return Err(format!("Cannot assign to the {} '{name}'.", binding.role.description()).into());
    }
    binding.value = value;
    Ok(())
}

fn unknown_assignment(name: &str) -> Failure {
    Failure::Message(format!("Unknown variable '{name}'; declare it with let before assigning it."))
}

/// The override a parameter receives when the host supplies none: its default in descriptor form.
fn default_override(default: &Value) -> Json {
    match default {
        Value::Enum(member) => json!(member.index),
        other => other.json(),
    }
}

fn logical_operand(operator: &str, value: &Value, token: &Token) -> Result<bool> {
    match value {
        Value::Bool(flag) => Ok(*flag),
        other => Err(Diagnostic::at(
            format!("{operator} needs true or false on both sides; found {}.", other.describe()),
            token,
        )
        .into()),
    }
}

/// The value a record field or list element holds: numbers, text, and true/false become plain values, lists of three or
/// four numbers become vectors or colors, and other nested records and lists stay records.
fn data_element(field: &Json, nested: impl FnOnce() -> crate::value::DataRef) -> Value {
    // Three or four numbers read back as the vector or color they were stored from.
    if let Some(items) = field.as_array()
        && (items.len() == 3 || items.len() == 4)
        && let Some(components) = items.iter().map(Json::as_f64).collect::<Option<Vec<f64>>>()
    {
        return Value::Array(components);
    }
    if let Some(number) = field.as_f64() {
        Value::Number(number)
    } else if let Some(text) = field.as_str() {
        Value::text(text)
    } else if let Some(flag) = field.as_bool() {
        Value::Bool(flag)
    } else {
        Value::Data(nested())
    }
}

/// `target[index]`: a list element by position, a record field by text or enum member, or a vector or color
/// component by position.
fn element_at(target: &Value, index: &Value) -> std::result::Result<Value, String> {
    match (target, index) {
        (Value::Data(data), Value::Number(position)) if data.is_array() => {
            let items = data.as_array().unwrap();
            let position = integer_range(*position, 0.0, items.len() as f64 - 1.0)
                .map_err(|_| format!("List index {position} is outside 0–{}.", items.len() as i64 - 1))?;
            Ok(data_element(&items[position], || data.child(&position.to_string())))
        }
        (Value::Data(data), Value::Text(_) | Value::Enum(_)) if data.is_object() => {
            let key = match index {
                Value::Text(text) => text.to_string(),
                Value::Enum(member) => member.member().to_owned(),
                _ => unreachable!(),
            };
            let field = data.as_object().unwrap().get(&key).ok_or_else(|| format!("Unknown record field '{key}'."))?;
            Ok(data_element(field, || data.child(&key)))
        }
        (Value::Array(components), Value::Number(position)) => {
            let position = integer_range(*position, 0.0, components.len() as f64 - 1.0)?;
            Ok(Value::Number(components[position]))
        }
        _ => Err(format!("Cannot index {} with {}.", target.describe(), index.describe())),
    }
}

fn call_name(expression: &Expression) -> &str {
    match &expression.kind {
        ExprKind::Call(name, _) | ExprKind::ModuleCall(_, name, _) => name,
        _ => "call",
    }
}

/// Migration guidance for drawing commands that used to write to an implicit canvas.
fn retired_builtin(name: &str, values: &[Value]) -> Option<&'static str> {
    let geometry_first = values.first().is_some_and(|value| value.kind("geometry") || value.kind("mesh"));
    match name {
        "point" => Some("point() was removed; add points to geometry with addPoint(geometry, vector(x, y, z))."),
        "triangle" => Some("triangle() was removed; add faces to geometry with addPoint() and addFace()."),
        "surface" => Some("surface() was removed; shape a grid(columns, rows, width, depth) in a points wrangle."),
        "uv" | "normal" => {
            Some("uv() and normal() no longer set drawing state; assign 'point.uv' or 'point.normal' in a wrangle.")
        }
        "userData" | "setUserData" => {
            Some("User data was removed; store per-point values with addAttribute() and setAttribute().")
        }
        "instance" => {
            Some("instance() was removed; use place(mesh, position, rotation, scale) and material(geometry, material).")
        }
        "color" if !geometry_first => Some(
            "color(r, g, b) no longer sets a drawing color; color geometry with color(geometry, rgba(r, g, b, a)).",
        ),
        "material" if !geometry_first => {
            Some("material(name) no longer selects a drawing material; assign one with material(geometry, material).")
        }
        "box" if values.len() == 6 => Some(
            "box(x, y, z, width, height, depth) was removed; use translate(box(width, height, depth), vector(x, y, z)).",
        ),
        "sphere" if values.len() == 4 || values.len() == 5 => {
            Some("sphere(x, y, z, radius) was removed; use translate(sphere(radius, segments), vector(x, y, z)).")
        }
        _ => None,
    }
}
