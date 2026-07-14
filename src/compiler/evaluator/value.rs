use csgrs::Real;

/// Runtime value produced by the `OpenSCAD` expression evaluator.
#[derive(Debug, Clone)]
pub enum Value {
    /// Ordinary binary floating-point number.
    Number(f64),
    /// Exact rational or symbolic Hyper number.
    Exact(Real),
    /// Boolean value.
    Bool(bool),
    /// `OpenSCAD` vector or list.
    List(Vec<Self>),
    /// UTF-8 string.
    String(String),
    /// Inclusive `(start, end, step)` numeric range.
    Range(f64, f64, f64),
    /// Undefined value.
    Undef,
}

impl Value {
    /// Returns a lossy primitive approximation for either numeric variant.
    #[must_use]
    pub fn as_number(&self) -> Option<f64> {
        match self {
            Self::Number(n) => Some(*n),
            Self::Exact(n) => n.to_f64_lossy(),
            _ => None,
        }
    }

    /// Returns an exact representation for either numeric variant.
    #[must_use]
    pub fn as_real(&self) -> Option<Real> {
        match self {
            Self::Number(n) => Real::try_from(*n).ok(),
            Self::Exact(n) => Some(n.clone()),
            _ => None,
        }
    }

    /// Applies `OpenSCAD` truthiness rules.
    #[must_use]
    pub fn as_bool(&self) -> bool {
        match self {
            Self::Bool(b) => *b,
            Self::Number(n) => *n != 0.0,
            Self::Exact(n) => n != &Real::zero(),
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

    /// Converts the list elements that are numeric to primitive approximations.
    #[must_use]
    pub fn to_number_list(&self) -> Option<Vec<f64>> {
        self.as_list()
            .map(|l| l.iter().filter_map(Self::as_number).collect())
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
                let mut v = *from;
                if *step > 0.0 {
                    #[allow(clippy::while_float)]
                    while v <= *to + 1e-12 {
                        vals.push(Self::Number(v));
                        v += step;
                    }
                } else if *step < 0.0 {
                    #[allow(clippy::while_float)]
                    while v >= *to - 1e-12 {
                        vals.push(Self::Number(v));
                        v += step;
                    }
                }
                vals
            }
            Self::List(l) => l.clone(),
            _ => vec![self.clone()],
        }
    }
}
