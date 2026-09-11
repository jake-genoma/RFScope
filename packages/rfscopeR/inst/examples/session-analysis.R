# Run after starting a local RFScope server and exporting observations.parquet.
library(rfscopeR)

connection <- rfscope_connect()
status <- rfscope_status(connection)
sessions <- rfscope_sessions(connection)
detections <- rfscope_detections(connection)

# DuckDB is optional; set RFSCOPE_DUCKDB_BIN when it is not on PATH.
observations <- rfscope_read_parquet("observations.parquet")
frequency_summary <- summarize_frequency(observations)

if (nrow(observations) > 0) {
  plot_signal_activity(
    data.frame(time = observations$observed_at_unix_ns, activity = observations$snr_db)
  )
}

list(status = status, sessions = sessions, detections = detections,
     frequency_summary = frequency_summary)
