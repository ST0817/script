use chumsky::{Parser, extra::Err, text};

use crate::Error;

pub enum Expr {
    Int(u64),
}

fn int<'src>() -> impl Parser<'src, &'src str, u64, Err<Error<'src>>> {
    text::int(10).from_str().unwrapped()
}

fn int_expr<'src>() -> impl Parser<'src, &'src str, Expr, Err<Error<'src>>> {
    int().map(Expr::Int)
}

pub fn expr<'src>() -> impl Parser<'src, &'src str, Expr, Err<Error<'src>>> {
    int_expr()
}
