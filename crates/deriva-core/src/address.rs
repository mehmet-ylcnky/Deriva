use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;
use std::fmt;

// --- FunctionId ---

#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize, PartialOrd, Ord)]
pub struct FunctionId {
    pub name: String,
    pub version: String,
}

impl FunctionId {
    pub fn new(name: impl Into<String>, version: impl Into<String>) -> Self {
        Self {
            name: name.into(),
            version: version.into(),
        }
    }
}

impl fmt::Display for FunctionId {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}/{}", self.name, self.version)
    }
}

// --- Value ---

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, PartialOrd, Ord)]
pub enum Value {
    String(String),
    Int(i64),
    Bool(bool),
    Bytes(Vec<u8>),
    List(Vec<Value>),
}

// --- CAddr ---

#[derive(Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize, PartialOrd, Ord)]
pub struct CAddr([u8; 32]);

impl CAddr {
    pub fn from_bytes(data: &[u8]) -> Self {
        Self(blake3::hash(data).into())
    }

    pub fn from_raw(hash: [u8; 32]) -> Self {
        Self(hash)
    }

    pub fn as_bytes(&self) -> &[u8; 32] {
        &self.0
    }

    pub fn to_hex(&self) -> String {
        self.0.iter().map(|b| format!("{:02x}", b)).collect()
    }
}

impl fmt::Debug for CAddr {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "CAddr({}…)", &self.to_hex()[..12])
    }
}

impl fmt::Display for CAddr {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}", self.to_hex())
    }
}

// --- Recipe ---

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Recipe {
    pub function_id: FunctionId,
    pub inputs: Vec<CAddr>,
    pub params: BTreeMap<String, Value>,
}

impl Recipe {
    pub fn new(
        function_id: FunctionId,
        inputs: Vec<CAddr>,
        params: BTreeMap<String, Value>,
    ) -> Self {
        Self { function_id, inputs, params }
    }

    pub fn canonical_bytes(&self) -> Vec<u8> {
        bincode::serialize(self).expect("recipe serialization is infallible for valid types")
    }

    pub fn addr(&self) -> CAddr {
        CAddr(blake3::hash(&self.canonical_bytes()).into())
    }
}

// --- DataRef ---

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum DataRef {
    Leaf { addr: CAddr, size: u64 },
    Derived { addr: CAddr, recipe: Recipe },
}

impl DataRef {
    pub fn leaf(data: &[u8]) -> Self {
        Self::Leaf {
            addr: CAddr::from_bytes(data),
            size: data.len() as u64,
        }
    }

    pub fn derived(recipe: Recipe) -> Self {
        let addr = recipe.addr();
        Self::Derived { addr, recipe }
    }

    pub fn addr(&self) -> &CAddr {
        match self {
            Self::Leaf { addr, .. } => addr,
            Self::Derived { addr, .. } => addr,
        }
    }
}
