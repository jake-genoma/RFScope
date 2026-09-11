#' Connect to RFScope
#' @param url Server base URL.
#' @export
rfscope_connect <- function(url = "http://127.0.0.1:8787") structure(list(url = sub("/$", "", url)), class = "rfscope_connection")

#' Read current RFScope status
#' @param connection An rfscope_connection.
#' @export
rfscope_status <- function(connection = rfscope_connect()) jsonlite::fromJSON(paste0(connection$url, "/api/v1/status"))

rfscope_get <- function(connection, path) jsonlite::fromJSON(paste0(connection$url, path), simplifyVector = FALSE)
rfscope_devices <- function(connection = rfscope_connect()) rfscope_get(connection, "/api/v1/devices")
rfscope_vfos <- function(connection = rfscope_connect()) rfscope_get(connection, "/api/v1/vfos")
rfscope_sessions <- function(connection = rfscope_connect()) rfscope_get(connection, "/api/v1/sessions")
rfscope_workspaces <- function(connection = rfscope_connect()) rfscope_get(connection, "/api/v1/workspaces")
rfscope_recordings <- function(connection = rfscope_connect()) rfscope_get(connection, "/api/v1/recordings")
rfscope_detections <- function(connection = rfscope_connect()) rfscope_get(connection, "/api/v1/detections")
rfscope_query <- function(connection = rfscope_connect(), path = "/api/v1/analysis") rfscope_get(connection, path)

#' Read an exported Parquet observation file through DuckDB
#' @param path Parquet file path.
#' @param limit Maximum rows to return.
#' @export
rfscope_read_parquet <- function(path, limit = 1000) {
  executable <- Sys.getenv("RFSCOPE_DUCKDB_BIN", "duckdb")
  if (!nzchar(Sys.which(executable))) stop("DuckDB executable not found; install duckdb or set RFSCOPE_DUCKDB_BIN")
  escaped <- gsub("'", "''", path, fixed = TRUE)
  sql <- sprintf("SELECT * FROM read_parquet('%s') LIMIT %d", escaped, min(as.integer(limit), 100000L))
  output <- system2(executable, c("-json", "-c", sql), stdout = TRUE, stderr = TRUE)
  jsonlite::fromJSON(paste(output, collapse = ""))
}

plot_rf_occupancy <- function(data, frequency = "frequency_hz", occupancy = "occupancy") graphics::plot(data[[frequency]], data[[occupancy]], type = "l", xlab = "Frequency (Hz)", ylab = "Occupancy")
plot_signal_activity <- function(data, time = "time", activity = "activity") graphics::plot(data[[time]], data[[activity]], type = "l", xlab = "Time", ylab = "Activity")
summarize_frequency <- function(data, frequency = "peak_frequency_hz") stats::aggregate(data[[frequency]], list(frequency = data[[frequency]]), FUN = length)
compare_sessions <- function(left, right, frequency = "peak_frequency_hz") list(left = left[[frequency]], right = right[[frequency]], delta_hz = right[[frequency]] - left[[frequency]])
