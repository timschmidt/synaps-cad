use csgrs::Real;

pub(crate) fn reals_equal(left: &Real, right: &Real) -> bool {
    left.certified_eq_until(right, Real::PARTIAL_CMP_MIN_PRECISION)
        .as_bool()
        .unwrap_or(false)
}

/// Runtime value produced by the `OpenSCAD` expression evaluator.
#[derive(Debug, Clone)]
pub enum Value {
    /// Exact rational, symbolic, or computable real number.
    Number(Real),
    /// Boolean value.
    Bool(bool),
    /// `OpenSCAD` vector or list.
    List(Vec<Self>),
    /// UTF-8 string.
    String(String),
    /// Inclusive `(start, end, step)` exact-real range.
    Range(Real, Real, Real),
    /// Undefined value.
    Undef,
}

impl Value {
    /// Returns the exact-real numeric value.
    #[must_use]
    pub fn as_real(&self) -> Option<Real> {
        match self {
            Self::Number(n) => Some(n.clone()),
            _ => None,
        }
    }

    /// Returns a lossy primitive approximation at an explicit interoperability boundary.
    #[must_use]
    pub fn to_f64_lossy(&self) -> Option<f64> {
        match self {
            Self::Number(n) => n.to_f64_lossy(),
            _ => None,
        }
    }

    /// Returns a nonnegative exact integer as `usize` for indexing/count APIs.
    #[must_use]
    pub fn to_usize_exact(&self) -> Option<usize> {
        let integer = self.as_real()?.exact_rational()?.to_big_integer()?;
        usize::try_from(integer).ok()
    }

    /// Returns a nonnegative exact integer as `u64` for deterministic seeds.
    #[must_use]
    pub fn to_u64_exact(&self) -> Option<u64> {
        let integer = self.as_real()?.exact_rational()?.to_big_integer()?;
        u64::try_from(integer).ok()
    }

    /// Returns a nonnegative exact integer as `u32` for character conversion.
    #[must_use]
    pub fn to_u32_exact(&self) -> Option<u32> {
        let integer = self.as_real()?.exact_rational()?.to_big_integer()?;
        u32::try_from(integer).ok()
    }

    /// Applies `OpenSCAD` truthiness rules.
    #[must_use]
    pub fn as_bool(&self) -> bool {
        match self {
            Self::Bool(b) => *b,
            Self::Number(n) => !reals_equal(n, &Real::zero()),
            Self::String(s) => !s.is_empty(),
            Self::List(l) => !l.is_empty(),
            Self::Undef => false,
            Self::Range(..) => true,
        }
    }

    /// Borrows the elements when this is a list.
    #[must_use]
    pub fn as_list(&self) -> Option<&[Self]> {
        match self {
            Self::List(l) => Some(l),
            _ => None,
        }
    }

    /// Converts the list elements that are numeric to exact values.
    #[must_use]
    pub fn to_real_list(&self) -> Option<Vec<Real>> {
        self.as_list()
            .map(|l| l.iter().filter_map(Self::as_real).collect())
    }

    /// Expands ranges and lists into values suitable for `for` iteration.
    #[must_use]
    pub fn to_iterable(&self) -> Vec<Self> {
        match self {
            Self::Range(from, to, step) => {
                let mut vals = Vec::new();
                let mut value = from.clone();
                if step > &Real::zero() {
                    while value <= *to {
                        vals.push(Self::Number(value.clone()));
                        value += step;
                    }
                } else if step < &Real::zero() {
                    while value >= *to {
                        vals.push(Self::Number(value.clone()));
                        value += step;
                    }
                }
                vals
            }
            Self::List(l) => l.clone(),
            _ => vec![self.clone()],
        }
    }
}
