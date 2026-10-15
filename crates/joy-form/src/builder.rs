// Copyright (c) 2026 Trent Polack. All Rights Reserved.
// Licensed under the MIT License.

//! Immediate-mode geometry builder: vertices, faces, boxes, spheres, instances, and material binding.
//! Ported from `src/form/geometry.ts`.

use crate::{
    GeometryData,
    materials::read_materials,
    resources::{read_mesh, transform_mesh},
    value::{DataRef, SharedBudget, SharedObject, Value, integer_range},
};
use serde_json::{Value as Json, json};
use std::{collections::HashMap, rc::Rc};

const MAX_VERTICES: usize = 150_000;
const MAX_TRIANGLES: usize = 180_000;

/// Owned indexed Y-up geometry. Attributes are copied at each vertex emission.
pub struct Builder {
    pub data: GeometryData,
    pub color: Vec<f64>,
    pub normal: Vec<f64>,
    pub uv: Vec<f64>,
    pub user_data: Option<DataRef>,
    pub material_index: u32,
    pub budget: SharedBudget,
    // Retaining the Rc prevents a dropped material's address being reused as a key.
    material_indices: HashMap<usize, (SharedObject, u32)>,
}

impl Builder {
    pub fn new(materials: Option<&Json>, budget: SharedBudget) -> Result<Self, String> {
        Ok(Self {
            data: GeometryData { materials: read_materials(materials)?, ..GeometryData::default() },
            color: vec![0.929, 0.424, 0.224, 1.0],
            normal: vec![0.0, 0.0, 0.0],
            uv: vec![0.0, 0.0],
            user_data: None,
            material_index: 0,
            budget,
            material_indices: HashMap::new(),
        })
    }

    pub fn vertex(&mut self, x: f64, y: f64, z: f64) -> Result<u32, String> {
        if self.data.positions.len() >= MAX_VERTICES * 3 {
            return Err("Vertex limit exceeded (150,000).".into());
        }
        if [x, y, z].iter().any(|value| !value.is_finite() || value.abs() > 1e6) {
            return Err("Coordinates must be finite and within ±1,000,000.".into());
        }
        let index = (self.data.positions.len() / 3) as u32;
        if self.user_data.as_ref().is_some_and(|value| !value.is_null()) && self.data.user_data.is_none() {
            self.data.user_data = Some(vec![Json::Null; index as usize]);
        }
        if let Some(user_data) = &mut self.data.user_data {
            user_data.push(crate::user_data::copy(self.user_data.as_deref().unwrap_or(&Json::Null), &self.budget)?);
        }
        self.data.positions.extend([x, y, z]);
        self.data.normals.extend_from_slice(&self.normal);
        self.data.uvs.extend_from_slice(&self.uv);
        self.data.colors.extend_from_slice(&self.color[..3]);
        self.data.alphas.push(self.color[3]);
        Ok(index)
    }

    pub fn face(&mut self, a: u32, b: u32, c: u32) -> Result<(), String> {
        if self.data.triangles.len() >= MAX_TRIANGLES * 3 {
            return Err("Triangle limit exceeded (180,000).".into());
        }
        self.data.triangles.extend([a, b, c]);
        self.data.triangle_materials.push(self.material_index);
        Ok(())
    }

    pub fn box_geometry(&mut self, values: &[f64]) -> Result<(), String> {
        crate::value::arity("box", values.len(), 6)?;
        let [x, y, z, sx, sy, sz] = <[f64; 6]>::try_from(values).unwrap();
        if sx <= 0.0 || sy <= 0.0 || sz <= 0.0 {
            return Err("Box sizes must be positive.".into());
        }
        let first = (self.data.positions.len() / 3) as u32;
        for [dx, dy, dz] in [
            [-1.0, -1.0, -1.0],
            [1.0, -1.0, -1.0],
            [1.0, 1.0, -1.0],
            [-1.0, 1.0, -1.0],
            [-1.0, -1.0, 1.0],
            [1.0, -1.0, 1.0],
            [1.0, 1.0, 1.0],
            [-1.0, 1.0, 1.0],
        ] {
            self.vertex(x + dx * sx / 2.0, y + dy * sy / 2.0, z + dz * sz / 2.0)?;
        }
        for [a, b, c] in [
            [0, 2, 1],
            [0, 3, 2],
            [4, 5, 6],
            [4, 6, 7],
            [0, 1, 5],
            [0, 5, 4],
            [3, 7, 6],
            [3, 6, 2],
            [0, 4, 7],
            [0, 7, 3],
            [1, 2, 6],
            [1, 6, 5],
        ] {
            self.face(first + a, first + b, first + c)?;
        }
        Ok(())
    }

    /// Radial normals override active normals temporarily; the final UV remains active.
    pub fn sphere(&mut self, values: &[f64]) -> Result<(), String> {
        if values.len() != 4 && values.len() != 5 {
            return Err("sphere expects center XYZ, radius, and optional segments.".into());
        }
        let [x, y, z, radius] = <[f64; 4]>::try_from(&values[..4]).unwrap();
        let segments = integer_range(values.get(4).copied().unwrap_or(20.0), 3.0, 96.0)?;
        if radius <= 0.0 {
            return Err("Sphere radius must be positive.".into());
        }
        let first = (self.data.positions.len() / 3) as u32;
        let rows = segments.div_ceil(2);
        let saved_normal = self.normal.clone();
        let result: Result<(), String> = (|| {
            for row in 0..=rows {
                let latitude = std::f64::consts::PI * row as f64 / rows as f64;
                for column in 0..=segments {
                    let angle = std::f64::consts::PI * 2.0 * column as f64 / segments as f64;
                    self.normal = vec![latitude.sin() * angle.cos(), latitude.cos(), latitude.sin() * angle.sin()];
                    self.uv = vec![column as f64 / segments as f64, row as f64 / rows as f64];
                    self.vertex(x + radius * self.normal[0], y + radius * self.normal[1], z + radius * self.normal[2])?;
                }
            }
            Ok(())
        })();
        self.normal = saved_normal;
        result?;
        for row in 0..rows {
            for column in 0..segments {
                let a = first + (row * (segments + 1) + column) as u32;
                let b = a + segments as u32 + 1;
                if row > 0 {
                    self.face(a, a + 1, b)?;
                }
                if row < rows - 1 {
                    self.face(a + 1, b + 1, b)?;
                }
            }
        }
        Ok(())
    }

    pub fn bind_material(&mut self, value: &Value) -> Result<u32, String> {
        let object = value.shared("material")?;
        let key = Rc::as_ptr(&object) as usize;
        if let Some((_, index)) = self.material_indices.get(&key) {
            return Ok(*index);
        }
        let mut suffix = self.data.materials.len();
        let name = loop {
            let candidate = format!("ScriptMaterial_{suffix}");
            if self.data.materials.iter().all(|material| material["name"] != candidate) {
                break candidate;
            }
            suffix += 1;
        };
        let mut material = object.borrow()["material"].clone();
        let record = material.as_object_mut().ok_or("Expected a material value.")?;
        record.insert("name".into(), json!(name));
        let mut materials = self.data.materials.clone();
        materials.push(material);
        let next = read_materials(Some(&json!(materials)))?;
        let index = (next.len() - 1) as u32;
        self.data.materials = next;
        self.material_indices.insert(key, (object, index));
        Ok(index)
    }

    /// Bake an instance while retaining resource ownership and active drawing state.
    pub fn instance(&mut self, values: &[Value]) -> Result<(), String> {
        if ![1, 2, 4, 5].contains(&values.len()) {
            return Err("instance expects mesh, optional position, rotation + scale, and material.".into());
        }
        let source = read_mesh(&values[0].shared("mesh")?.borrow())?;
        let vertex_components = source["positions"].as_array().unwrap().len();
        let index_count = source["indices"].as_array().unwrap().len();
        if self.data.positions.len() + vertex_components > MAX_VERTICES * 3
            || self.data.triangles.len() + index_count > MAX_TRIANGLES * 3
        {
            return Err("Instance output exceeds the geometry limit.".into());
        }
        let mesh = if values.len() == 1 {
            source
        } else {
            let rotation = if values.len() >= 4 { values[2].vector()? } else { vec![0.0; 3] };
            let scale = if values.len() >= 4 { values[3].vector()? } else { vec![1.0; 3] };
            transform_mesh(&source, &values[1].vector()?, &rotation, &scale)?
        };
        let saved_normal = self.normal.clone();
        let saved_uv = self.uv.clone();
        let saved_material = self.material_index;
        let first = (self.data.positions.len() / 3) as u32;
        if values.len() == 5 {
            self.material_index = self.bind_material(&values[4])?;
        }
        let result = (|| {
            let positions = mesh["positions"].as_array().unwrap();
            let normals = mesh["normals"].as_array().unwrap();
            let uvs = mesh["uvs"].as_array().unwrap();
            for (index, position) in positions.as_chunks::<3>().0.iter().enumerate() {
                self.normal = normals[index * 3..index * 3 + 3].iter().map(|value| value.as_f64().unwrap()).collect();
                self.uv = uvs[index * 2..index * 2 + 2].iter().map(|value| value.as_f64().unwrap()).collect();
                self.vertex(
                    position[0].as_f64().unwrap(),
                    position[1].as_f64().unwrap(),
                    position[2].as_f64().unwrap(),
                )?;
            }
            for triangle in mesh["indices"].as_array().unwrap().as_chunks::<3>().0 {
                self.face(
                    first + triangle[0].as_f64().unwrap() as u32,
                    first + triangle[1].as_f64().unwrap() as u32,
                    first + triangle[2].as_f64().unwrap() as u32,
                )?;
            }
            Ok(())
        })();
        self.normal = saved_normal;
        self.uv = saved_uv;
        self.material_index = saved_material;
        result
    }
}
