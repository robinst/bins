use bins::engines::{Bin, GeneratesIndex, Index, ProduceRawContent, ProduceRawUrls, ProduceUrls, UploadContent};
use bins::error::*;
use bins::network::download::Downloader;
use bins::network::upload::{ProduceUploadBody, Uploader};
use bins::network;
use bins::{Bins, PasteFile};
use hyper::client::Response;
use hyper::Url;
use linked_hash_map::LinkedHashMap;
use std::cell::RefCell;
use url::form_urlencoded;

pub struct Sprunge {
  body: RefCell<Option<String>>
}

impl Sprunge {
  pub fn new() -> Self {
    Sprunge { body: RefCell::new(None) }
  }
}

impl Bin for Sprunge {
  fn get_name(&self) -> &str {
    "sprunge"
  }

  fn get_domain(&self) -> &str {
    "sprunge.us"
  }
}

unsafe impl Sync for Sprunge {}

impl ProduceUrls for Sprunge {
  fn produce_urls(&self, bins: &Bins, res: Response, urls: Vec<&Url>) -> Result<Vec<Url>> {
    Ok(urls.into_iter().map(|u| u.clone()).collect())
  }
}

impl ProduceRawUrls for Sprunge {
  fn produce_raw_urls(&self, bins: &Bins, urls: Vec<&Url>) -> Result<Vec<Url>> {
    let raw_urls: Vec<Url> = urls.into_iter()
      .map(|u| {
        let mut u = u.clone();
        u.set_query(None);
        u
      })
      .collect();
    let indices: LinkedHashMap<Url, Result<Index>> = try!(raw_urls.into_iter()
      .map(|u| self.download(&u).map(|c| (u, Index::parse(c))))
      .collect());
    let mut urls: Vec<Url> = vec![];
    for (url, res) in indices.into_iter() {
      match *res {
        Ok(ref i) => {
          for (_, url) in i.files.into_iter() {
            urls.push(url.clone());
          }
        }
        Err(ref e) => {
          if let ErrorKind::InvalidIndexError = *e.kind() {} else {
            return Err(e.to_string().into());
          }
          urls.push(url.clone());
        }
      }
    }
    Ok(urls)
  }
}

impl ProduceRawContent for Sprunge {}

impl GeneratesIndex for Sprunge {
  fn generate_index(&self, bins: &Bins, content: Vec<PasteFile>) -> Result<Index> {
    let names: Vec<String> = (&content).into_iter().map(|p| p.name.clone()).collect();
    let urls: Vec<Url> = try!(content.into_iter().map(|p| (self as &UploadContent).upload(bins, p)).collect());
    let uploads: LinkedHashMap<String, Url> = names.into_iter().zip(urls.into_iter()).collect();
    Ok(Index { files: uploads })
  }
}

impl ProduceUploadBody for Sprunge {
  fn produce_body(&self) -> Result<String> {
    let body = self.body.borrow();
    let body = some_ref_or_err!(body,
                                "no body was prepared for upload (this is a bug)".into());
    Ok(form_urlencoded::Serializer::new(String::new())
      .append_pair("sprunge", &body)
      .finish())
  }
}

impl Uploader for Sprunge {}

impl Downloader for Sprunge {}

impl UploadContent for Sprunge {
  fn upload(&self, bins: &Bins, content: PasteFile) -> Result<Url> {
    let url = try!(network::parse_url("http://sprunge.us/"));
    *self.body.borrow_mut() = Some(content.data);
    let mut response = try!((self as &Uploader).upload(&url));
    *self.body.borrow_mut() = None;
    network::parse_url(try!(network::read_response(&mut response)))
  }
}
