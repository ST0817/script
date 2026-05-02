use std::process::ExitCode;

use rustyline::{DefaultEditor, error::ReadlineError};

fn main() -> ExitCode {
    let mut editor = DefaultEditor::new().unwrap();

    loop {
        match editor.readline("❯ ") {
            Ok(src) if !src.is_empty() => {
                println!("Got: {src}");
            }
            Ok(_) => {},
            Err(ReadlineError::Eof) => break ExitCode::SUCCESS,
            Err(error) => {
                eprintln!("{error}");
                break ExitCode::FAILURE;
            }
        }
    }

}
