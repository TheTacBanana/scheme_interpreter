use scheme_core::parser::ast::AST;

use crate::{InterpreterContext, InterpreterResult};

pub type NativeFunc = fn(&mut InterpreterContext, usize) -> InterpreterResult<()>;
pub type MacroFunc = fn(&mut InterpreterContext, Vec<&AST>) -> InterpreterResult<AST>;

#[derive(Debug, Clone, PartialEq)]
pub enum Func {
    Native(String, NativeFunc),
    Macro(String, MacroFunc),
    Defined(Option<String>, Vec<String>, AST),
}


impl Func {
    pub fn to_string(&self) -> String {
        format!("{self}")
    }
}

impl std::fmt::Display for Func {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Func::Native(name, n) => write!(f, "{name} {n:?}"),
            Func::Macro(name, n) => write!(f, "{name} {n:?}"),
            Func::Defined(Some(name), args, body) => write!(f, "{name}({args:?})"),
            Func::Defined(None, args, body) => write!(f, "Lambda({args:?})"),
        }
    }
}
