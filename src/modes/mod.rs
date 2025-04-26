use std::fmt;
use serde::{Serialize, Deserialize};

pub mod database;
pub mod file;
pub mod control_plane;
pub mod data_plane;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum OperationMode {
    Database,
    File,
    ControlPlane,
    DataPlane,
}

impl fmt::Display for OperationMode {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            OperationMode::Database => write!(f, "Database Mode"),
            OperationMode::File => write!(f, "File Mode"),
            OperationMode::ControlPlane => write!(f, "Control Plane Mode"),
            OperationMode::DataPlane => write!(f, "Data Plane Mode"),
        }
    }
}
