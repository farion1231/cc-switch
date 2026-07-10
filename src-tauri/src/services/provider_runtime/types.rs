use crate::Provider;
use indexmap::IndexMap;
use serde_json::{Map, Value};

pub type DbProvidersMap = IndexMap<String, Provider>;
pub type PiProvidersMap = Map<String, Value>;

#[derive(Debug)]
pub enum ProviderRuntimeProviders {
    Db(DbProvidersMap),
    Pi(PiProvidersMap),
}

#[derive(Debug, Clone)]
pub struct PiProviderWriteResult {
    pub file_hash: String,
    pub models_json: Value,
    pub backup_path: String,
}
