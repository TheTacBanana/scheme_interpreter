use std::{env, fs::File, io::Read};

use scheme_core::{literal::Literal, parser::ast::AST, LexerParser};

use scheme_core::token::span::TotalSpan;

use crate::{
    alloc::{InterpreterHeapAlloc, InterpreterStackAlloc},
    deref::InterpreterDeref,
    func::Func,
    object::{HeapObject, ObjectPointer, ObjectRef, StackObject, UnallocatedObject},
    InterpreterContext, InterpreterError, InterpreterErrorKind, InterpreterResult,
};

pub fn stack_trace(interpreter: &mut InterpreterContext, n: usize) -> InterpreterResult<()> {
    interpreter.stack_trace();
    Ok(())
}

pub fn heap_dump(interpreter: &mut InterpreterContext, n: usize) -> InterpreterResult<()> {
    interpreter.heap_dump();
    Ok(())
}

pub fn import(interpreter: &mut InterpreterContext, mut ast: Vec<&AST>) -> InterpreterResult<()> {
    if ast.is_empty() {
        return Err(InterpreterError::new(InterpreterErrorKind::EmptyImport));
    }
    let cur_file_id = ast[0].span().file_id;
    let total_span = ast.total_span().unwrap();

    let mut path = Vec::new();
    for ident in ast.drain(..) {
        let name = match ident {
            AST::Identifier(name, _) => name,
            e => {
                return Err(InterpreterError::spanned(
                    InterpreterErrorKind::InvalidInImport,
                    e.span(),
                ))?
            }
        };
        path.push(name.clone());
    }

    let mut file_path = interpreter
        .error_writer
        .id_to_path
        .get(&cur_file_id)
        .map(|p| {
            let mut p = p.clone();
            p.pop();
            p
        })
        .unwrap_or(env::current_dir().unwrap())
        .clone();

    file_path = path.iter().fold(file_path, |l, r| l.join(r));
    file_path.set_extension("scm");

    if interpreter.error_writer.already_loaded(&file_path) {
        return Ok(());
    }

    let mut file = File::open(file_path.clone()).map_err(|_| {
        InterpreterError::spanned(
            InterpreterErrorKind::ImportNotFound(file_path.to_str().unwrap().to_string()),
            total_span,
        )
    })?;
    let mut contents = String::new();
    file.read_to_string(&mut contents).unwrap();

    let id = interpreter
        .error_writer
        .add_file(file_path, contents.clone());

    let ast = LexerParser::from_string(id, contents, &interpreter.error_writer).map_err(|_| {
        InterpreterError::spanned(InterpreterErrorKind::ErrorInParsingImport, total_span)
    })?;

    for node in ast {
        interpreter.interpret(&node)?;
    }

    Ok(())
}

pub fn if_macro(
    interpreter: &mut InterpreterContext,
    mut ast: Vec<&AST>,
) -> InterpreterResult<*const AST> {
    if ast.len() != 3 {
        Err(InterpreterError::spanned(
            InterpreterErrorKind::ExpectedNParams {
                expected: 3,
                received: ast.len(),
            },
            {
                if let Some(span) = ast
                    .iter()
                    .skip(3)
                    .map(|l| l.span())
                    .reduce(|l, r| l.max_span(r))
                {
                    span
                } else {
                    ast.last().unwrap().span()
                }
            },
        ))?
    }

    let mut drain = ast.drain(..);
    let cond = drain.next().unwrap();

    interpreter.interpret(cond)?;
    let cond = interpreter.pop_data()?;
    let result = match cond.deref(interpreter)? {
        ObjectRef::Value(Literal::Boolean(false)) => false,
        _ => true,
    };

    if result {
        Ok(drain.next().unwrap())
    } else {
        Ok(drain.nth(1).unwrap())
    }
}

pub fn define(interpreter: &mut InterpreterContext, mut ast: Vec<&AST>) -> InterpreterResult<()> {
    if ast.len() > 2 || ast.is_empty() {
        return Err(InterpreterError::new(
            InterpreterErrorKind::ExpectedNParams {
                expected: 2,
                received: ast.len(),
            },
        ));
    }

    let mut ast = ast.drain(..);
    match ast.next().unwrap() {
        // Define a value
        AST::Identifier(ident, _) => {
            interpreter.interpret(ast.next().unwrap())?;
            let p = interpreter.pop_data()?;
            p.heap_alloc_named(ident, interpreter)?;
        }
        // Define a function
        AST::Operation(op_name, op_params, _) => {
            let AST::Identifier(op_name, _) = &**op_name else {
                return Err(InterpreterError::new(InterpreterErrorKind::CannotCall(
                    op_name.to_string(),
                )));
            };

            let mut param_names = Vec::new();
            for p in op_params.iter() {
                match p {
                    AST::Identifier(ident, _) => param_names.push(ident.clone()),
                    AST::Literal(_, span) | AST::StringLiteral(_, span) => {
                        return Err(InterpreterError::spanned(
                            InterpreterErrorKind::InvalidFuncParamNames,
                            *span,
                        ))
                    }
                    _ => {
                        todo!()
                    }
                }
            }

            HeapObject::Func(Func::Defined(
                Some(op_name.clone()),
                param_names,
                ast.next().unwrap().clone(),
            ))
            .heap_alloc_named(op_name, interpreter)?;
        }
        e => {
            return Err(InterpreterError::new(InterpreterErrorKind::CannotCall(
                e.to_string(),
            )))
        }
    };
    Ok(())
}

pub fn lambda(interpreter: &mut InterpreterContext, mut ast: Vec<&AST>) -> InterpreterResult<()> {
    if ast.len() > 2 || ast.is_empty() {
        return Err(InterpreterError::new(
            InterpreterErrorKind::ExpectedNParams {
                expected: 2,
                received: ast.len(),
            },
        ));
    }

    let mut ast = ast.drain(..);
    let param_names = match ast.next().unwrap() {
        AST::Identifier(ident, _) => {
            vec![ident.clone()]
        }
        AST::Operation(op_name, op_params, _) => {
            let build_err = |ast: &AST| -> Result<(), InterpreterError> {
                Err(InterpreterError::spanned(
                    InterpreterErrorKind::IsNotParamName(ast.to_string()),
                    ast.span(),
                ))
            };

            let AST::Identifier(op_name, _) = &**op_name else {
                return build_err(op_name);
            };

            let mut param_names = vec![op_name.clone()];
            for p in op_params.iter() {
                match p {
                    AST::Identifier(ident, _) => param_names.push(ident.clone()),
                    e => build_err(e)?,
                }
            }

            param_names
        }
        AST::EmptyList(_) => Vec::new(),
        e => {
            return Err(InterpreterError::new(InterpreterErrorKind::CannotCall(
                e.to_string(),
            )))
        }
    };

    let obj = UnallocatedObject::Func(Func::Defined(
        None,
        param_names,
        ast.next().unwrap().clone(),
    ))
    .stack_alloc(interpreter)?;

    interpreter.push_data(obj);

    Ok(())
}

pub fn write(interpreter: &mut InterpreterContext, n: usize) -> InterpreterResult<()> {
    let mut data = Vec::new();
    for _ in 0..n {
        data.push(interpreter.pop_data()?);
    }
    data.reverse();
    for d in data {
        print!("{} ", d.deref(interpreter)?);
    }
    println!();

    Ok(())
}

macro_rules! bin_op {
    ($name:ident, $l:ident, $r:ident, $calc:expr) => {
        pub fn $name(interpreter: &mut InterpreterContext, n: usize) -> InterpreterResult<()> {
            let mut objs = Vec::new();
            for _ in 0..n {
                objs.push(interpreter.pop_data()?);
            }
            objs.reverse();

            let drain = objs.drain(..);
            let out = drain.fold(None, |out, obj| {
                match (out, obj.deref(interpreter).unwrap()) {
                    (None, v) => Some(v.clone_to_unallocated()),
                    (
                        Some(UnallocatedObject::Value(Literal::Numeric($l))),
                        ObjectRef::Value(Literal::Numeric($r)),
                    ) => Some(UnallocatedObject::Value(Literal::Numeric($calc))),
                    _ => panic!(),
                }
            });

            let stack_obj = out
                .ok_or(InterpreterError::new(InterpreterErrorKind::FailedOperation))?
                .stack_alloc(interpreter)?;

            interpreter.push_data(stack_obj);

            return Ok(());
        }
    };
}

bin_op!(add, l, r, l + r);
bin_op!(sub, l, r, l - r);
bin_op!(mul, l, r, l * r);
bin_op!(div, l, r, l / r);

macro_rules! cmp_op {
    ($name:ident, $l:ident, $r:ident, $calc:expr) => {
        pub fn $name(interpreter: &mut InterpreterContext, n: usize) -> InterpreterResult<()> {
            let mut objs = Vec::new();
            for _ in 0..n {
                objs.push(interpreter.pop_data()?);
            }
            objs.reverse();

            let drain = objs.windows(2);
            let out = drain.fold(Ok(true), |out, objs| match out {
                Ok(out) => Ok(out && {
                    let $l = objs[0].deref(interpreter)?;
                    let $r = objs[1].deref(interpreter)?;
                    $calc
                }),
                e => e,
            });
            interpreter.push_data(StackObject::Value(Literal::Boolean(out?)));

            Ok(())
        }
    };
}

cmp_op!(eq, l, r, l == r);
cmp_op!(lt, l, r, l < r);
cmp_op!(lteq, l, r, l <= r);
cmp_op!(gt, l, r, l > r);
cmp_op!(gteq, l, r, l >= r);

pub fn car(interpreter: &mut InterpreterContext, n: usize) -> InterpreterResult<()> {
    if n != 1 {
        return Err(InterpreterError::new(
            InterpreterErrorKind::ExpectedNParams {
                expected: 1,
                received: n,
            },
        ));
    }

    let list = interpreter.pop_data()?;
    match list {
        StackObject::Value(_) => interpreter.push_data(list),
        StackObject::Ref(p) => match p {
            ObjectPointer::Null => {
                return Err(InterpreterError::new(InterpreterErrorKind::NullDeref))
            }
            ObjectPointer::Stack(i, p) => {
                interpreter.push_data(
                    *interpreter
                        .frame_stack
                        .get(i)
                        .and_then(|f| f.get_local_by_index(p))
                        .ok_or(InterpreterError::new(InterpreterErrorKind::NullDeref))?,
                );
            }
            ObjectPointer::Heap(p) => {
                match interpreter
                    .heap
                    .get(p)
                    .and_then(|x| x.as_ref())
                    .ok_or(InterpreterError::new(InterpreterErrorKind::NullDeref))?
                {
                    HeapObject::List(h, _) => {
                        let p = h.stack_alloc(interpreter)?;
                        interpreter.push_data(p);
                    }
                    _ => todo!(),
                }
            }
        },
    }
    Ok(())
}

pub fn cdr(interpreter: &mut InterpreterContext, n: usize) -> InterpreterResult<()> {
    if n != 1 {
        return Err(InterpreterError::new(
            InterpreterErrorKind::ExpectedNParams {
                expected: 1,
                received: n,
            },
        ));
    }

    let list = interpreter.pop_data()?;
    match list {
        StackObject::Value(_) => {
            return Err(InterpreterError::new(InterpreterErrorKind::ExpectedList))
        }
        StackObject::Ref(p) => match p {
            ObjectPointer::Null => {
                return Err(InterpreterError::new(InterpreterErrorKind::NullDeref))
            }
            ObjectPointer::Stack(_, _) => todo!(),
            ObjectPointer::Heap(p) => {
                match interpreter
                    .heap
                    .get(p)
                    .and_then(|x| x.as_ref())
                    .ok_or(InterpreterError::new(InterpreterErrorKind::NullDeref))?
                {
                    HeapObject::List(_, t) => {
                        let p = t.stack_alloc(interpreter)?;
                        interpreter.push_data(p);
                    }
                    _ => todo!(),
                }
            }
        },
    }
    Ok(())
}
