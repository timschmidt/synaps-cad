use std::fmt;

use csgrs::bmesh::BMesh;
use csgrs::csg::CSG;
use csgrs::mesh::Mesh as CsgMesh;
use csgrs::mesh::plane::Plane;
use csgrs::sketch::Sketch;
use nalgebra::Vector3;

use crate::compiler::geometry::conversions::{bmesh_to_csg_mesh, csg_mesh_to_bmesh};

#[derive(Clone, Copy, Debug)]
pub enum BoolOp {
    Union,
    Difference,
    Intersection,
}

#[derive(Clone, Copy)]
pub enum TransformKind {
    Translate,
    Rotate,
    Scale,
    Mirror,
}

#[derive(Clone)]
pub enum Shape {
    Mesh3D(Box<BMesh<()>>),
    Sketch2D(Sketch<()>),
    /// Render-only fallback: `CsgMesh` that failed manifold creation.
    /// Can be rendered directly but boolean ops will degrade to empty.
    FallbackMesh(CsgMesh<()>),
    /// A boolean operation panicked — propagate failure to avoid cascading panics.
    Failed(String),
}

impl fmt::Debug for Shape {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Mesh3D(_) => write!(f, "Shape::Mesh3D"),
            Self::Sketch2D(_) => write!(f, "Shape::Sketch2D"),
            Self::FallbackMesh(_) => write!(f, "Shape::FallbackMesh"),
            Self::Failed(e) => write!(f, "Shape::Failed({e})"),
        }
    }
}

impl Shape {
    /// Create a 3D shape from a `CsgMesh` primitive.
    /// Falls back to direct polygon rendering if manifold creation fails (still renders,
    /// but boolean ops on it may produce artifacts).
    pub fn from_csg_mesh(mesh: CsgMesh<()>) -> Self {
        match csg_mesh_to_bmesh(&mesh) {
            Ok(bmesh) => Self::Mesh3D(Box::new(bmesh)),
            Err(e) => {
                if cfg!(debug_assertions) {
                    eprintln!("[DEBUG] Manifold failed ({e}), using polygon fallback");
                }
                Self::FallbackMesh(mesh)
            }
        }
    }

    /// Convert to `BMesh` for boolean operations.
    pub fn into_bmesh(self) -> BMesh<()> {
        match self {
            Self::Mesh3D(b) => *b,
            Self::Sketch2D(s) => BMesh::from(s.extrude(0.01)),
            Self::FallbackMesh(_) | Self::Failed(_) => BMesh::new(),
        }
    }

    /// Extract polygon data for hull computation (converts back to `CsgMesh`).
    pub fn into_csg_mesh(self) -> CsgMesh<()> {
        match self {
            Self::Mesh3D(b) => bmesh_to_csg_mesh(&b),
            Self::Sketch2D(s) => s.extrude(0.01),
            Self::FallbackMesh(m) => m,
            Self::Failed(_) => CsgMesh::new(),
        }
    }

    #[must_use]
    pub fn union(self, other: Self) -> Self {
        if let Self::Failed(e) = &self {
            return Self::Failed(e.clone());
        }
        if let Self::Failed(e) = &other {
            return Self::Failed(e.clone());
        }
        match (self, other) {
            (Self::Sketch2D(a), Self::Sketch2D(b)) => Self::Sketch2D(a.union(&b)),
            (a, b) => Self::bool_op_with_fallback(a, b, BoolOp::Union),
        }
    }

    #[must_use]
    pub fn difference(self, other: Self) -> Self {
        if let Self::Failed(e) = &self {
            return Self::Failed(e.clone());
        }
        if let Self::Failed(e) = &other {
            return Self::Failed(e.clone());
        }
        match (self, other) {
            (Self::Sketch2D(a), Self::Sketch2D(b)) => Self::Sketch2D(a.difference(&b)),
            (a, b) => Self::bool_op_with_fallback(a, b, BoolOp::Difference),
        }
    }

    #[must_use]
    pub fn intersection(self, other: Self) -> Self {
        if let Self::Failed(e) = &self {
            return Self::Failed(e.clone());
        }
        if let Self::Failed(e) = &other {
            return Self::Failed(e.clone());
        }
        match (self, other) {
            (Self::Sketch2D(a), Self::Sketch2D(b)) => Self::Sketch2D(a.intersection(&b)),
            (a, b) => Self::bool_op_with_fallback(a, b, BoolOp::Intersection),
        }
    }

    /// Try boolmesh first; on panic, fall back to csgrs BSP-tree booleans.
    fn bool_op_with_fallback(a: Self, b: Self, op: BoolOp) -> Self {
        let a_csg = a.into_csg_mesh();
        let b_csg = b.into_csg_mesh();

        // Try boolmesh path: CsgMesh → BMesh → boolean → Shape
        let a_bmesh = csg_mesh_to_bmesh(&a_csg);
        let b_bmesh = csg_mesh_to_bmesh(&b_csg);

        if let (Ok(ab), Ok(bb)) = (a_bmesh, b_bmesh) {
            let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| match op {
                BoolOp::Union => ab.union(&bb),
                BoolOp::Difference => ab.difference(&bb),
                BoolOp::Intersection => ab.intersection(&bb),
            }));
            if let Ok(r) = result {
                return Self::Mesh3D(Box::new(r));
            }
            eprintln!("[SynapsCAD] boolmesh {op:?} panicked, falling back to BSP");
        } else if cfg!(debug_assertions) {
            eprintln!("[DEBUG] BMesh conversion failed, using BSP for {op:?}");
        }

        // Fallback: csgrs BSP-tree booleans
        let result_csg = match op {
            BoolOp::Union => a_csg.union(&b_csg),
            BoolOp::Difference => a_csg.difference(&b_csg),
            BoolOp::Intersection => a_csg.intersection(&b_csg),
        };
        Self::from_csg_mesh(result_csg)
    }

    #[must_use]
    pub fn translate(self, x: f64, y: f64, z: f64) -> Self {
        match self {
            Self::Mesh3D(m) => Self::bmesh_transform_with_fallback(*m, |m| m.translate(x, y, z)),
            Self::Sketch2D(s) => {
                if z.abs() < 1e-12 {
                    Self::Sketch2D(s.translate(x, y, 0.0))
                } else {
                    Self::from_csg_mesh(s.extrude(0.01).translate(x, y, z))
                }
            }
            Self::FallbackMesh(m) => Self::FallbackMesh(m.translate(x, y, z)),
            Self::Failed(e) => Self::Failed(e),
        }
    }

    #[must_use]
    pub fn rotate(self, x: f64, y: f64, z: f64) -> Self {
        match self {
            Self::Mesh3D(m) => Self::bmesh_transform_with_fallback(*m, |m| m.rotate(x, y, z)),
            Self::Sketch2D(s) => {
                if x.abs() < 1e-12 && y.abs() < 1e-12 {
                    Self::Sketch2D(s.rotate(0.0, 0.0, z))
                } else {
                    Self::from_csg_mesh(s.extrude(0.01).rotate(x, y, z))
                }
            }
            Self::FallbackMesh(m) => Self::FallbackMesh(m.rotate(x, y, z)),
            Self::Failed(e) => Self::Failed(e),
        }
    }

    #[must_use]
    pub fn scale(self, sx: f64, sy: f64, sz: f64) -> Self {
        match self {
            Self::Mesh3D(m) => Self::bmesh_transform_with_fallback(*m, |m| m.scale(sx, sy, sz)),
            Self::Sketch2D(s) => {
                if (sz - 1.0).abs() < 1e-12 {
                    Self::Sketch2D(s.scale(sx, sy, 1.0))
                } else {
                    Self::from_csg_mesh(s.extrude(0.01).scale(sx, sy, sz))
                }
            }
            Self::FallbackMesh(m) => Self::FallbackMesh(m.scale(sx, sy, sz)),
            Self::Failed(e) => Self::Failed(e),
        }
    }

    /// Try a `BMesh` transform; on panic fall back to `CsgMesh` transform.
    fn bmesh_transform_with_fallback(m: BMesh<()>, f: impl FnOnce(BMesh<()>) -> BMesh<()>) -> Self {
        let csg_backup = bmesh_to_csg_mesh(&m);
        std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| f(m))).map_or_else(
            |_| {
                eprintln!("[SynapsCAD] BMesh transform panicked, falling back to CsgMesh");
                Self::FallbackMesh(csg_backup)
            },
            |result| Self::Mesh3D(Box::new(result)),
        )
    }

    #[must_use]
    pub fn mirror(self, nx: f64, ny: f64, nz: f64) -> Self {
        let len = (nx.mul_add(nx, ny.mul_add(ny, nz * nz))).sqrt();
        if len < 1e-12 {
            return self;
        }
        let plane = Plane::from_normal(Vector3::new(nx, ny, nz), 0.0);
        match self {
            Self::Mesh3D(m) => Self::bmesh_transform_with_fallback(*m, |m| m.mirror(plane)),
            Self::Sketch2D(s) => Self::Sketch2D(s.mirror(plane)),
            Self::FallbackMesh(m) => Self::FallbackMesh(m.mirror(plane)),
            Self::Failed(e) => Self::Failed(e),
        }
    }

    #[must_use]
    #[allow(dead_code)]
    pub fn center(self) -> Self {
        match self {
            Self::Mesh3D(m) => Self::Mesh3D(Box::new(m.center())),
            Self::Sketch2D(s) => Self::Sketch2D(s.center()),
            Self::FallbackMesh(m) => Self::FallbackMesh(m.center()),
            Self::Failed(e) => Self::Failed(e),
        }
    }
}

pub mod conversions;
