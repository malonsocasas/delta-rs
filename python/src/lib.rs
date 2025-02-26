#![deny(warnings)]

extern crate arrow;
extern crate pyo3;

use arrow::datatypes::Schema as ArrowSchema;
use deltalake::partitions::PartitionFilter;
use pyo3::create_exception;
use pyo3::exceptions::PyException;
use pyo3::prelude::*;
use std::collections::HashMap;
use std::convert::TryFrom;

create_exception!(deltalake, PyDeltaTableError, PyException);

impl PyDeltaTableError {
    fn from_arrow(err: arrow::error::ArrowError) -> pyo3::PyErr {
        PyDeltaTableError::new_err(err.to_string())
    }

    fn from_raw(err: deltalake::DeltaTableError) -> pyo3::PyErr {
        PyDeltaTableError::new_err(err.to_string())
    }

    fn from_tokio(err: tokio::io::Error) -> pyo3::PyErr {
        PyDeltaTableError::new_err(err.to_string())
    }
}

#[inline]
fn rt() -> PyResult<tokio::runtime::Runtime> {
    tokio::runtime::Runtime::new().map_err(PyDeltaTableError::from_tokio)
}

#[derive(FromPyObject)]
enum PartitionFilterValue<'a> {
    Single(&'a str),
    Multiple(Vec<&'a str>),
}

#[pyclass]
struct RawDeltaTable {
    _table: deltalake::DeltaTable,
}

#[pyclass]
struct RawDeltaTableMetaData {
    #[pyo3(get)]
    id: String,
    #[pyo3(get)]
    name: Option<String>,
    #[pyo3(get)]
    description: Option<String>,
    #[pyo3(get)]
    partition_columns: Vec<String>,
    #[pyo3(get)]
    created_time: deltalake::DeltaDataTypeTimestamp,
    #[pyo3(get)]
    configuration: HashMap<String, String>,
}

#[pymethods]
impl RawDeltaTable {
    #[new]
    fn new(table_path: &str, version: Option<deltalake::DeltaDataTypeLong>) -> PyResult<Self> {
        let table = match version {
            None => rt()?.block_on(deltalake::open_table(table_path)),
            Some(version) => {
                rt()?.block_on(deltalake::open_table_with_version(table_path, version))
            }
        }
        .map_err(PyDeltaTableError::from_raw)?;
        Ok(RawDeltaTable { _table: table })
    }

    pub fn table_path(&self) -> PyResult<&str> {
        Ok(&self._table.table_path)
    }

    pub fn version(&self) -> PyResult<i64> {
        Ok(self._table.version)
    }

    pub fn metadata(&self) -> PyResult<RawDeltaTableMetaData> {
        let metadata = self
            ._table
            .get_metadata()
            .map_err(PyDeltaTableError::from_raw)?;
        Ok(RawDeltaTableMetaData {
            id: metadata.id.clone(),
            name: metadata.name.clone(),
            description: metadata.description.clone(),
            partition_columns: metadata.partition_columns.clone(),
            created_time: metadata.created_time,
            configuration: metadata.configuration.clone(),
        })
    }

    pub fn load_version(&mut self, version: deltalake::DeltaDataTypeVersion) -> PyResult<()> {
        rt()?
            .block_on(self._table.load_version(version))
            .map_err(PyDeltaTableError::from_raw)
    }

    pub fn files_by_partitions(
        &self,
        partitions_filters: Vec<(&str, &str, PartitionFilterValue)>,
    ) -> PyResult<Vec<String>> {
        let partition_filters: Result<Vec<PartitionFilter<&str>>, deltalake::DeltaTableError> =
            partitions_filters
                .into_iter()
                .map(|filter| match filter {
                    (key, op, PartitionFilterValue::Single(v)) => {
                        PartitionFilter::try_from((key, op, v))
                    }
                    (key, op, PartitionFilterValue::Multiple(v)) => {
                        PartitionFilter::try_from((key, op, v))
                    }
                })
                .collect();
        match partition_filters {
            Ok(filters) => Ok(self
                ._table
                .get_files_by_partitions(&filters)
                .map_err(PyDeltaTableError::from_raw)?),
            Err(err) => Err(PyDeltaTableError::from_raw(err)),
        }
    }

    pub fn files(&self) -> PyResult<Vec<String>> {
        Ok(self._table.get_files().to_vec())
    }

    pub fn file_paths(&self) -> PyResult<Vec<String>> {
        Ok(self._table.get_file_paths())
    }

    pub fn schema_json(&self) -> PyResult<String> {
        let schema = self
            ._table
            .get_schema()
            .map_err(PyDeltaTableError::from_raw)?;
        serde_json::to_string(&schema)
            .map_err(|_| PyDeltaTableError::new_err("Got invalid table schema"))
    }

    /// Run the Vacuum command on the Delta Table: lists and removes files no longer referenced by the Delta table and are older than the retention threshold.
    pub fn vacuum(&self, dry_run: bool, retention_hours: u64) -> PyResult<Vec<String>> {
        match dry_run {
            true => Ok(self
                ._table
                .vacuum_dry_run(retention_hours)
                .map_err(PyDeltaTableError::from_raw)?),
            false => unimplemented!("Only Vacuum with dry_run is available."),
        }
    }

    pub fn arrow_schema_json(&self) -> PyResult<String> {
        let schema = self
            ._table
            .get_schema()
            .map_err(PyDeltaTableError::from_raw)?;
        serde_json::to_string(
            &<ArrowSchema as TryFrom<&deltalake::Schema>>::try_from(schema)
                .map_err(PyDeltaTableError::from_arrow)?
                .to_json(),
        )
        .map_err(|_| PyDeltaTableError::new_err("Got invalid table schema"))
    }
}

#[pyfunction]
fn rust_core_version() -> &'static str {
    deltalake::crate_version()
}

#[pymodule]
// module name need to match project name
fn deltalake(py: Python, m: &PyModule) -> PyResult<()> {
    env_logger::Builder::from_env(env_logger::Env::default().default_filter_or("warn")).init();

    m.add_function(pyo3::wrap_pyfunction!(rust_core_version, m)?)?;
    m.add_class::<RawDeltaTable>()?;
    m.add_class::<RawDeltaTableMetaData>()?;
    m.add("DeltaTableError", py.get_type::<PyDeltaTableError>())?;
    Ok(())
}
