use chumsky::{
    IterParser, Parser,
    extra::Err,
    prelude::{any, choice, just, recursive},
    span::{SimpleSpan, SpanWrap, Spanned},
    text::{self, ascii::keyword},
};

use crate::Error;

pub type Name<'src> = Spanned<&'src str>;

#[derive(Debug)]
pub enum Type<'src> {
    Int,
    Unit,
    Named(Name<'src>),
    Fun(Vec<Self>, Box<Self>),
}

#[derive(Debug)]
pub enum Ref<'src> {
    Var(Name<'src>),
    Access(Spanned<Box<Self>>, Name<'src>),
}

#[derive(Debug)]
pub enum Expr<'src> {
    Int(u64),
    Global(Name<'src>),
    Var(Name<'src>, Type<'src>),
    Print(Spanned<Box<Self>>),
    Assign(Ref<'src>, Spanned<Box<Self>>),
    Deref(Ref<'src>),
    Call(Spanned<Box<Self>>, Spanned<Vec<Spanned<Self>>>),
    Add(Spanned<Box<Self>>, Spanned<Box<Self>>),
}

#[derive(Debug)]
pub struct Field<'src> {
    pub name: Name<'src>,
    pub ty: Type<'src>,
}

#[derive(Debug)]
pub struct Param<'src> {
    pub name: Name<'src>,
    pub ty: Type<'src>,
}

#[derive(Debug)]
pub enum Def<'src> {
    Struct(Name<'src>, Vec<Field<'src>>),
    Fun(
        Name<'src>,
        Vec<Param<'src>>,
        Type<'src>,
        Spanned<Vec<Expr<'src>>>,
    ),
}

// Parens parser := "(" parser ")"
fn parens<'src, T>(
    parser: impl Parser<'src, &'src str, T, Err<Error<'src>>> + Clone,
) -> impl Parser<'src, &'src str, T, Err<Error<'src>>> + Clone {
    parser.delimited_by(just('(').padded(), just(')'))
}

// ParensComma parser := Parens ( parser ( "," parser )* )?
fn parens_comma<'src, T>(
    parser: impl Parser<'src, &'src str, T, Err<Error<'src>>> + Clone,
) -> impl Parser<'src, &'src str, Vec<T>, Err<Error<'src>>> + Clone {
    parens(parser.padded().separated_by(just(',').padded()).collect())
}

// Braces parser := "{" parser "}"
fn braces<'src, T>(
    parser: impl Parser<'src, &'src str, T, Err<Error<'src>>>,
) -> impl Parser<'src, &'src str, T, Err<Error<'src>>> {
    parser.delimited_by(just('{').padded(), just('}'))
}

// Int := [0-9]+
fn int<'src>() -> impl Parser<'src, &'src str, u64, Err<Error<'src>>> + Clone {
    text::int(10).from_str().unwrapped()
}

// Upper := [A-Z]
fn upper<'src>() -> impl Parser<'src, &'src str, char, Err<Error<'src>>> + Clone {
    any().filter(char::is_ascii_uppercase).labelled("uppercase")
}

// Lower := [a-z]
fn lower<'src>() -> impl Parser<'src, &'src str, char, Err<Error<'src>>> + Clone {
    any().filter(char::is_ascii_lowercase).labelled("lowercase")
}

// Alphanum := [a-zA-Z0-9]
fn alphanum<'src>() -> impl Parser<'src, &'src str, char, Err<Error<'src>>> + Clone {
    any()
        .filter(char::is_ascii_alphanumeric)
        .labelled("alphanumeric")
}

// Name first := first Alphanum* "'"*
fn name<'src>(
    first: impl Parser<'src, &'src str, char, Err<Error<'src>>> + Clone,
) -> impl Parser<'src, &'src str, Name<'src>, Err<Error<'src>>> + Clone {
    first
        .ignore_then(alphanum().repeated())
        .ignore_then(just("'").repeated())
        .to_slice()
        .spanned()
}

// VarName := Name Lower
fn var_name<'src>() -> impl Parser<'src, &'src str, Name<'src>, Err<Error<'src>>> + Clone {
    name(lower())
}

// TypeName := Name Upper
fn type_name<'src>() -> impl Parser<'src, &'src str, Name<'src>, Err<Error<'src>>> + Clone {
    name(upper())
}

// IntType := "Int"
fn int_type<'src>() -> impl Parser<'src, &'src str, Type<'src>, Err<Error<'src>>> + Clone {
    keyword("Int").map(|_| Type::Int)
}

// UnitType := "Unit"
fn unit_type<'src>() -> impl Parser<'src, &'src str, Type<'src>, Err<Error<'src>>> + Clone {
    keyword("Unit").map(|_| Type::Unit)
}

// NamedType := TypeName
fn named_type<'src>() -> impl Parser<'src, &'src str, Type<'src>, Err<Error<'src>>> + Clone {
    type_name().map(Type::Named)
}

// FunType := "fun" (ParensComma Type) ":" Type
fn fun_type<'src>(
    ty: impl Parser<'src, &'src str, Type<'src>, Err<Error<'src>>> + Clone,
) -> impl Parser<'src, &'src str, Type<'src>, Err<Error<'src>>> + Clone {
    keyword("fun")
        .padded()
        .ignore_then(parens_comma(ty.clone()))
        .padded()
        .then_ignore(just(':'))
        .padded()
        .then(ty.map(Box::new))
        .map(|(param_types, return_type)| Type::Fun(param_types, return_type))
}

// Type
//   := IntType
//    | UnitType
//    | FunType
fn ty<'src>() -> impl Parser<'src, &'src str, Type<'src>, Err<Error<'src>>> + Clone {
    recursive(|ty| choice((int_type(), unit_type(), named_type(), fun_type(ty))))
}

// VarRef := VarName
fn var_ref<'src>() -> impl Parser<'src, &'src str, Ref<'src>, Err<Error<'src>>> + Clone {
    var_name().map(Ref::Var)
}

// AccessRef := Ref "." VarName
fn access_ref<'src>(
    reference: impl Parser<'src, &'src str, Ref<'src>, Err<Error<'src>>> + Clone,
) -> impl Parser<'src, &'src str, Ref<'src>, Err<Error<'src>>> + Clone {
    reference
        .spanned()
        .padded()
        .foldl(
            just('.')
                .padded()
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

// Ref
//   := VarRef
//    | AccessRef
fn reference<'src>() -> impl Parser<'src, &'src str, Ref<'src>, Err<Error<'src>>> + Clone {
    let reference = var_ref();
    access_ref(reference)
}

// IntExpr := Int
fn int_expr<'src>() -> impl Parser<'src, &'src str, Expr<'src>, Err<Error<'src>>> + Clone {
    int().map(Expr::Int)
}

// GlobalExpr := "@" VarName
fn global_expr<'src>() -> impl Parser<'src, &'src str, Expr<'src>, Err<Error<'src>>> + Clone {
    just('@').padded().ignore_then(var_name()).map(Expr::Global)
}

// VarExpr := "var" VaeName ":" Type
fn var_expr<'src>() -> impl Parser<'src, &'src str, Expr<'src>, Err<Error<'src>>> + Clone {
    keyword("var")
        .padded()
        .ignore_then(var_name())
        .padded()
        .then_ignore(just(':'))
        .padded()
        .then(ty())
        .map(|(name, ty)| Expr::Var(name, ty))
}

// PrintExpr := "print" Expr
fn print_expr<'src>(
    expr: impl Parser<'src, &'src str, Expr<'src>, Err<Error<'src>>> + Clone,
) -> impl Parser<'src, &'src str, Expr<'src>, Err<Error<'src>>> + Clone {
    keyword("print")
        .padded()
        .ignore_then(expr.map(Box::new).spanned())
        .map(Expr::Print)
}

// AssignExpr := Ref "<-" Expr
fn assign_expr<'src>(
    expr: impl Parser<'src, &'src str, Expr<'src>, Err<Error<'src>>> + Clone,
) -> impl Parser<'src, &'src str, Expr<'src>, Err<Error<'src>>> + Clone {
    reference()
        .padded()
        .then_ignore(just("<-"))
        .padded()
        .then(expr.map(Box::new).spanned())
        .map(|(reference, expr)| Expr::Assign(reference, expr))
}

// DerefExpr := Ref
fn deref_expr<'src>() -> impl Parser<'src, &'src str, Expr<'src>, Err<Error<'src>>> + Clone {
    reference().map(Expr::Deref)
}

// CallExpr := Expr (ParensComma Expr)
fn call_expr<'src>(
    expr: impl Parser<'src, &'src str, Expr<'src>, Err<Error<'src>>> + Clone,
) -> impl Parser<'src, &'src str, Expr<'src>, Err<Error<'src>>> + Clone {
    expr.clone()
        .spanned()
        .padded()
        .foldl(
            parens_comma(expr.spanned().padded()).spanned().repeated(),
            |callee, args| {
                let span: SimpleSpan = (callee.span.start..args.span.end).into();
                Expr::Call(Box::new(callee.inner).with_span(callee.span), args).with_span(span)
            },
        )
        .map(|spanned| spanned.inner)
}

fn add_expr<'src>(
    expr: impl Parser<'src, &'src str, Expr<'src>, Err<Error<'src>>> + Clone,
) -> impl Parser<'src, &'src str, Expr<'src>, Err<Error<'src>>> + Clone {
    expr.clone()
        .spanned()
        .padded()
        .foldl(
            just('+')
                .padded()
                .ignore_then(expr)
                .spanned()
                .padded()
                .repeated(),
            |lhs, rhs| {
                let span: SimpleSpan = (lhs.span.start..rhs.span.end).into();
                Expr::Add(
                    Box::new(lhs.inner).with_span(lhs.span),
                    Box::new(rhs.inner).with_span(rhs.span),
                )
                .with_span(span)
            },
        )
        .map(|spanned| spanned.inner)
}

// Expr
//   := IntExpr
//    | GlobalExpr
//    | VarExpr
//    | PrintExpr
//    | AssignExpr
//    | DerefExpr
//    | CallExpr
//    | Parens Expr
fn expr<'src>() -> impl Parser<'src, &'src str, Expr<'src>, Err<Error<'src>>> {
    recursive(|expr| {
        let atom = choice((
            int_expr(),
            global_expr(),
            var_expr(),
            print_expr(expr.clone()),
            assign_expr(expr.clone()),
            deref_expr(),
            parens(expr),
        ));
        let expr = call_expr(atom);
        add_expr(expr)
    })
}

// Field := VarName ":" Type ";"
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

// StructDef := "struct" (Braces Field*)
fn struct_def<'src>() -> impl Parser<'src, &'src str, Def<'src>, Err<Error<'src>>> {
    keyword("struct")
        .padded()
        .ignore_then(type_name())
        .padded()
        .then(braces(field().padded().repeated().collect()))
        .map(|(name, fileds)| Def::Struct(name, fileds))
}

// Param := VarName ":" Type
fn param<'src>() -> impl Parser<'src, &'src str, Param<'src>, Err<Error<'src>>> + Clone {
    var_name()
        .padded()
        .then_ignore(just(':'))
        .padded()
        .then(ty())
        .map(|(name, ty)| Param { name, ty })
}

// FunBody := ":=" Expr | Braces ( Expr ( ";" Expr )* )?
// FunDef := "fun" (ParensComma Field) ":" Type FunBody
fn fun_def<'src>() -> impl Parser<'src, &'src str, Def<'src>, Err<Error<'src>>> {
    keyword("fun")
        .padded()
        .ignore_then(var_name())
        .padded()
        .then(parens_comma(param()))
        .padded()
        .then_ignore(just(':'))
        .padded()
        .then(ty())
        .padded()
        .then(choice((
            just(":=")
                .padded()
                .ignore_then(expr())
                .map(|body| vec![body])
                .spanned(),
            braces(expr().padded().separated_by(just(';').padded()).collect()).spanned(),
        )))
        .map(|(((name, params), return_type), body)| Def::Fun(name, params, return_type, body))
}

// Def
//   := StructDef
//    | FunDef
fn def<'src>() -> impl Parser<'src, &'src str, Def<'src>, Err<Error<'src>>> {
    choice((struct_def(), fun_def()))
}

// Program := Def*
pub fn program<'src>() -> impl Parser<'src, &'src str, Vec<Def<'src>>, Err<Error<'src>>> {
    def().padded().repeated().collect()
}
