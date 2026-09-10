# rfscopeR

`rfscopeR` is the low-rate R client for RFScope. It retrieves status, devices, VFOs, sessions, recordings, detections, and analysis results from the HTTP API. Plotting helpers use base R; raw IQ and real-time DSP remain in Rust.

```r
conn <- rfscope_connect()
rfscope_status(conn)
rfscope_query(conn)
```
