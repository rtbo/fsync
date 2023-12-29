use hyper::client::HttpConnector;
use hyper_rustls::HttpsConnector;

pub type Connector = HttpsConnector<HttpConnector>;
