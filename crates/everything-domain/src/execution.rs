use serde::{Deserialize, Serialize};
use std::fmt::{Display, Formatter};

#[derive(Debug, Clone, Copy, Eq, PartialEq, Serialize, Deserialize)]
pub enum ExecutionMode {
    Fast,
    Balanced,
    Deep,
}

impl Display for ExecutionMode {
    fn fmt(&self, f: &mut Formatter<'_>) -> std::fmt::Result {
        let value = match self {
            Self::Fast => "fast",
            Self::Balanced => "balanced",
            Self::Deep => "deep",
        };

        f.write_str(value)
    }
}

impl Default for ExecutionMode {
    fn default() -> Self {
        Self::Balanced
    }
}
