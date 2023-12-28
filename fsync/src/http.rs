use hyper::{client::HttpConnector, Body};
use hyper_rustls::HttpsConnector;

pub type Connector = HttpsConnector<HttpConnector>;

#[derive(Debug, thiserror::Error)]
pub enum Error {
    #[error("Status")]
    Status(http::Response<Body>),
}
