// Copyright (c) 2026 Trent Polack. All Rights Reserved.
// Licensed under the MIT License.

//! Syntax tree produced by the parser and walked by the evaluator.

use std::rc::Rc;

#[derive(Clone, Debug)]
pub struct Token {
    pub text: String,
    pub line: usize,
    pub column: usize,
}
#[derive(Clone, Debug)]
pub struct Expression {
    pub kind: ExprKind,
    pub token: Token,
}
#[derive(Clone, Debug)]
pub enum ExprKind {
    Number(f64),
    Bool(bool),
    Null,
    Text(String),
    Variable(String),
    /// `value.name`: a vector or color component, a record field, an enum member, or a module member.
    Component(Box<Expression>, String),
    /// `value[index]`: a list element, a record field by text or enum key, or a vector or color component.
    Index(Box<Expression>, Box<Expression>),
    /// `-value`, `+value`, or `NOT condition`.
    Unary(String, Box<Expression>),
    Binary(String, Box<Expression>, Box<Expression>),
    Call(String, Vec<Expression>),
    /// `module.function(arguments)`: a call to a function declared in a module bound with `use`.
    ModuleCall(Box<Expression>, String, Vec<Expression>),
    /// `ref name`: a call argument that lets a function edit the caller's variable in place.
    Reference(String),
    Record(Vec<(String, Expression)>),
    Array(Vec<Expression>),
}
#[derive(Clone, Debug)]
pub struct Statement {
    pub kind: StmtKind,
    pub token: Token,
}
/// How editors present a parameter: text and control choices from `meta(...)` that never change evaluation, except
/// that a `kind` of `int`, `bool`, `seed`, or `enum` rounds the value to a whole number.
#[derive(Clone, Debug, Default)]
pub struct ParameterPresentation {
    pub label: Option<String>,
    pub group: Option<String>,
    pub info: Option<String>,
    pub kind: Option<String>,
    pub options: Vec<String>,
}

/// One `fn` declaration. Parameters marked `ref` edit the caller's variable; every other argument is a copy.
#[derive(Debug)]
pub struct FunctionDefinition {
    pub name: String,
    pub parameters: Vec<FunctionParameter>,
    pub body: Vec<Statement>,
}
#[derive(Clone, Debug)]
pub struct FunctionParameter {
    pub name: String,
    pub by_reference: bool,
}

/// One `if` or `else if` condition and the block it guards.
#[derive(Clone, Debug)]
pub struct ConditionalBranch {
    pub condition: Expression,
    pub body: Vec<Statement>,
}

/// One `case` arm of a `match`: the values it accepts and the block it runs.
#[derive(Clone, Debug)]
pub struct MatchCase {
    pub values: Vec<Expression>,
    pub body: Vec<Statement>,
}

/// `=` or a compound assignment (`+=`, `-=`, `*=`, `/=`), stored as its arithmetic operator.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum AssignmentOperator {
    Set,
    Add,
    Subtract,
    Multiply,
    Divide,
}
impl AssignmentOperator {
    /// The binary operator a compound assignment applies, or `None` for plain assignment.
    pub fn arithmetic(self) -> Option<&'static str> {
        match self {
            Self::Set => None,
            Self::Add => Some("+"),
            Self::Subtract => Some("-"),
            Self::Multiply => Some("*"),
            Self::Divide => Some("/"),
        }
    }
}

#[derive(Clone, Debug)]
pub enum StmtKind {
    Project {
        fields: serde_json::Value,
        name: Token,
    },
    Wrangle {
        domain: String,
        expression: Expression,
        binding: String,
        body: Vec<Statement>,
    },
    BindingAttribute {
        binding: String,
        name: String,
        operator: AssignmentOperator,
        expression: Expression,
    },
    If {
        branches: Vec<ConditionalBranch>,
        otherwise: Option<Vec<Statement>>,
    },
    Match {
        subject: Expression,
        cases: Vec<MatchCase>,
        otherwise: Option<Vec<Statement>>,
    },
    Let(String, Expression),
    Assign {
        name: String,
        operator: AssignmentOperator,
        expression: Expression,
    },
    Param {
        name: String,
        values: Vec<Expression>,
        metadata: Vec<(String, Expression)>,
        presentation: ParameterPresentation,
    },
    Repeat {
        name: String,
        expression: Expression,
        body: Vec<Statement>,
    },
    Function(Rc<FunctionDefinition>),
    Return(Option<Expression>),
    Enum {
        name: String,
        members: Vec<String>,
    },
    /// `use "path" as alias`; `module` indexes the compiled script's module table.
    Use {
        path: String,
        alias: String,
        module: usize,
    },
    Command(Expression),
}

// Flat binary/component chains are bounded by tokens, not recursive parser depth.
// Drain owned child expressions iteratively so disposing a compiled script is
// safe on browser and worker stacks as well as the native main thread.
impl Drop for Expression {
    fn drop(&mut self) {
        if matches!(
            self.kind,
            ExprKind::Number(_)
                | ExprKind::Bool(_)
                | ExprKind::Null
                | ExprKind::Text(_)
                | ExprKind::Variable(_)
                | ExprKind::Reference(_)
        ) {
            return;
        }
        let mut pending = vec![std::mem::replace(&mut self.kind, ExprKind::Null)];
        while let Some(kind) = pending.pop() {
            match kind {
                ExprKind::Component(mut expression, _) | ExprKind::Unary(_, mut expression) => {
                    pending.push(std::mem::replace(&mut expression.kind, ExprKind::Null));
                }
                ExprKind::Binary(_, mut left, mut right) | ExprKind::Index(mut left, mut right) => {
                    pending.push(std::mem::replace(&mut left.kind, ExprKind::Null));
                    pending.push(std::mem::replace(&mut right.kind, ExprKind::Null));
                }
                ExprKind::Call(_, expressions) | ExprKind::Array(expressions) => {
                    for mut expression in expressions {
                        pending.push(std::mem::replace(&mut expression.kind, ExprKind::Null));
                    }
                }
                ExprKind::ModuleCall(mut receiver, _, expressions) => {
                    pending.push(std::mem::replace(&mut receiver.kind, ExprKind::Null));
                    for mut expression in expressions {
                        pending.push(std::mem::replace(&mut expression.kind, ExprKind::Null));
                    }
                }
                ExprKind::Record(fields) => {
                    for (_, mut expression) in fields {
                        pending.push(std::mem::replace(&mut expression.kind, ExprKind::Null));
                    }
                }
                _ => {}
            }
        }
    }
}
