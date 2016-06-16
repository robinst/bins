use bins::error::*;
use hyper::client::Client;
use hyper::Url;
use std::io::Read;

pub trait Downloader {
  fn download(&self, url: &Url) -> Result<String> {
    let client = Client::new();
    let mut res = try!(client.get(url.as_str()).send());
    let mut content = String::new();
    try!(res.read_to_string(&mut content));
    Ok(content)
  }
}
