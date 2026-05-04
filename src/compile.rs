use std::collections::HashMap;

use inkwell::{
    AddressSpace,
    builder::Builder,
    context::Context,
    execution_engine::{ExecutionEngine, JitFunction},
    module::Module,
    values::{BasicValueEnum, PointerValue},
};

use crate::{
    Error, Result,
    parse::{Expr, Stmt},
};

pub struct Compiler<'ctx> {
    context: &'ctx Context,
    scope: HashMap<String, PointerValue<'ctx>>,
}

impl<'ctx> Compiler<'ctx> {
    pub fn new(context: &'ctx Context) -> Self {
        Self {
            context,
            scope: HashMap::new(),
        }
    }

    pub fn create_module(&self, name: &str) -> Module<'ctx> {
        self.context.create_module(name)
    }

    pub fn create_builder(&self) -> Builder<'ctx> {
        self.context.create_builder()
    }

    fn declare_printf(&mut self, module: &Module<'ctx>) {
        let ty = self.context.void_type().fn_type(
            &[self.context.ptr_type(AddressSpace::default()).into()],
            true,
        );
        module.add_function("printf", ty, None);
    }

    fn compile_expr<'src>(
        &mut self,
        expr: &Expr<'src>,
        builder: &Builder<'ctx>,
    ) -> Result<'src, BasicValueEnum<'ctx>> {
        match expr {
            Expr::Int(value) => Ok(self.context.i64_type().const_int(*value, false).into()),
            Expr::Var(name) => {
                let Some(ptr) = self.scope.get(name.inner) else {
                    return Err(vec![Error::custom(name.span, "undefined variable")]);
                };
                let value = builder
                    .build_load(self.context.i64_type(), *ptr, name.inner)
                    .unwrap();
                Ok(value)
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
            Stmt::Var(name) => {
                let None = self.scope.get(name.inner) else {
                    return Err(vec![Error::custom(name.span, "variable redefinition")]);
                };
                let ptr = builder
                    .build_alloca(self.context.i64_type(), name.inner)
                    .unwrap();
                self.scope.insert(name.inner.to_string(), ptr);
            }
            Stmt::Print(expr) => {
                let printf = module.get_function("printf").unwrap();
                let value = self.compile_expr(expr, builder)?;
                let fmt = builder.build_global_string_ptr("%d\n", "fmt").unwrap();
                builder
                    .build_call(
                        printf,
                        &[fmt.as_pointer_value().into(), value.into()],
                        "call",
                    )
                    .unwrap();
            }
            Stmt::Assign(name, expr) => match self.scope.get(name.inner).cloned() {
                Some(ptr) => {
                    let value = self.compile_expr(expr, builder)?;
                    builder.build_store(ptr, value).unwrap();
                }
                None => return Err(vec![Error::custom(name.span, "undefined variable")]),
            },
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
        unsafe { Ok(execution_engine.get_function("main").unwrap()) }
    }
}
