use std::{
    collections::HashMap,
    fmt::{self, Display, Formatter},
    iter::zip,
};

use chumsky::span::{SimpleSpan, SpanWrap, Spanned};
use indexmap::IndexMap;
use inkwell::{
    AddressSpace, OptimizationLevel,
    builder::Builder,
    context::Context,
    execution_engine::{ExecutionEngine, JitFunction},
    module::Module,
    types::{BasicType, BasicTypeEnum},
    values::{BasicValue, BasicValueEnum, PointerValue},
};

use crate::{
    Error, Result,
    parse::{Def, Expr, Field, Name, Param, Ref, Type},
};

#[derive(Clone, PartialEq)]
enum RawType {
    Int,
    Unit,
    FunPtr(Vec<Self>, Box<Self>),
    Struct(String, IndexMap<String, Self>),
}

impl Display for RawType {
    fn fmt(&self, f: &mut Formatter) -> fmt::Result {
        match self {
            Self::Int => write!(f, "Int"),
            Self::Unit => write!(f, "Unit"),
            Self::FunPtr(params, body) => write!(
                f,
                "fun({}): {}",
                params
                    .iter()
                    .map(|param| param.to_string())
                    .collect::<Vec<_>>()
                    .join(", "),
                body
            ),
            Self::Struct(name, _) => write!(f, "{name}"),
        }
    }
}

#[derive()]
struct RawValue<'ctx> {
    value: BasicValueEnum<'ctx>,
    raw_type: RawType,
}

#[derive(Clone)]
struct RawRef<'ctx> {
    ptr_value: PointerValue<'ctx>,
    raw_type: RawType,
}

type Scope<'ctx> = HashMap<String, RawRef<'ctx>>;

pub struct Compiler<'ctx> {
    context: &'ctx Context,
    module: Module<'ctx>,
    builder: Builder<'ctx>,
    execution_engine: ExecutionEngine<'ctx>,
    globals: Scope<'ctx>,
    types: HashMap<String, RawType>,
}

impl<'ctx> Compiler<'ctx> {
    pub fn new(context: &'ctx Context) -> Self {
        let module = context.create_module("main");
        let builder = context.create_builder();
        let execution_engine = module
            .create_jit_execution_engine(OptimizationLevel::Default)
            .unwrap();
        Self {
            context,
            module,
            builder,
            execution_engine,
            globals: HashMap::new(),
            types: HashMap::new(),
        }
    }

    fn raw_type_of<'src>(&self, ty: &Type<'src>) -> Result<'src, RawType> {
        match ty {
            Type::Int => Ok(RawType::Int),
            Type::Unit => Ok(RawType::Unit),
            Type::Named(name) => self
                .types
                .get(name.inner)
                .cloned()
                .ok_or_else(|| vec![Error::custom(name.span, "undefined type")]),
            Type::Fun(params, body) => Ok(RawType::FunPtr(
                params
                    .iter()
                    .map(|param| self.raw_type_of(param))
                    .collect::<Result<_>>()?,
                Box::new(self.raw_type_of(body)?),
            )),
        }
    }

    fn llvm_type_of(&self, raw_type: &RawType) -> BasicTypeEnum<'ctx> {
        match raw_type {
            RawType::Int => self.context.i64_type().into(),
            RawType::Unit => self.context.struct_type(&[], false).into(),
            RawType::FunPtr(..) => self.context.ptr_type(AddressSpace::default()).into(),
            RawType::Struct(_, fields) => self
                .context
                .struct_type(
                    &fields
                        .iter()
                        .map(|(_, ty)| self.llvm_type_of(ty))
                        .collect::<Vec<_>>()[..],
                    false,
                )
                .into(),
        }
    }

    fn raw_unit_value(&self) -> RawValue<'ctx> {
        RawValue {
            value: self.context.const_struct(&[], false).into(),
            raw_type: RawType::Unit,
        }
    }

    fn check_type<'src>(
        &self,
        type1: &RawType,
        type2: &RawType,
        span: &SimpleSpan,
    ) -> Result<'src, ()> {
        if type1 != type2 {
            return Err(vec![Error::custom(
                *span,
                format!("type mismatch {type1} and {type2}"),
            )]);
        }
        Ok(())
    }

    fn define_local<'src>(
        &mut self,
        name: &Name<'src>,
        raw_ref: RawRef<'ctx>,
        locals: &mut Scope<'ctx>,
    ) -> Result<'src, ()> {
        if locals.contains_key(name.inner) {
            return Err(vec![Error::custom(name.span, "variable redefinition")]);
        }
        locals.insert(name.inner.to_string(), raw_ref);
        Ok(())
    }

    fn get_local<'src, 'scope>(
        &self,
        name: &Name<'src>,
        locals: &'scope Scope<'ctx>,
    ) -> Result<'src, &'scope RawRef<'ctx>> {
        locals
            .get(name.inner)
            .ok_or_else(|| vec![Error::custom(name.span, "undefined variable")])
    }

    fn define_global<'src>(
        &mut self,
        name: &Name<'src>,
        raw_ref: RawRef<'ctx>,
    ) -> Result<'src, ()> {
        if self.globals.contains_key(name.inner) {
            return Err(vec![Error::custom(
                name.span,
                "global variable redefinition",
            )]);
        }
        self.globals.insert(name.inner.to_string(), raw_ref);
        Ok(())
    }

    fn get_global<'src, 'scope>(
        &'scope self,
        name: &Name<'src>,
    ) -> Result<'src, &'scope RawRef<'ctx>> {
        self.globals
            .get(name.inner)
            .ok_or_else(|| vec![Error::custom(name.span, "undefined global variable")])
    }

    fn declare_printf(&mut self) {
        let ty = self.context.void_type().fn_type(
            &[self.context.ptr_type(AddressSpace::default()).into()],
            true,
        );
        self.module.add_function("printf", ty, None);
    }

    fn compile_var_ref<'src>(
        &mut self,
        name: &Name<'src>,
        locals: &mut Scope<'ctx>,
    ) -> Result<'src, RawRef<'ctx>> {
        self.get_local(name, locals).cloned()
    }

    fn compile_access_ref<'src>(
        &mut self,
        reference: &Spanned<Box<Ref<'src>>>,
        field_name: &Name<'src>,
        locals: &mut Scope<'ctx>,
    ) -> Result<'src, RawRef<'ctx>> {
        let raw_ref = self.compile_ref(&reference, locals)?;
        let RawType::Struct(_, rraw_fialds) = &raw_ref.raw_type else {
            return Err(vec![Error::custom(reference.span, "not an struct")]);
        };
        let (index, _, raw_field_type) = rraw_fialds
            .get_full(field_name.inner)
            .ok_or_else(|| vec![Error::custom(field_name.span, "no field")])?;
        let field_ptr_value = self
            .builder
            .build_struct_gep(
                self.llvm_type_of(&raw_ref.raw_type),
                raw_ref.ptr_value,
                index as u32,
                "struct_ref",
            )
            .unwrap();
        Ok(RawRef {
            ptr_value: field_ptr_value,
            raw_type: raw_field_type.clone(),
        })
    }

    fn compile_ref<'src>(
        &mut self,
        reference: &Ref<'src>,
        locals: &mut Scope<'ctx>,
    ) -> Result<'src, RawRef<'ctx>> {
        match reference {
            Ref::Var(name) => self.compile_var_ref(name, locals),
            Ref::Access(reference, field_name) => {
                self.compile_access_ref(reference, field_name, locals)
            }
        }
    }

    fn compile_int_expr<'src>(&mut self, value: &u64) -> Result<'src, RawValue<'ctx>> {
        Ok(RawValue {
            value: self.context.i64_type().const_int(*value, false).into(),
            raw_type: RawType::Int,
        })
    }

    fn compile_global<'src>(&mut self, name: &Name<'src>) -> Result<'src, RawValue<'ctx>> {
        self.get_global(name).cloned().map(|raw_ref| RawValue {
            value: raw_ref.ptr_value.as_basic_value_enum(),
            raw_type: raw_ref.raw_type,
        })
    }

    fn compile_var_expr<'src>(
        &mut self,
        name: &Name<'src>,
        ty: &Type<'src>,
        locals: &mut Scope<'ctx>,
    ) -> Result<'src, RawValue<'ctx>> {
        let raw_type = self.raw_type_of(ty)?;
        let ptr_value = self
            .builder
            .build_alloca(self.llvm_type_of(&self.raw_type_of(ty)?), name.inner)
            .unwrap();
        let raw_ref = RawRef {
            ptr_value,
            raw_type,
        };
        self.define_local(name, raw_ref, locals)?;
        Ok(self.raw_unit_value())
    }

    fn compile_print_expr<'src>(
        &mut self,
        expr: &Spanned<Box<Expr<'src>>>,
        locals: &mut Scope<'ctx>,
    ) -> Result<'src, RawValue<'ctx>> {
        let printf = self.module.get_function("printf").unwrap();
        let raw_value = self.compile_expr(&expr.inner, locals)?;
        self.check_type(&raw_value.raw_type, &RawType::Int, &expr.span)?;
        let fmt = self.builder.build_global_string_ptr("%d\n", "fmt").unwrap();
        self.builder
            .build_call(
                printf,
                &[fmt.as_pointer_value().into(), raw_value.value.into()],
                "call",
            )
            .unwrap();
        Ok(self.raw_unit_value())
    }

    fn compile_assign_expr<'src>(
        &mut self,
        reference: &Ref<'src>,
        expr: &Spanned<Box<Expr<'src>>>,
        locals: &mut Scope<'ctx>,
    ) -> Result<'src, RawValue<'ctx>> {
        let raw_ref = self.compile_ref(reference, locals)?;
        let raw_value = self.compile_expr(&expr.inner, locals)?;
        self.check_type(&raw_ref.raw_type, &raw_value.raw_type, &expr.span)?;
        self.builder
            .build_store(raw_ref.ptr_value, raw_value.value)
            .unwrap();
        Ok(self.raw_unit_value())
    }

    fn compile_deref_expr<'src>(
        &mut self,
        reference: &Ref<'src>,
        locals: &mut Scope<'ctx>,
    ) -> Result<'src, RawValue<'ctx>> {
        let raw_ref = self.compile_ref(reference, locals)?;
        let value = self
            .builder
            .build_load(
                self.llvm_type_of(&raw_ref.raw_type),
                raw_ref.ptr_value,
                "deref",
            )
            .unwrap();
        Ok(RawValue {
            value,
            raw_type: raw_ref.raw_type,
        })
    }

    fn compile_call_expr<'src>(
        &mut self,
        callee: &Spanned<Box<Expr<'src>>>,
        args: &Spanned<Vec<Spanned<Expr<'src>>>>,
        locals: &mut Scope<'ctx>,
    ) -> Result<'src, RawValue<'ctx>> {
        let raw_callee_value = self.compile_expr(&callee.inner, locals)?;
        let RawType::FunPtr(raw_param_types, raw_return_type) = raw_callee_value.raw_type else {
            return Err(vec![Error::custom(callee.span, "not a function")]);
        };
        let (arg_values, raw_arg_types) = args.iter().try_fold(
            (Vec::new(), Vec::new()),
            |(mut arg_values, mut raw_arg_types), arg| {
                let raw_arg_value = self.compile_expr(&arg.inner, locals)?;
                arg_values.push(raw_arg_value.value.into());
                raw_arg_types.push(raw_arg_value.raw_type.with_span(arg.span));
                Ok((arg_values, raw_arg_types)) as Result<_>
            },
        )?;

        if raw_arg_types.len() != raw_param_types.len() {
            return Err(vec![Error::custom(
                args.span,
                "the number of arguments does not match",
            )]);
        }

        for (raw_arg_type, raw_param_type) in zip(&raw_arg_types, &raw_param_types) {
            self.check_type(&raw_arg_type.inner, raw_param_type, &raw_arg_type.span)?;
        }

        let llvm_param_types = raw_param_types
            .iter()
            .map(|raw_param_type| self.llvm_type_of(raw_param_type).into())
            .collect::<Vec<_>>();
        let fun_llvm_type = self
            .llvm_type_of(&raw_return_type)
            .fn_type(&llvm_param_types[..], false);
        let value = self
            .builder
            .build_indirect_call(
                fun_llvm_type,
                raw_callee_value.value.into_pointer_value(),
                &arg_values[..],
                "call",
            )
            .unwrap();
        Ok(RawValue {
            value: value.try_as_basic_value().unwrap_basic(),
            raw_type: *raw_return_type,
        })
    }

    fn compile_add_expr<'src>(
        &mut self,
        lhs: &Spanned<Box<Expr<'src>>>,
        rhs: &Spanned<Box<Expr<'src>>>,
        locals: &mut Scope<'ctx>,
    ) -> Result<'src, RawValue<'ctx>> {
        let raw_lhs_value = self.compile_expr(&lhs.inner, locals)?;
        let raw_rhs_value = self.compile_expr(&rhs.inner, locals)?;
        self.check_type(&raw_lhs_value.raw_type, &RawType::Int, &lhs.span)?;
        self.check_type(&raw_rhs_value.raw_type, &RawType::Int, &rhs.span)?;
        let value = self
            .builder
            .build_int_add(
                raw_lhs_value.value.into_int_value(),
                raw_rhs_value.value.into_int_value(),
                "add",
            )
            .unwrap();
        let raw_value = RawValue {
            value: value.into(),
            raw_type: RawType::Int,
        };
        Ok(raw_value)
    }

    fn compile_expr<'src>(
        &mut self,
        expr: &Expr<'src>,
        locals: &mut Scope<'ctx>,
    ) -> Result<'src, RawValue<'ctx>> {
        match expr {
            Expr::Int(value) => self.compile_int_expr(value),
            Expr::Global(name) => self.compile_global(name),
            Expr::Var(name, ty) => self.compile_var_expr(name, ty, locals),
            Expr::Print(expr) => self.compile_print_expr(expr, locals),
            Expr::Assign(reference, expr) => self.compile_assign_expr(reference, expr, locals),
            Expr::Deref(reference) => self.compile_deref_expr(reference, locals),
            Expr::Call(callee, args) => self.compile_call_expr(callee, args, locals),
            Expr::Add(lhs, rhs) => self.compile_add_expr(lhs, rhs, locals),
        }
    }

    fn compile_struct_def<'src>(
        &mut self,
        name: &Name<'src>,
        fields: &Vec<Field<'src>>,
    ) -> Result<'src, ()> {
        if self.types.contains_key(name.inner) {
            return Err(vec![Error::custom(name.span, "type redefinition")]);
        }
        let raw_fields = fields
            .iter()
            .try_fold(IndexMap::new(), |mut raw_fields, field| {
                if raw_fields.contains_key(field.name.inner) {
                    return Err(vec![Error::custom(field.name.span, "field redefinition")]);
                }
                raw_fields.insert(field.name.inner.to_string(), self.raw_type_of(&field.ty)?);
                Ok(raw_fields)
            })?;
        let raw_type = RawType::Struct(name.to_string(), raw_fields);
        self.types.insert(name.inner.to_string(), raw_type);
        Ok(())
    }

    fn compile_fun_def<'src>(
        &mut self,
        name: &Name<'src>,
        params: &Vec<Param<'src>>,
        return_type: &Type<'src>,
        body: &Spanned<Vec<Expr<'src>>>,
    ) -> Result<'src, ()> {
        let raw_param_types = params
            .iter()
            .map(|param| self.raw_type_of(&param.ty))
            .collect::<Result<Vec<_>>>()?;
        let llvm_param_types = raw_param_types
            .iter()
            .map(|param_type| self.llvm_type_of(param_type).into())
            .collect::<Vec<_>>();
        let raw_return_type = self.raw_type_of(return_type)?;

        let fun_type = self
            .llvm_type_of(&self.raw_type_of(return_type)?)
            .fn_type(&llvm_param_types[..], false);
        let fun = self.module.add_function(name.inner, fun_type, None);

        let raw_fun_ptr_type = RawType::FunPtr(raw_param_types, Box::new(raw_return_type.clone()));
        let raw_ref = RawRef {
            ptr_value: fun.as_global_value().as_pointer_value(),
            raw_type: raw_fun_ptr_type,
        };
        self.define_global(name, raw_ref)?;

        let mut locals = Scope::new();
        let block = self.context.append_basic_block(fun, "entry");
        self.builder.position_at_end(block);

        for (i, param) in params.iter().enumerate() {
            let raw_type = self.raw_type_of(&param.ty)?;
            let ptr_value = self
                .builder
                .build_alloca(self.llvm_type_of(&raw_type), "param")
                .unwrap();
            let raw_ref = RawRef {
                ptr_value,
                raw_type,
            };
            self.define_local(&param.name, raw_ref, &mut locals)?;

            let value = fun.get_nth_param(i as u32).unwrap();
            self.builder.build_store(ptr_value, value).unwrap();
        }

        let raw_value = body.iter().try_fold(self.raw_unit_value(), |_, expr| {
            self.compile_expr(expr, &mut locals)
        })?;

        self.check_type(&raw_value.raw_type, &raw_return_type, &body.span)?;
        self.builder.build_return(Some(&raw_value.value)).unwrap();

        Ok(())
    }

    fn compile_def<'src>(&mut self, def: &Def<'src>) -> Result<'src, ()> {
        match def {
            Def::Struct(name, fields) => self.compile_struct_def(name, fields),
            Def::Fun(name, params, return_type, body) => {
                self.compile_fun_def(name, params, return_type, body)
            }
        }
    }

    pub fn compile<'src>(
        &mut self,
        defs: &Vec<Def<'src>>,
    ) -> Result<'src, JitFunction<'ctx, unsafe extern "C" fn()>> {
        self.declare_printf();

        for def in defs {
            self.compile_def(def)?;
        }

        let raw_main_ref = self.get_global(&"main".with_span(SimpleSpan::default()))?;
        let raw_main_type = &raw_main_ref.raw_type;
        self.check_type(
            raw_main_type,
            &RawType::FunPtr(Vec::new(), Box::new(RawType::Unit)),
            &SimpleSpan::default(),
        )?;
        self.module.print_to_stderr();
        unsafe { Ok(self.execution_engine.get_function("main").unwrap()) }
    }
}
