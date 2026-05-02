use inkwell::{
    OptimizationLevel,
    builder::Builder,
    context::Context,
    execution_engine::{ExecutionEngine, JitFunction},
    module::Module,
    values::BasicValueEnum,
};

use crate::parse::Expr;

pub struct Compiler<'ctx> {
    context: &'ctx Context,
    module: Module<'ctx>,
    builder: Builder<'ctx>,
    execution_engine: ExecutionEngine<'ctx>,
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
        }
    }

    pub fn compile_expr(
        &mut self,
        expr: &Expr,
    ) -> JitFunction<'ctx, unsafe extern "C" fn() -> u64> {
        let fun_type = self.context.i64_type().fn_type(&[], false);
        let fun = self.module.add_function("main", fun_type, None);
        let block = self.context.append_basic_block(fun, "entry");
        self.builder.position_at_end(block);

        let value = match expr {
            Expr::Int(value) => self.context.i64_type().const_int(*value, false),
        };

        self.builder
            .build_return(Some(&Into::<BasicValueEnum<'ctx>>::into(value)))
            .unwrap();

        unsafe { self.execution_engine.get_function("main").unwrap() }
    }
}
