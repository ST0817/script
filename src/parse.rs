use chumsky::{
    IterParser, Parser,
    extra::Err,
    prelude::{any, choice, just},
    span::Spanned,
    text::{self, ascii::keyword},
};

use crate::Error;

pub type Name<'src> = Spanned<&'src str>;

pub enum Expr<'src> {
    Int(u64),
    Var(Name<'src>),
}

pub enum Stmt<'src> {
    Var(Name<'src>),
    Print(Expr<'src>),
    Assign(Name<'src>, Expr<'src>),
}

fn int<'src>() -> impl Parser<'src, &'src str, u64, Err<Error<'src>>> {
    text::int(10).from_str().unwrapped()
}

fn alpha<'src>() -> impl Parser<'src, &'src str, char, Err<Error<'src>>> {
    any()
        .filter(char::is_ascii_alphabetic)
        .labelled("alphabetic")
}

fn alphanum<'src>() -> impl Parser<'src, &'src str, char, Err<Error<'src>>> {
    any()
        .filter(char::is_ascii_alphanumeric)
        .labelled("alphanumeric")
}

fn name<'src>() -> impl Parser<'src, &'src str, Name<'src>, Err<Error<'src>>> {
    alpha()
        .ignore_then(alphanum().repeated())
        .to_slice()
        .spanned()
}

fn int_expr<'src>() -> impl Parser<'src, &'src str, Expr<'src>, Err<Error<'src>>> {
    int().map(Expr::Int)
}

fn var_expr<'src>() -> impl Parser<'src, &'src str, Expr<'src>, Err<Error<'src>>> {
    name().map(Expr::Var)
}

fn expr<'src>() -> impl Parser<'src, &'src str, Expr<'src>, Err<Error<'src>>> {
    choice((int_expr(), var_expr()))
}

fn var_stmt<'src>() -> impl Parser<'src, &'src str, Stmt<'src>, Err<Error<'src>>> {
    keyword("var")
        .padded()
        .ignore_then(name())
        .then_ignore(just(';'))
        .map(Stmt::Var)
}

fn print_stmt<'src>() -> impl Parser<'src, &'src str, Stmt<'src>, Err<Error<'src>>> {
    keyword("print")
        .padded()
        .ignore_then(expr())
        .padded()
        .then_ignore(just(';'))
        .map(Stmt::Print)
}

fn assign_stmt<'src>() -> impl Parser<'src, &'src str, Stmt<'src>, Err<Error<'src>>> {
    name()
        .padded()
        .then_ignore(just("<-"))
        .padded()
        .then(expr())
        .padded()
        .then_ignore(just(';'))
        .map(|(name, expr)| Stmt::Assign(name, expr))
}

fn stmt<'src>() -> impl Parser<'src, &'src str, Stmt<'src>, Err<Error<'src>>> {
    choice((var_stmt(), print_stmt(), assign_stmt()))
}

pub fn stmts<'src>() -> impl Parser<'src, &'src str, Vec<Stmt<'src>>, Err<Error<'src>>> {
    stmt().repeated().collect().padded()
}
