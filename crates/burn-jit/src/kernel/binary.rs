use crate::{element::JitElement, tensor::JitTensor, JitRuntime};
use burn_tensor::Shape;
use cubecl::{frontend::TensorHandleRef, CubeCountSettings, Execution};

/// Creates a binary kernel.
#[macro_export]
macro_rules! binary {
    (
        operation: $ops:expr,
        runtime: $runtime:ty,
        input: $lhs:expr; $rhs:expr,
        elem: $elem:ty
    ) => {{
        binary!(operation: $ops, compiler: <$runtime as JitRuntime>::Compiler, elem_in: $elem, elem_out: $elem);

        $crate::kernel::binary::<
            Ops<<$runtime as Runtime>::Compiler, $elem, $elem>,
            OpsInplaceLhs<<$runtime as Runtime>::Compiler, $elem, $elem>,
            OpsInplaceRhs<<$runtime as Runtime>::Compiler, $elem, $elem>,
            $runtime,
            $elem,
            D
        >($lhs, $rhs, true, Ops::new(), OpsInplaceLhs::new(), OpsInplaceRhs::new())
    }};

    (
        operation: $ops:expr,
        compiler: $compiler:ty,
        elem_in: $elem_in:ty,
        elem_out: $elem_out:ty
    ) => {
        #[derive(new)]
        pub struct Ops<C, I, O> {
            _c: core::marker::PhantomData<C>,
            _i: core::marker::PhantomData<I>,
            _o: core::marker::PhantomData<O>,
        }
        #[derive(new)]
        pub struct OpsInplaceLhs<C, I, O> {
            _c: core::marker::PhantomData<C>,
            _i: core::marker::PhantomData<I>,
            _o: core::marker::PhantomData<O>,
        }
        #[derive(new)]
        pub struct OpsInplaceRhs<C, I, O> {
            _c: core::marker::PhantomData<C>,
            _i: core::marker::PhantomData<I>,
            _o: core::marker::PhantomData<O>,
        }

        #[allow(clippy::redundant_closure_call)]
        fn compile<I, O>(
            settings: cubecl::KernelSettings,
        ) -> cubecl::ir::KernelDefinition
        where
            I: $crate::element::JitElement,
            O: $crate::element::JitElement
        {
            let mut scope = cubecl::ir::Scope::root();
            let position = cubecl::ir::Variable::AbsolutePos;

            let op = $ops(&mut scope, I::cube_elem(), position);
            scope.register(op);

            let local = scope.last_local_index().unwrap().index().unwrap();

            let lhs = cubecl::InputInfo::Array {
                item: cubecl::ir::Item::new(I::cube_elem()),
                visibility: cubecl::ir::Visibility::Read,
            };
            let rhs = cubecl::InputInfo::Array {
                item: cubecl::ir::Item::new(I::cube_elem()),
                visibility: cubecl::ir::Visibility::Read,
            };
            let out = cubecl::OutputInfo::ArrayWrite {
                item: cubecl::ir::Item::new(O::cube_elem()),
                local,
                position,
            };
            let info = cubecl::prelude::KernelExpansion {
                inputs: vec![lhs, rhs],
                outputs: vec![out],
                scope,
            };
            cubecl::prelude::KernelIntegrator::new(info).integrate(settings)
        }

        #[allow(clippy::redundant_closure_call)]
        impl<C, I, O> $crate::kernel::Kernel for Ops<C, I, O>
        where
            C: cubecl::Compiler,
            I: $crate::element::JitElement,
            O: $crate::element::JitElement
        {
            fn define(&self) -> cubecl::ir::KernelDefinition {
                let settings = cubecl::KernelSettings::default();
                compile::<I, O>(settings)
            }
        }

        #[allow(clippy::redundant_closure_call)]
        impl<C, I, O> $crate::kernel::Kernel
            for OpsInplaceLhs<C, I, O>
        where
            C: cubecl::Compiler,
            I: $crate::element::JitElement,
            O: $crate::element::JitElement
        {
            fn define(&self) -> cubecl::ir::KernelDefinition {
                let mapping = cubecl::InplaceMapping {
                    pos_input: 0,
                    pos_output: 0,
                };
                let settings = cubecl::KernelSettings::default()
                    .inplace(vec![mapping]);
                compile::<I, O>(settings)
            }
        }

        #[allow(clippy::redundant_closure_call)]
        impl<C, I, O> $crate::kernel::Kernel
            for OpsInplaceRhs<C, I, O>
        where
            C: cubecl::Compiler,
            I: $crate::element::JitElement,
            O: $crate::element::JitElement
        {
            fn define(&self) -> cubecl::ir::KernelDefinition {
                let mapping = cubecl::InplaceMapping {
                    pos_input: 1,
                    pos_output: 0,
                };
                let settings = cubecl::KernelSettings::default()
                    .inplace(vec![mapping]);
                compile::<I, O>(settings)
            }
        }
    };
}

/// Launch an binary operation.
pub fn binary<Kernel, KernelInplaceLhs, KernelInplaceRhs, R: JitRuntime, E, const D: usize>(
    lhs: JitTensor<R, E, D>,
    rhs: JitTensor<R, E, D>,
    inplace_enabled: bool,
    kernel: Kernel,
    kernel_inplace_lhs: KernelInplaceLhs,
    kernel_inplace_rhs: KernelInplaceRhs,
) -> JitTensor<R, E, D>
where
    Kernel: crate::kernel::Kernel,
    KernelInplaceLhs: crate::kernel::Kernel,
    KernelInplaceRhs: crate::kernel::Kernel,
    E: JitElement,
{
    if inplace_enabled && lhs.can_mut_broadcast(&rhs) {
        Execution::start(kernel_inplace_lhs, rhs.client)
            .inputs(&[
                TensorHandleRef::<R>::new(&lhs.handle, &lhs.strides, &lhs.shape.dims),
                TensorHandleRef::new(&rhs.handle, &rhs.strides, &rhs.shape.dims),
            ])
            .execute(CubeCountSettings::Input { pos: 0 });

        lhs
    } else if inplace_enabled && rhs.can_mut_broadcast(&lhs) {
        Execution::start(kernel_inplace_rhs, lhs.client)
            .inputs(&[
                TensorHandleRef::<R>::new(&lhs.handle, &lhs.strides, &lhs.shape.dims),
                TensorHandleRef::new(&rhs.handle, &rhs.strides, &rhs.shape.dims),
            ])
            .execute(CubeCountSettings::Input { pos: 1 });

        rhs
    } else {
        let mut shape_out = [0; D];
        lhs.shape
            .dims
            .iter()
            .zip(rhs.shape.dims.iter())
            .enumerate()
            .for_each(|(index, (dim_lhs, dim_rhs))| {
                shape_out[index] = usize::max(*dim_lhs, *dim_rhs);
            });

        let shape_out = Shape::new(shape_out);
        let num_elems = shape_out.num_elements();
        let buffer = lhs.client.empty(num_elems * core::mem::size_of::<E>());
        let out = JitTensor::new_contiguous(lhs.client.clone(), lhs.device, shape_out, buffer);

        Execution::start(kernel, lhs.client)
            .inputs(&[
                TensorHandleRef::<R>::new(&lhs.handle, &lhs.strides, &lhs.shape.dims),
                TensorHandleRef::new(&rhs.handle, &rhs.strides, &rhs.shape.dims),
            ])
            .outputs(&[TensorHandleRef::new(
                &out.handle,
                &out.strides,
                &out.shape.dims,
            )])
            .execute(CubeCountSettings::Output { pos: 0 });

        out
    }
}
