use std::collections::HashMap;

use chumsky::span::SimpleSpan;
use indexmap::IndexMap;
use inkwell::{
    AddressSpace,
    builder::Builder,
    context::Context,
    execution_engine::{ExecutionEngine, JitFunction},
    module::Module,
    types::BasicTypeEnum,
    values::{BasicValueEnum, PointerValue},
};

use crate::{
    Error, Result,
    parse::{Expr, Ref, Stmt, Type},
};

#[derive(Clone, PartialEq)]
enum RawType {
    Int,
    Struct(IndexMap<String, Self>),
}

#[derive(Clone)]
struct RawRef<'ctx> {
    ptr: PointerValue<'ctx>,
    ty: RawType,
}

pub struct Compiler<'ctx> {
    context: &'ctx Context,
    scope: HashMap<String, RawRef<'ctx>>,
    types: HashMap<String, RawType>,
}

impl<'ctx> Compiler<'ctx> {
    pub fn new(context: &'ctx Context) -> Self {
        Self {
            context,
            scope: HashMap::new(),
            types: HashMap::new(),
        }
    }

    pub fn create_module(&self, name: &str) -> Module<'ctx> {
        self.context.create_module(name)
    }

    pub fn create_builder(&self) -> Builder<'ctx> {
        self.context.create_builder()
    }

    fn raw_type_of<'src>(&self, ty: &Type<'src>) -> Result<'src, RawType> {
        match ty {
            Type::Int => Ok(RawType::Int),
            Type::Named(name) => self
                .types
                .get(name.inner)
                .cloned()
                .ok_or_else(|| vec![Error::custom(name.span, "undefined type")]),
        }
    }

    fn llvm_type_of(&self, ty: &RawType) -> BasicTypeEnum<'ctx> {
        match ty {
            RawType::Int => self.context.i64_type().into(),
            RawType::Struct(fields) => self
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

    fn check_type<'src>(
        &self,
        type1: &RawType,
        type2: &RawType,
        span: &SimpleSpan,
    ) -> Result<'src, ()> {
        if type1 != type2 {
            return Err(vec![Error::custom(*span, "type mismatch")]);
        }
        Ok(())
    }

    fn declare_printf(&mut self, module: &Module<'ctx>) {
        let ty = self.context.void_type().fn_type(
            &[self.context.ptr_type(AddressSpace::default()).into()],
            true,
        );
        module.add_function("printf", ty, None);
    }

    fn compile_ref<'src>(
        &mut self,
        reference: &Ref<'src>,
        builder: &Builder<'ctx>,
    ) -> Result<'src, RawRef<'ctx>> {
        match reference {
            Ref::Var(name) => self
                .scope
                .get(name.inner)
                .cloned()
                .ok_or_else(|| vec![Error::custom(name.span, "undefined variable")]),
            Ref::Access(reference, field_name) => {
                let raw_ref = self.compile_ref(&reference, builder)?;
                let RawType::Struct(fields) = &raw_ref.ty else {
                    return Err(vec![Error::custom(reference.span, "not an struct")]);
                };
                let (index, _, field_type) = fields
                    .get_full(field_name.inner)
                    .ok_or_else(|| vec![Error::custom(field_name.span, "no field")])?;
                let field_ptr = builder
                    .build_struct_gep(
                        self.llvm_type_of(&raw_ref.ty),
                        raw_ref.ptr,
                        index as u32,
                        "struct_ref",
                    )
                    .unwrap();
                let field_ref = RawRef {
                    ptr: field_ptr,
                    ty: field_type.clone(),
                };
                Ok(field_ref)
            }
        }
    }

    fn compile_expr<'src>(
        &mut self,
        expr: &Expr<'src>,
        builder: &Builder<'ctx>,
    ) -> Result<'src, (BasicValueEnum<'ctx>, RawType)> {
        match expr {
            Expr::Int(value) => Ok((
                self.context.i64_type().const_int(*value, false).into(),
                RawType::Int,
            )),
            Expr::Deref(reference) => {
                let raw_ref = self.compile_ref(reference, builder)?;
                let value = builder
                    .build_load(self.llvm_type_of(&raw_ref.ty), raw_ref.ptr, "deref")
                    .unwrap();
                Ok((value, raw_ref.ty))
            }
        }
    }

    fn compile_stmt<'src>(
        &mut self,
        stmt: &Stmt<'src>,
        module: &Module<'ctx>,
        builder: &Builder<'ctx>,
    ) -> Result<'src, ()> {
        match stmt {
            Stmt::Var(name, ty) => {
                if self.scope.contains_key(name.inner) {
                    return Err(vec![Error::custom(name.span, "variable redefinition")]);
                };
                let raw_type = self.raw_type_of(ty)?;
                let ptr = builder
                    .build_alloca(self.llvm_type_of(&raw_type), name.inner)
                    .unwrap();
                let entry = RawRef { ptr, ty: raw_type };
                self.scope.insert(name.inner.to_string(), entry);
            }
            Stmt::Struct(name, fields) => {
                if self.types.contains_key(name.inner) {
                    return Err(vec![Error::custom(name.span, "type redefinition")]);
                }
                let type_fields =
                    fields
                        .iter()
                        .try_fold(IndexMap::new(), |mut type_fields, field| {
                            if type_fields.contains_key(field.name.inner) {
                                return Err(vec![Error::custom(
                                    field.name.span,
                                    "field redefinition",
                                )]);
                            }
                            type_fields
                                .insert(field.name.inner.to_string(), self.raw_type_of(&field.ty)?);
                            Ok(type_fields)
                        })?;
                let ty = RawType::Struct(type_fields);
                self.types.insert(name.inner.to_string(), ty);
            }
            Stmt::Print(expr) => {
                let printf = module.get_function("printf").unwrap();
                let (value, expr_type) = self.compile_expr(&expr.inner, builder)?;
                self.check_type(&expr_type, &RawType::Int, &expr.span)?;
                let fmt = builder.build_global_string_ptr("%d\n", "fmt").unwrap();
                builder
                    .build_call(
                        printf,
                        &[fmt.as_pointer_value().into(), value.into()],
                        "call",
                    )
                    .unwrap();
            }
            Stmt::Assign(reference, expr) => {
                let raw_ref = self.compile_ref(reference, builder)?;
                let (value, value_type) = self.compile_expr(&expr.inner, builder)?;
                self.check_type(&raw_ref.ty, &value_type, &expr.span)?;
                builder.build_store(raw_ref.ptr, value).unwrap();
            }
        }
        Ok(())
    }

    pub fn compile_stmts<'src>(
        &mut self,
        stmts: &Vec<Stmt<'src>>,
        module: &Module<'ctx>,
        builder: &Builder<'ctx>,
        execution_engine: &ExecutionEngine<'ctx>,
    ) -> Result<'src, JitFunction<'ctx, unsafe extern "C" fn()>> {
        self.declare_printf(&module);

        let main_type = self.context.i64_type().fn_type(&[], false);
        let main = module.add_function("main", main_type, None);
        let block = self.context.append_basic_block(main, "entry");
        builder.position_at_end(block);

        for stmt in stmts {
            self.compile_stmt(stmt, &module, &builder)?;
        }

        builder.build_return(None).unwrap();
        module.print_to_stderr();
        unsafe { Ok(execution_engine.get_function("main").unwrap()) }
    }
}
