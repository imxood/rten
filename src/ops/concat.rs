use crate::ops::{Input, InputList, IntoOpResult, OpError, Operator, Output};
use crate::tensor::{Elements, Tensor};

pub fn concat<T: Copy>(inputs: &[&Tensor<T>], dim: usize) -> Result<Tensor<T>, OpError> {
    let first_shape = inputs[0].shape();
    if dim >= first_shape.len() {
        return Err(OpError::InvalidValue("dim is larger than input rank"));
    }

    for other in &inputs[1..] {
        let other_shape = other.shape();
        if other_shape.len() != first_shape.len() {
            return Err(OpError::IncompatibleInputShapes(
                "Tensors must have the same number of dimensions",
            ));
        }
        for d in 0..first_shape.len() {
            if d != dim && first_shape[d] != other_shape[d] {
                return Err(OpError::IncompatibleInputShapes(
                    "Dimensions must be the same except for concat dim",
                ));
            }
        }
    }

    let mut out_shape: Vec<_> = first_shape.into();
    for other in &inputs[1..] {
        out_shape[dim] += other.shape()[dim];
    }
    let mut out_data = Vec::with_capacity(out_shape.iter().product());

    struct ConcatIter<'a, T: Copy> {
        elements: Elements<'a, T>,
        chunk_size: usize,
    }

    let mut input_iters: Vec<ConcatIter<'_, T>> = inputs
        .iter()
        .map(|tensor| ConcatIter {
            elements: tensor.elements(),
            chunk_size: tensor.shape()[dim..].iter().product(),
        })
        .collect();

    while input_iters.iter().any(|it| it.elements.len() > 0) {
        for iter in input_iters.iter_mut() {
            out_data.extend(iter.elements.by_ref().take(iter.chunk_size));
        }
    }

    Ok(Tensor::from_data(out_shape, out_data))
}

#[derive(Debug)]
pub struct Concat {
    pub dim: usize,
}

impl Operator for Concat {
    fn name(&self) -> &str {
        "Concat"
    }

    fn run(&self, inputs: InputList) -> Result<Vec<Output>, OpError> {
        let first = inputs.require(0)?;
        match first {
            Input::FloatTensor(_) => {
                let mut typed_inputs: Vec<_> = Vec::new();
                for input in inputs.iter() {
                    let tensor: &Tensor<f32> = input.try_into()?;
                    typed_inputs.push(tensor);
                }
                concat(&typed_inputs, self.dim).into_op_result()
            }
            Input::IntTensor(_) => {
                let mut typed_inputs: Vec<_> = Vec::new();
                for input in inputs.iter() {
                    let tensor: &Tensor<i32> = input.try_into()?;
                    typed_inputs.push(tensor);
                }
                concat(&typed_inputs, self.dim).into_op_result()
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use crate::ops::{concat, OpError};
    use crate::tensor::{from_data, zeros, Tensor};
    use crate::test_util::expect_equal;

    fn from_slice<T: Copy>(data: &[T]) -> Tensor<T> {
        from_data(vec![data.len()], data.into())
    }

    #[test]
    fn test_concat() -> Result<(), String> {
        let a = from_data(vec![2, 2, 1], vec![0.1, 0.2, 0.3, 0.4]);
        let b = from_data(vec![2, 2, 1], vec![1.0, 2.0, 3.0, 4.0]);

        // Concatenation along the first dimension
        let expected = from_data(vec![4, 2, 1], vec![0.1, 0.2, 0.3, 0.4, 1.0, 2.0, 3.0, 4.0]);
        let result = concat(&[&a, &b], 0).unwrap();
        expect_equal(&result, &expected)?;

        // Concatenation along a non-first dimension
        let expected = from_data(vec![2, 2, 2], vec![0.1, 1.0, 0.2, 2.0, 0.3, 3.0, 0.4, 4.0]);
        let result = concat(&[&a, &b], 2).unwrap();
        expect_equal(&result, &expected)?;

        // Concatenation with one input
        let result = concat(&[&a], 0).unwrap();
        expect_equal(&result, &a)?;

        // Concatenation with more than two inputs
        let result = concat(&[&a, &b, &a], 0).unwrap();
        assert_eq!(result.shape(), &[6, 2, 1]);

        // Concatentation with some empty inputs
        let a = from_slice(&[1, 2, 3]);
        let b = from_slice(&[]);
        let c = from_slice(&[4, 5, 6]);
        let result = concat(&[&a, &b, &c], 0).unwrap();
        assert_eq!(result.shape(), &[6]);
        assert_eq!(result.data(), &[1, 2, 3, 4, 5, 6]);

        Ok(())
    }

    #[test]
    fn test_concat_invalid_inputs() {
        // Invalid `dim` attribute
        let input = from_slice(&[1, 2, 3]);
        let result = concat(&[&input, &input], 1);
        assert_eq!(
            result.err(),
            Some(OpError::InvalidValue("dim is larger than input rank"))
        );

        // Shape mismatch
        let a = zeros::<f32>(&[1]);
        let b = zeros::<f32>(&[1, 2]);
        let result = concat(&[&a, &b], 0);
        assert_eq!(
            result.err(),
            Some(OpError::IncompatibleInputShapes(
                "Tensors must have the same number of dimensions"
            ))
        );

        // Shape mismatch in non-`dim` dimension
        let a = zeros::<f32>(&[5, 10]);
        let b = zeros::<f32>(&[5, 11]);
        let result = concat(&[&a, &b], 0);
        assert_eq!(
            result.err(),
            Some(OpError::IncompatibleInputShapes(
                "Dimensions must be the same except for concat dim"
            ))
        );
    }
}