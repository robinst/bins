pub mod download;
pub mod upload;

use bins::error::*;
use hyper::Url;
use hyper::client::Response;
use std::io::Read;

pub fn parse_url<S: Into<String>>(url: S) -> Result<Url> {
  match Url::parse(&url.into()) {
    Ok(url) => Ok(url),
    Err(e) => Err(e.to_string().into()),
  }
}

pub fn read_response(response: &mut Response) -> Result<String> {
  let mut s = String::new();
  try!(response.read_to_string(&mut s));
  Ok(s)
}
