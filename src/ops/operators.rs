use wasnn_tensor::prelude::*;
use wasnn_tensor::{NdTensorBase, NdTensorView, Tensor, TensorBase};

use crate::ops::OpError;
use crate::ops::{arg_max, pad, resize_image, softmax, topk};

/// Trait which exposes ONNX operators as methods of tensors.
///
/// This trait provides methods which are available on all tensor types. See
/// [FloatOperators] for additional operators which are only available on float
/// tensors.
pub trait Operators {
    type Elem;

    fn arg_max(&self, axis: isize, keep_dims: bool) -> Result<Tensor<i32>, OpError>
    where
        Self::Elem: Copy + PartialOrd;

    fn pad(
        &self,
        padding: NdTensorView<i32, 1>,
        val: Self::Elem,
    ) -> Result<Tensor<Self::Elem>, OpError>
    where
        Self::Elem: Copy;

    fn topk(
        &self,
        k: usize,
        axis: Option<isize>,
        largest: bool,
        sorted: bool,
    ) -> Result<(Tensor<Self::Elem>, Tensor<i32>), OpError>
    where
        Self::Elem: Copy + Default + PartialOrd;
}

/// Trait which exposes ONNX operators as methods of tensors.
///
/// This trait provides methods which are only available on float tensors.
pub trait FloatOperators {
    /// Resize an NCHW image tensor to a given `[height, width]` using bilinear
    /// interpolation.
    fn resize_image(&self, size: [usize; 2]) -> Result<Tensor, OpError>;
    fn softmax(&self, axis: isize) -> Result<Tensor, OpError>;
}

impl<T, S: AsRef<[T]>> Operators for TensorBase<T, S> {
    type Elem = T;

    fn arg_max(&self, axis: isize, keep_dims: bool) -> Result<Tensor<i32>, OpError>
    where
        T: Copy + PartialOrd,
    {
        arg_max(self.view(), axis, keep_dims)
    }

    fn pad(&self, padding: NdTensorView<i32, 1>, val: T) -> Result<Tensor<Self::Elem>, OpError>
    where
        Self::Elem: Copy,
    {
        pad(self.view(), &padding, val)
    }

    fn topk(
        &self,
        k: usize,
        axis: Option<isize>,
        largest: bool,
        sorted: bool,
    ) -> Result<(Tensor<Self::Elem>, Tensor<i32>), OpError>
    where
        T: Copy + Default + PartialOrd,
    {
        topk(self.view(), k, axis, largest, sorted)
    }
}

impl<T, S: AsRef<[T]>, const N: usize> Operators for NdTensorBase<T, S, N> {
    type Elem = T;

    fn arg_max(&self, axis: isize, keep_dims: bool) -> Result<Tensor<i32>, OpError>
    where
        T: Copy + PartialOrd,
    {
        arg_max(self.as_dyn(), axis, keep_dims)
    }

    fn pad(&self, padding: NdTensorView<i32, 1>, val: T) -> Result<Tensor<Self::Elem>, OpError>
    where
        Self::Elem: Copy,
    {
        pad(self.as_dyn(), &padding, val)
    }

    fn topk(
        &self,
        k: usize,
        axis: Option<isize>,
        largest: bool,
        sorted: bool,
    ) -> Result<(Tensor<Self::Elem>, Tensor<i32>), OpError>
    where
        T: Copy + Default + PartialOrd,
    {
        topk(self.as_dyn(), k, axis, largest, sorted)
    }
}

impl<S: AsRef<[f32]>> FloatOperators for TensorBase<f32, S> {
    fn resize_image(&self, size: [usize; 2]) -> Result<Tensor, OpError> {
        resize_image(self.view(), size)
    }

    fn softmax(&self, axis: isize) -> Result<Tensor, OpError> {
        softmax(self.view(), axis)
    }
}

impl<S: AsRef<[f32]>, const N: usize> FloatOperators for NdTensorBase<f32, S, N> {
    fn resize_image(&self, size: [usize; 2]) -> Result<Tensor, OpError> {
        resize_image(self.as_dyn(), size)
    }

    fn softmax(&self, axis: isize) -> Result<Tensor, OpError> {
        softmax(self.as_dyn(), axis)
    }
}