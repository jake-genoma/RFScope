# rfscopeR

`rfscopeR` is the low-rate R client for RFScope. It retrieves status, devices, VFOs, sessions, recordings, detections, and analysis results from the HTTP API. Plotting helpers use base R; raw IQ and real-time DSP remain in Rust.

```r
conn <- rfscope_connect()
rfscope_status(conn)
rfscope_query(conn)
rfscope_read_parquet("observations.parquet")
```

`rfscope_read_parquet()` uses DuckDB's CLI when installed. The dependency is optional and is never used for real-time IQ processing.

`rfscope_bookmarks()` and `rfscope_annotations()` expose the transactional metadata stored by the workstation.

`rfscope_detections()` returns bounded, uncalibrated spectrum events with start/end times when their SNR threshold lifecycle completes.
