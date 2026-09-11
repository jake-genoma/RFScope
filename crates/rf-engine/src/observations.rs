//! Arrow-compatible Parquet export for retained, low-rate spectrum observations.
use arrow_array::{ArrayRef, Float32Array, Float64Array, RecordBatch, UInt32Array, UInt64Array};
use arrow_schema::{DataType, Field, Schema};
use parquet::arrow::ArrowWriter;
use parquet::file::properties::WriterProperties;
use serde::{Deserialize, Serialize};
use std::{fs::File, path::Path, sync::Arc};
use thiserror::Error;

#[derive(Clone, Debug, Deserialize, Serialize, PartialEq)]
pub struct Observation {
    pub observed_at_unix_ns: u64,
    pub center_frequency_hz: u64,
    pub sample_rate_hz: u32,
    pub peak_frequency_hz: f64,
    pub peak_dbfs: f32,
    pub noise_floor_dbfs: f32,
    pub snr_db: f32,
    pub bandwidth_3db_hz: f64,
    pub bandwidth_6db_hz: f64,
    pub occupied_bandwidth_99_hz: f64,
}

#[derive(Debug, Error)]
pub enum ObservationError {
    #[error("Parquet: {0}")]
    Parquet(#[from] parquet::errors::ParquetError),
    #[error("Arrow: {0}")]
    Arrow(#[from] arrow_schema::ArrowError),
    #[error("I/O: {0}")]
    Io(#[from] std::io::Error),
}

pub fn schema() -> Arc<Schema> {
    Arc::new(Schema::new(vec![
        Field::new("observed_at_unix_ns", DataType::UInt64, false),
        Field::new("center_frequency_hz", DataType::UInt64, false),
        Field::new("sample_rate_hz", DataType::UInt32, false),
        Field::new("peak_frequency_hz", DataType::Float64, false),
        Field::new("peak_dbfs", DataType::Float32, false),
        Field::new("noise_floor_dbfs", DataType::Float32, false),
        Field::new("snr_db", DataType::Float32, false),
        Field::new("bandwidth_3db_hz", DataType::Float64, false),
        Field::new("bandwidth_6db_hz", DataType::Float64, false),
        Field::new("occupied_bandwidth_99_hz", DataType::Float64, false),
    ]))
}

pub fn write_parquet(
    path: impl AsRef<Path>,
    observations: &[Observation],
) -> Result<(), ObservationError> {
    let schema = schema();
    let columns: Vec<ArrayRef> = vec![
        Arc::new(UInt64Array::from_iter_values(
            observations.iter().map(|v| v.observed_at_unix_ns),
        )),
        Arc::new(UInt64Array::from_iter_values(
            observations.iter().map(|v| v.center_frequency_hz),
        )),
        Arc::new(UInt32Array::from_iter_values(
            observations.iter().map(|v| v.sample_rate_hz),
        )),
        Arc::new(Float64Array::from_iter_values(
            observations.iter().map(|v| v.peak_frequency_hz),
        )),
        Arc::new(Float32Array::from_iter_values(
            observations.iter().map(|v| v.peak_dbfs),
        )),
        Arc::new(Float32Array::from_iter_values(
            observations.iter().map(|v| v.noise_floor_dbfs),
        )),
        Arc::new(Float32Array::from_iter_values(
            observations.iter().map(|v| v.snr_db),
        )),
        Arc::new(Float64Array::from_iter_values(
            observations.iter().map(|v| v.bandwidth_3db_hz),
        )),
        Arc::new(Float64Array::from_iter_values(
            observations.iter().map(|v| v.bandwidth_6db_hz),
        )),
        Arc::new(Float64Array::from_iter_values(
            observations.iter().map(|v| v.occupied_bandwidth_99_hz),
        )),
    ];
    let batch = RecordBatch::try_new(schema.clone(), columns)?;
    let file = File::create(path)?;
    let props = WriterProperties::builder()
        .set_created_by("RFScope".into())
        .build();
    let mut writer = ArrowWriter::try_new(file, schema, Some(props))?;
    writer.write(&batch)?;
    writer.close()?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use arrow_array::RecordBatchReader;
    #[test]
    fn writes_arrow_compatible_parquet_schema() {
        let path = std::env::temp_dir().join(format!(
            "rfscope-observations-{}.parquet",
            std::process::id()
        ));
        let row = Observation {
            observed_at_unix_ns: 1,
            center_frequency_hz: 100_000_000,
            sample_rate_hz: 2_000_000,
            peak_frequency_hz: 100_000_100.0,
            peak_dbfs: -3.0,
            noise_floor_dbfs: -70.0,
            snr_db: 67.0,
            bandwidth_3db_hz: 1_000.0,
            bandwidth_6db_hz: 2_000.0,
            occupied_bandwidth_99_hz: 5_000.0,
        };
        write_parquet(&path, &[row]).unwrap();
        let reader = parquet::arrow::arrow_reader::ParquetRecordBatchReaderBuilder::try_new(
            File::open(&path).unwrap(),
        )
        .unwrap()
        .build()
        .unwrap();
        assert_eq!(reader.schema().fields().len(), 10);
        let _ = std::fs::remove_file(path);
    }
}
