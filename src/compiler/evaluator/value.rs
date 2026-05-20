#[derive(Debug, Clone)]
pub enum Value {
    Number(f64),
    Bool(bool),
    List(Vec<Self>),
    String(String),
    Range(f64, f64, f64), // from, to, step
    Undef,
}

impl Value {
    #[must_use]
    pub const fn as_number(&self) -> Option<f64> {
        match self {
            Self::Number(n) => Some(*n),
            _ => None,
        }
    }

    #[must_use]
    pub fn as_bool(&self) -> bool {
        match self {
            Self::Bool(b) => *b,
            Self::Number(n) => *n != 0.0,
            Self::String(s) => !s.is_empty(),
            Self::List(l) => !l.is_empty(),
            Self::Undef => false,
            Self::Range(..) => true,
        }
    }

    #[must_use]
    pub fn as_list(&self) -> Option<&[Self]> {
        match self {
            Self::List(l) => Some(l),
            _ => None,
        }
    }

    #[must_use]
    pub fn to_number_list(&self) -> Option<Vec<f64>> {
        self.as_list()
            .map(|l| l.iter().filter_map(Self::as_number).collect())
    }

    /// Expand ranges into lists for iteration.
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
