use std::fmt;

use csgrs::Profile;
use csgrs::Real;
use csgrs::csg::CSG;
use csgrs::mesh::Mesh as CsgMesh;
use csgrs::mesh::plane::Plane;
use hyperlattice::Vector3;

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
#[allow(clippy::large_enum_variant)]
pub enum Shape {
    Mesh3D(CsgMesh<()>),
    Sketch2D(Profile<()>),
    /// Boolean/transform operations failed with this error.
    Failed(String),
}

impl fmt::Debug for Shape {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Mesh3D(_) => write!(f, "Shape::Mesh3D"),
            Self::Sketch2D(_) => write!(f, "Shape::Sketch2D"),
            Self::Failed(e) => write!(f, "Shape::Failed({e})"),
        }
    }
}

impl Shape {
    /// Create a 3D shape from a `CsgMesh` primitive.
    pub const fn from_csg_mesh(mesh: CsgMesh<()>) -> Self {
        Self::Mesh3D(mesh)
    }

    /// Extract polygon mesh data for downstream boolean + hull operations.
    pub fn into_csg_mesh(self) -> CsgMesh<()> {
        match self {
            Self::Mesh3D(m) => m,
            Self::Sketch2D(s) => s.extrude(Self::to_real(0.01)),
            Self::Failed(_) => CsgMesh::new(),
        }
    }

    fn to_real(value: f64) -> Real {
        Real::try_from(value).ok().unwrap_or_else(Real::zero)
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
            (a, b) => Self::from_csg_mesh(csg_bool(
                a.into_csg_mesh(),
                b.into_csg_mesh(),
                BoolOp::Union,
            )),
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
            (a, b) => Self::from_csg_mesh(csg_bool(
                a.into_csg_mesh(),
                b.into_csg_mesh(),
                BoolOp::Difference,
            )),
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
            (a, b) => Self::from_csg_mesh(csg_bool(
                a.into_csg_mesh(),
                b.into_csg_mesh(),
                BoolOp::Intersection,
            )),
        }
    }

    #[must_use]
    pub fn translate(self, x: f64, y: f64, z: f64) -> Self {
        let (x, y, z) = (Self::to_real(x), Self::to_real(y), Self::to_real(z));
        let zero = Self::to_real(0.0);
        let epsilon = Self::to_real(1e-12);
        match self {
            Self::Mesh3D(m) => Self::Mesh3D(m.translate(x, y, z)),
            Self::Sketch2D(s) => {
                if z.abs() < epsilon {
                    Self::Sketch2D(s.translate(x, y, zero))
                } else {
                    Self::from_csg_mesh(s.extrude(Self::to_real(0.01)).translate(x, y, z))
                }
            }
            Self::Failed(e) => Self::Failed(e),
        }
    }

    #[must_use]
    pub fn rotate(self, x: f64, y: f64, z: f64) -> Self {
        let (x, y, z) = (Self::to_real(x), Self::to_real(y), Self::to_real(z));
        let zero = Self::to_real(0.0);
        let epsilon = Self::to_real(1e-12);
        match self {
            Self::Mesh3D(m) => Self::Mesh3D(m.rotate(x, y, z)),
            Self::Sketch2D(s) => {
                if x.abs() < epsilon && y.abs() < epsilon {
                    Self::Sketch2D(s.rotate(zero.clone(), zero, z))
                } else {
                    Self::from_csg_mesh(s.extrude(Self::to_real(0.01)).rotate(x, y, z))
                }
            }
            Self::Failed(e) => Self::Failed(e),
        }
    }

    #[must_use]
    pub fn scale(self, sx: f64, sy: f64, sz: f64) -> Self {
        let (sx, sy, sz) = (Self::to_real(sx), Self::to_real(sy), Self::to_real(sz));
        let one = Self::to_real(1.0);
        let epsilon = Self::to_real(1e-12);
        match self {
            Self::Mesh3D(m) => Self::Mesh3D(m.scale(sx, sy, sz)),
            Self::Sketch2D(s) => {
                if (sz.clone() - one.clone()).abs() < epsilon {
                    Self::Sketch2D(s.scale(sx, sy, one))
                } else {
                    Self::from_csg_mesh(s.extrude(Self::to_real(0.01)).scale(sx, sy, sz))
                }
            }
            Self::Failed(e) => Self::Failed(e),
        }
    }

    #[must_use]
    pub fn mirror(self, nx: f64, ny: f64, nz: f64) -> Self {
        let len = (nx.mul_add(nx, ny.mul_add(ny, nz * nz))).sqrt();
        if len < 1e-12 {
            return self;
        }
        let plane = Plane::from_normal(
            Vector3::new([Self::to_real(nx), Self::to_real(ny), Self::to_real(nz)]),
            Real::zero(),
        );
        match self {
            Self::Mesh3D(m) => Self::Mesh3D(m.mirror(plane)),
            Self::Sketch2D(s) => Self::Sketch2D(s.mirror(plane)),
            Self::Failed(e) => Self::Failed(e),
        }
    }

    #[must_use]
    #[allow(dead_code)]
    pub fn center(self) -> Self {
        match self {
            Self::Mesh3D(m) => Self::Mesh3D(m.center()),
            Self::Sketch2D(s) => Self::Sketch2D(s.center()),
            Self::Failed(e) => Self::Failed(e),
        }
    }
}

fn csg_bool(lhs: CsgMesh<()>, rhs: CsgMesh<()>, op: BoolOp) -> CsgMesh<()> {
    match op {
        BoolOp::Union => lhs.union(&rhs),
        BoolOp::Difference => lhs.difference(&rhs),
        BoolOp::Intersection => lhs.intersection(&rhs),
    }
}

pub mod conversions;
