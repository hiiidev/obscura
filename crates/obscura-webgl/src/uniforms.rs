//! Uniform upload lengths and program/link ownership are checked in browser
//! state before any native call can read a page-provided array.
use crate::{
    api::CanvasContext,
    objects::{Kind, Object},
    queries::Value,
};
use glow::HasContext;
use serde::Deserialize;

#[derive(Debug, Deserialize)]
#[serde(tag = "type", content = "values", rename_all = "camelCase")]
pub enum UniformValues {
    Float(Vec<f32>),
    Int(Vec<i32>),
    Uint(Vec<u32>),
}
#[derive(Debug, Deserialize)]
pub struct Uniform {
    pub location: u32,
    pub columns: u8,
    pub rows: u8,
    #[serde(default)]
    pub matrix: bool,
    #[serde(default)]
    pub transpose: bool,
    pub values: UniformValues,
}
fn shape(columns: u8, rows: u8, matrix: bool, transpose: bool, version: u8) -> Result<usize, u32> {
    if transpose {
        return Err(glow::INVALID_VALUE);
    }
    if matrix {
        if !(2..=4).contains(&columns)
            || !(2..=4).contains(&rows)
            || version == 1 && columns != rows
        {
            return Err(glow::INVALID_VALUE);
        }
    } else if !(1..=4).contains(&columns) || rows != 1 {
        return Err(glow::INVALID_VALUE);
    }
    Ok(columns as usize * rows as usize)
}
impl CanvasContext {
    pub fn uniform(&mut self, request: Uniform) {
        if !self.activate() {
            return;
        }
        if let Err(error) = self.uniform_current(request) {
            self.error(error);
        }
    }
    fn uniform_current(&mut self, request: Uniform) -> Result<(), u32> {
        let Uniform {
            location,
            columns,
            rows,
            matrix,
            transpose,
            values,
        } = request;
        // null is an explicit no-op, including after a program switch.
        if location == 0 {
            return Ok(());
        }
        let count = shape(columns, rows, matrix, transpose, self.version)?;
        let location = self.objects.location(location, self.current_program)?;
        let gl = &self.driver.as_ref().unwrap().gl;
        unsafe {
            match values {
                UniformValues::Float(values) => {
                    if values.len() % count != 0 {
                        return Err(glow::INVALID_VALUE);
                    }
                    if values.is_empty() {
                        return Ok(());
                    }
                    if matrix {
                        match (columns, rows) {
                            (2, 2) => {
                                gl.uniform_matrix_2_f32_slice(Some(&location), false, &values)
                            }
                            (2, 3) => {
                                gl.uniform_matrix_2x3_f32_slice(Some(&location), false, &values)
                            }
                            (2, 4) => {
                                gl.uniform_matrix_2x4_f32_slice(Some(&location), false, &values)
                            }
                            (3, 2) => {
                                gl.uniform_matrix_3x2_f32_slice(Some(&location), false, &values)
                            }
                            (3, 3) => {
                                gl.uniform_matrix_3_f32_slice(Some(&location), false, &values)
                            }
                            (3, 4) => {
                                gl.uniform_matrix_3x4_f32_slice(Some(&location), false, &values)
                            }
                            (4, 2) => {
                                gl.uniform_matrix_4x2_f32_slice(Some(&location), false, &values)
                            }
                            (4, 3) => {
                                gl.uniform_matrix_4x3_f32_slice(Some(&location), false, &values)
                            }
                            (4, 4) => {
                                gl.uniform_matrix_4_f32_slice(Some(&location), false, &values)
                            }
                            _ => unreachable!(),
                        }
                    } else {
                        match columns {
                            1 => gl.uniform_1_f32_slice(Some(&location), &values),
                            2 => gl.uniform_2_f32_slice(Some(&location), &values),
                            3 => gl.uniform_3_f32_slice(Some(&location), &values),
                            4 => gl.uniform_4_f32_slice(Some(&location), &values),
                            _ => unreachable!(),
                        }
                    }
                }
                UniformValues::Int(values) => {
                    if matrix {
                        return Err(glow::INVALID_OPERATION);
                    }
                    if values.len() % count != 0 {
                        return Err(glow::INVALID_VALUE);
                    }
                    if values.is_empty() {
                        return Ok(());
                    }
                    match columns {
                        1 => gl.uniform_1_i32_slice(Some(&location), &values),
                        2 => gl.uniform_2_i32_slice(Some(&location), &values),
                        3 => gl.uniform_3_i32_slice(Some(&location), &values),
                        4 => gl.uniform_4_i32_slice(Some(&location), &values),
                        _ => unreachable!(),
                    }
                }
                UniformValues::Uint(values) => {
                    if self.version != 2 {
                        return Err(glow::INVALID_OPERATION);
                    }
                    if matrix {
                        return Err(glow::INVALID_OPERATION);
                    }
                    if values.len() % count != 0 {
                        return Err(glow::INVALID_VALUE);
                    }
                    if values.is_empty() {
                        return Ok(());
                    }
                    match columns {
                        1 => gl.uniform_1_u32_slice(Some(&location), &values),
                        2 => gl.uniform_2_u32_slice(Some(&location), &values),
                        3 => gl.uniform_3_u32_slice(Some(&location), &values),
                        4 => gl.uniform_4_u32_slice(Some(&location), &values),
                        _ => unreachable!(),
                    }
                }
            }
        }
        Ok(())
    }
    pub fn get_uniform(&mut self, program: u32, location: u32) -> Value {
        if !self.activate() {
            return Value::Null;
        }
        match self.get_uniform_current(program, location) {
            Ok(value) => value,
            Err(error) => {
                self.error(error);
                Value::Null
            }
        }
    }
    fn get_uniform_current(&self, program: u32, location: u32) -> Result<Value, u32> {
        let Object::Program(native) = self.objects.get_for_query(program, Kind::Program)? else {
            unreachable!()
        };
        let location = self.objects.location(location, program)?;
        let gl = &self.driver.as_ref().unwrap().gl;
        unsafe {
            if !gl.is_program(native) || !gl.get_program_link_status(native) {
                return Err(glow::INVALID_OPERATION);
            }
            for index in 0..gl.get_active_uniforms(native) {
                let Some(info) = gl.get_active_uniform(native, index) else {
                    continue;
                };
                for element in 0..info.size {
                    let name = if info.size > 1 {
                        if info.name.contains("[0]") {
                            info.name.replacen("[0]", &format!("[{element}]"), 1)
                        } else {
                            format!("{}[{element}]", info.name)
                        }
                    } else {
                        info.name.clone()
                    };
                    if gl.get_uniform_location(native, &name) != Some(location) {
                        continue;
                    }
                    let (kind, count) = uniform_type(info.utype)?;
                    return Ok(match kind {
                        'f' => {
                            let mut values = vec![0.0; count];
                            gl.get_uniform_f32(native, &location, &mut values);
                            if count == 1 {
                                Value::Float(values[0])
                            } else {
                                Value::Float32(values)
                            }
                        }
                        'u' => {
                            let mut values = vec![0; count];
                            gl.get_uniform_u32(native, &location, &mut values);
                            if count == 1 {
                                Value::UInt(values[0])
                            } else {
                                Value::Uint32(values)
                            }
                        }
                        'b' => {
                            let mut values = vec![0; count];
                            gl.get_uniform_i32(native, &location, &mut values);
                            if count == 1 {
                                Value::Bool(values[0] != 0)
                            } else {
                                Value::Bools(values.into_iter().map(|v| v != 0).collect())
                            }
                        }
                        _ => {
                            let mut values = vec![0; count];
                            gl.get_uniform_i32(native, &location, &mut values);
                            if count == 1 {
                                Value::Int(values[0])
                            } else {
                                Value::Int32(values)
                            }
                        }
                    });
                }
            }
        }
        Err(glow::INVALID_OPERATION)
    }
}
fn uniform_type(data_type: u32) -> Result<(char, usize), u32> {
    Ok(match data_type {
        glow::FLOAT => ('f', 1),
        glow::FLOAT_VEC2 => ('f', 2),
        glow::FLOAT_VEC3 => ('f', 3),
        glow::FLOAT_VEC4 => ('f', 4),
        glow::FLOAT_MAT2 => ('f', 4),
        glow::FLOAT_MAT3 => ('f', 9),
        glow::FLOAT_MAT4 => ('f', 16),
        glow::FLOAT_MAT2x3 | glow::FLOAT_MAT3x2 => ('f', 6),
        glow::FLOAT_MAT2x4 | glow::FLOAT_MAT4x2 => ('f', 8),
        glow::FLOAT_MAT3x4 | glow::FLOAT_MAT4x3 => ('f', 12),
        glow::BOOL => ('b', 1),
        glow::BOOL_VEC2 => ('b', 2),
        glow::BOOL_VEC3 => ('b', 3),
        glow::BOOL_VEC4 => ('b', 4),
        glow::INT => ('i', 1),
        glow::INT_VEC2 => ('i', 2),
        glow::INT_VEC3 => ('i', 3),
        glow::INT_VEC4 => ('i', 4),
        glow::UNSIGNED_INT => ('u', 1),
        glow::UNSIGNED_INT_VEC2 => ('u', 2),
        glow::UNSIGNED_INT_VEC3 => ('u', 3),
        glow::UNSIGNED_INT_VEC4 => ('u', 4),
        glow::SAMPLER_2D
        | glow::SAMPLER_CUBE
        | glow::SAMPLER_3D
        | glow::SAMPLER_2D_SHADOW
        | glow::SAMPLER_CUBE_SHADOW
        | glow::SAMPLER_2D_ARRAY
        | glow::SAMPLER_2D_ARRAY_SHADOW
        | glow::INT_SAMPLER_2D
        | glow::INT_SAMPLER_CUBE
        | glow::INT_SAMPLER_3D
        | glow::INT_SAMPLER_2D_ARRAY
        | glow::UNSIGNED_INT_SAMPLER_2D
        | glow::UNSIGNED_INT_SAMPLER_CUBE
        | glow::UNSIGNED_INT_SAMPLER_3D
        | glow::UNSIGNED_INT_SAMPLER_2D_ARRAY => ('i', 1),
        _ => return Err(glow::INVALID_OPERATION),
    })
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn uniform_shapes_validate_matrix_and_version_before_native_reads() {
        assert_eq!(shape(4, 4, true, false, 1), Ok(16));
        assert!(shape(4, 3, true, false, 1).is_err());
        assert_eq!(shape(4, 3, true, false, 2), Ok(12));
        assert!(shape(4, 4, true, true, 2).is_err());
        assert!(shape(1, 4, false, false, 2).is_err());
        assert!(shape(0, 1, false, false, 1).is_err());
    }
    #[test]
    fn uniform_query_sizes_cover_vectors_matrices_samplers_and_bools() {
        assert_eq!(uniform_type(glow::FLOAT_MAT4), Ok(('f', 16)));
        assert_eq!(uniform_type(glow::FLOAT_MAT3x2), Ok(('f', 6)));
        assert_eq!(uniform_type(glow::BOOL_VEC3), Ok(('b', 3)));
        assert_eq!(uniform_type(glow::UNSIGNED_INT_SAMPLER_2D), Ok(('i', 1)));
        assert_eq!(uniform_type(glow::UNSIGNED_INT_VEC4), Ok(('u', 4)));
        assert!(uniform_type(glow::DOUBLE).is_err());
    }
}
