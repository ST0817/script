mod compile;
mod parse;

use std::process::ExitCode;

use ariadne::{Color, Config, IndexType, Label, Report, ReportKind, Source};
use chumsky::{Parser, error::Rich};
use inkwell::context::Context;
use rustyline::{DefaultEditor, error::ReadlineError};

use crate::compile::Compiler;

const REPL_ID: &str = "REPL";

pub type Error<'src> = Rich<'src, char>;
pub type Result<'src, T> = std::result::Result<T, Vec<Error<'src>>>;

fn print_errors(errors: &Vec<Error>, id: &str, src: &str) {
    for error in errors {
        Report::build(ReportKind::Error, (id, error.span().into_range()))
            .with_config(Config::new().with_index_type(IndexType::Byte))
            .with_label(
                Label::new((id, error.span().into_range()))
                    .with_message(error.to_string())
                    .with_color(Color::Red),
            )
            .finish()
            .print((id, Source::from(src)))
            .unwrap();
    }
}

fn repl_process<'src>(src: &'src str, compiler: &mut Compiler) -> Result<'src, ()> {
    let expr = parse::expr().parse(src).into_result()?;
    let main = compiler.compile_expr(&expr);
    let result = unsafe { main.call() };
    println!("result: {result}");
    Ok(())
}

fn main() -> ExitCode {
    let mut editor = DefaultEditor::new().unwrap();
    let context = Context::create();
    let mut compiler = Compiler::new(&context);

    loop {
        match editor.readline("❯ ") {
            Ok(src) if !src.is_empty() => {
                editor.add_history_entry(&src).unwrap();

                if let Err(errors) = repl_process(&src, &mut compiler) {
                    print_errors(&errors, REPL_ID, &src);
                }
            }
            Ok(_) => {}
            Err(ReadlineError::Eof) => break ExitCode::SUCCESS,
            Err(error) => {
                eprintln!("{error}");
                break ExitCode::FAILURE;
            }
        }
    }
}
