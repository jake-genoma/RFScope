\name{rfscopeR}
\alias{rfscope_connect}
\alias{rfscope_status}
\alias{rfscope_devices}
\alias{rfscope_vfos}
\alias{rfscope_sessions}
\alias{rfscope_workspaces}
\alias{rfscope_bookmarks}
\alias{rfscope_annotations}
\alias{rfscope_recordings}
\alias{rfscope_detections}
\alias{rfscope_query}
\alias{rfscope_read_parquet}
\alias{plot_rf_occupancy}
\alias{plot_signal_activity}
\alias{summarize_frequency}
\alias{compare_sessions}
\title{RFScope low-rate API and analysis helpers}
\description{Connect to a local RFScope server, retrieve low-rate metadata and
analysis results, read exported Parquet observations, and make basic plots. These
functions never process real-time IQ.}
\usage{
rfscope_connect(url = "http://127.0.0.1:8787")
rfscope_status(connection = rfscope_connect())
rfscope_devices(connection = rfscope_connect())
rfscope_vfos(connection = rfscope_connect())
rfscope_sessions(connection = rfscope_connect())
rfscope_workspaces(connection = rfscope_connect())
rfscope_bookmarks(connection = rfscope_connect())
rfscope_annotations(connection = rfscope_connect())
rfscope_recordings(connection = rfscope_connect())
rfscope_detections(connection = rfscope_connect())
rfscope_query(connection = rfscope_connect(), path = "/api/v1/analysis")
rfscope_read_parquet(path, limit = 1000)
plot_rf_occupancy(data, frequency = "frequency_hz", occupancy = "occupancy")
plot_signal_activity(data, time = "time", activity = "activity")
summarize_frequency(data, frequency = "peak_frequency_hz")
compare_sessions(left, right, frequency = "peak_frequency_hz")
}
\arguments{
  \item{connection}{An \code{rfscope_connection}.}
  \item{url}{RFScope server base URL.}
  \item{path}{A server API path or exported Parquet file path.}
  \item{limit}{Maximum Parquet rows.}
  \item{data,left,right}{Data frames or list-like analysis inputs.}
  \item{frequency,occupancy,time,activity}{Column names used by the plotting and summary helpers.}
}
\value{A connection, decoded JSON response, Parquet data, base-R plot, or summary list as appropriate.}
\examples{
conn <- rfscope_connect()
\dontrun{rfscope_status(conn)}
}
