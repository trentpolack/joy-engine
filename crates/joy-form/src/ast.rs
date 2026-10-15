// Copyright (c) 2026 Trent Polack. All Rights Reserved.
// Licensed under the MIT License.

//! Syntax tree produced by the parser and walked by the evaluator. Ported from `src/form/parser.ts`.

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
    Text(String),
    Literal(serde_json::Value),
    Variable(String),
    Component(Box<Expression>, String),
    Unary(String, Box<Expression>),
    Binary(String, Box<Expression>, Box<Expression>),
    Call(String, Vec<Expression>),
    Record(Vec<(String, Expression)>),
    Array(Vec<Expression>),
}
#[derive(Clone, Debug)]
pub struct Statement {
    pub kind: StmtKind,
    pub token: Token,
}
#[derive(Clone, Debug)]
pub enum StmtKind {
    Project(serde_json::Value),
    WithUserData(Expression, Vec<Statement>),
    Wrangle { domain: String, expression: Expression, binding: Option<String>, body: Vec<Statement> },
    BindingAttribute { binding: String, name: String, expression: Expression },
    If(Expression, Vec<Statement>),
    Let(String, Expression),
    Param { name: String, values: Vec<Expression>, metadata: Vec<(String, Expression)> },
    Repeat { name: String, expression: Expression, body: Vec<Statement> },
    Command(Expression),
}

// Flat binary/component chains are bounded by tokens, not recursive parser depth.
// Drain owned child expressions iteratively so disposing a compiled script is
// safe on browser and worker stacks as well as the native main thread.
impl Drop for Expression {
    fn drop(&mut self) {
        if matches!(self.kind, ExprKind::Number(_) | ExprKind::Text(_) | ExprKind::Literal(_) | ExprKind::Variable(_)) {
            return;
        }
        let mut pending = vec![std::mem::replace(&mut self.kind, ExprKind::Number(0.0))];
        while let Some(kind) = pending.pop() {
            match kind {
                ExprKind::Component(mut expression, _) | ExprKind::Unary(_, mut expression) => {
                    pending.push(std::mem::replace(&mut expression.kind, ExprKind::Number(0.0)));
                }
                ExprKind::Binary(_, mut left, mut right) => {
                    pending.push(std::mem::replace(&mut left.kind, ExprKind::Number(0.0)));
                    pending.push(std::mem::replace(&mut right.kind, ExprKind::Number(0.0)));
                }
                ExprKind::Call(_, expressions) | ExprKind::Array(expressions) => {
                    for mut expression in expressions {
                        pending.push(std::mem::replace(&mut expression.kind, ExprKind::Number(0.0)));
                    }
                }
                ExprKind::Record(fields) => {
                    for (_, mut expression) in fields {
                        pending.push(std::mem::replace(&mut expression.kind, ExprKind::Number(0.0)));
                    }
                }
                _ => {}
            }
        }
    }
}
