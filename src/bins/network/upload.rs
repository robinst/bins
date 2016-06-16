use bins::error::*;
use hyper::client::{Client, Response};
use hyper::Url;

pub trait Uploader: ProduceUploadBody {
  fn upload(&self, url: &Url) -> Result<Response> {
    let body = &try!(self.produce_body());
    let client = Client::new();
    Ok(try!(client.post(url.as_str())
      .body(body)
      .send()))
  }
}

pub trait ProduceUploadBody {
  fn produce_body(&self) -> Result<String> {
    Ok(String::new())
  }
}
