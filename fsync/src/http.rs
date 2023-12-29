use hyper::{client::HttpConnector, Body};
use hyper_rustls::HttpsConnector;

pub type Connector = HttpsConnector<HttpConnector>;

#[derive(Debug, thiserror::Error)]
pub enum Error {
    #[error("HTTP status error: {0}")]
    Status(http::status::StatusCode, http::Response<Body>),
}
