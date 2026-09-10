#' Connect to RFScope
#' @param url Server base URL.
#' @export
rfscope_connect <- function(url = "http://127.0.0.1:8787") structure(list(url = sub("/$", "", url)), class = "rfscope_connection")

#' Read current RFScope status
#' @param connection An rfscope_connection.
#' @export
rfscope_status <- function(connection = rfscope_connect()) jsonlite::fromJSON(paste0(connection$url, "/api/v1/status"))
