mod compile;
mod parse;

use std::{env::args, fs::read_to_string, process::ExitCode};

use ariadne::{Color, Config, IndexType, Label, Report, ReportKind, Source};
use chumsky::{Parser, error::Rich};
use inkwell::context::Context;

use crate::compile::Compiler;

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

fn run_file<'src>(src: &'src str) -> Result<'src, ()> {
    let defs = parse::program().parse(src).into_result()?;
    dbg!(&defs);
    let context = Context::create();
    let mut compiler = Compiler::new(&context);
    let main = compiler.compile(&defs)?;
    unsafe { main.call() }
    Ok(())
}

/*fn repl() -> ExitCode {
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
}*/

fn main() -> ExitCode {
    let [_, file_path] = &args().collect::<Vec<_>>()[..] else {
        eprintln!("Invalid arguments");
        return ExitCode::FAILURE;
    };
    let src = match read_to_string(file_path) {
        Ok(src) => src,
        Err(error) => {
            eprintln!("Failed to read file: {error}");
            return ExitCode::FAILURE;
        }
    };
    if let Err(errors) = run_file(&src) {
        print_errors(&errors, file_path, &src);
    }
    ExitCode::SUCCESS
}
