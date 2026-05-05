use chumsky::{
    IterParser, Parser,
    extra::Err,
    prelude::{any, choice, just},
    span::{SimpleSpan, SpanWrap, Spanned},
    text::{self, ascii::keyword},
};

use crate::Error;

pub type Name<'src> = Spanned<&'src str>;

#[derive(Debug)]
pub enum Type<'src> {
    Int,
    Named(Name<'src>),
}

#[derive(Debug)]
pub enum Ref<'src> {
    Var(Name<'src>),
    Access(Spanned<Box<Self>>, Name<'src>),
}

#[derive(Debug)]
pub enum Expr<'src> {
    Int(u64),
    Deref(Ref<'src>),
}

#[derive(Debug)]
pub struct Field<'src> {
    pub name: Name<'src>,
    pub ty: Type<'src>,
}

#[derive(Debug)]
pub enum Stmt<'src> {
    Var(Name<'src>, Type<'src>),
    Struct(Name<'src>, Vec<Field<'src>>),
    Print(Spanned<Expr<'src>>),
    Assign(Ref<'src>, Spanned<Expr<'src>>),
}

fn int<'src>() -> impl Parser<'src, &'src str, u64, Err<Error<'src>>> {
    text::int(10).from_str().unwrapped()
}

fn upper<'src>() -> impl Parser<'src, &'src str, char, Err<Error<'src>>> {
    any().filter(char::is_ascii_uppercase).labelled("uppercase")
}

fn lower<'src>() -> impl Parser<'src, &'src str, char, Err<Error<'src>>> {
    any().filter(char::is_ascii_lowercase).labelled("lowercase")
}

fn alphanum<'src>() -> impl Parser<'src, &'src str, char, Err<Error<'src>>> {
    any()
        .filter(char::is_ascii_alphanumeric)
        .labelled("alphanumeric")
}

fn name<'src>(
    first: impl Parser<'src, &'src str, char, Err<Error<'src>>>,
) -> impl Parser<'src, &'src str, Name<'src>, Err<Error<'src>>> {
    first
        .ignore_then(alphanum().repeated())
        .ignore_then(just("'").repeated())
        .to_slice()
        .spanned()
}

fn var_name<'src>() -> impl Parser<'src, &'src str, Name<'src>, Err<Error<'src>>> {
    name(lower())
}

fn type_name<'src>() -> impl Parser<'src, &'src str, Name<'src>, Err<Error<'src>>> {
    name(upper())
}

fn int_type<'src>() -> impl Parser<'src, &'src str, Type<'src>, Err<Error<'src>>> {
    keyword("Int").map(|_| Type::Int)
}

fn named_type<'src>() -> impl Parser<'src, &'src str, Type<'src>, Err<Error<'src>>> {
    type_name().map(Type::Named)
}

fn ty<'src>() -> impl Parser<'src, &'src str, Type<'src>, Err<Error<'src>>> {
    choice((int_type(), named_type()))
}

fn access_ref<'src>(
    reference: impl Parser<'src, &'src str, Ref<'src>, Err<Error<'src>>>,
) -> impl Parser<'src, &'src str, Ref<'src>, Err<Error<'src>>> {
    reference
        .spanned()
        .padded()
        .foldl(
            just('.')
                .ignore_then(var_name())
                .spanned()
                .padded()
                .repeated(),
            |reference, field_name| {
                let span: SimpleSpan = (reference.span.start..field_name.span.end).into();
                Ref::Access(
                    Box::new(reference.inner).with_span(reference.span),
                    field_name.inner,
                )
                .with_span(span)
            },
        )
        .map(|spanned| spanned.inner)
}

fn var_ref<'src>() -> impl Parser<'src, &'src str, Ref<'src>, Err<Error<'src>>> {
    var_name().map(Ref::Var)
}

fn reference<'src>() -> impl Parser<'src, &'src str, Ref<'src>, Err<Error<'src>>> {
    let reference = var_ref();
    access_ref(reference)
}

fn int_expr<'src>() -> impl Parser<'src, &'src str, Expr<'src>, Err<Error<'src>>> {
    int().map(Expr::Int)
}

fn deref_expr<'src>() -> impl Parser<'src, &'src str, Expr<'src>, Err<Error<'src>>> {
    reference().map(Expr::Deref)
}

fn expr<'src>() -> impl Parser<'src, &'src str, Expr<'src>, Err<Error<'src>>> {
    choice((int_expr(), deref_expr()))
}

fn var_stmt<'src>() -> impl Parser<'src, &'src str, Stmt<'src>, Err<Error<'src>>> {
    keyword("var")
        .padded()
        .ignore_then(var_name())
        .padded()
        .then_ignore(just(':'))
        .padded()
        .then(ty())
        .padded()
        .then_ignore(just(';'))
        .map(|(name, ty)| Stmt::Var(name, ty))
}

fn field<'src>() -> impl Parser<'src, &'src str, Field<'src>, Err<Error<'src>>> {
    var_name()
        .padded()
        .then_ignore(just(':'))
        .padded()
        .then(ty())
        .padded()
        .then_ignore(just(';'))
        .map(|(name, ty)| Field { name, ty })
}

fn struct_stmt<'src>() -> impl Parser<'src, &'src str, Stmt<'src>, Err<Error<'src>>> {
    keyword("struct")
        .padded()
        .ignore_then(type_name())
        .padded()
        .then(
            field()
                .padded()
                .repeated()
                .collect()
                .delimited_by(just('{').padded(), just('}')),
        )
        .map(|(name, fileds)| Stmt::Struct(name, fileds))
}

fn print_stmt<'src>() -> impl Parser<'src, &'src str, Stmt<'src>, Err<Error<'src>>> {
    keyword("print")
        .padded()
        .ignore_then(expr().spanned())
        .padded()
        .then_ignore(just(';'))
        .map(Stmt::Print)
}

fn assign_stmt<'src>() -> impl Parser<'src, &'src str, Stmt<'src>, Err<Error<'src>>> {
    reference()
        .padded()
        .then_ignore(just("<-"))
        .padded()
        .then(expr().spanned())
        .padded()
        .then_ignore(just(';'))
        .map(|(reference, expr)| Stmt::Assign(reference, expr))
}

fn stmt<'src>() -> impl Parser<'src, &'src str, Stmt<'src>, Err<Error<'src>>> {
    choice((var_stmt(), struct_stmt(), print_stmt(), assign_stmt()))
}

pub fn stmts<'src>() -> impl Parser<'src, &'src str, Vec<Stmt<'src>>, Err<Error<'src>>> {
    stmt().padded().repeated().collect().padded()
}
