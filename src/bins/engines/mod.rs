pub mod sprunge;

use bins::error::*;
use bins::{Bins, PasteFile};
use hyper::client::{Client, Response};
use hyper::Url;
use bins::network;
use linked_hash_map::LinkedHashMap;
use std::io::Read;
use std::iter::repeat;

pub struct Index {
  pub files: LinkedHashMap<String, Url>
}

impl Index {
  fn repeat_str(string: &str, count: usize) -> String {
    repeat(string).take(count).collect()
  }

  pub fn to_string(&self) -> String {
    let header = format!("{} files", self.files.len());
    let separator = Self::repeat_str("-", header.len());
    let mut body = String::from("");
    for (i, (name, url)) in self.files.iter().enumerate() {
      let number = i + 1;
      body.push_str(&format!("{number}. {name}: {url}\n",
                             number = number,
                             name = name,
                             url = url));
    }
    format!("{}\n{}\n\n{}", header, separator, body)
  }

  pub fn parse<S: Into<String>>(string: S) -> Result<Index> {
    let string = string.into();
    let lines: Vec<&str> = string.split('\n').collect();
    if lines.len() < 4 {
      return Err(ErrorKind::InvalidIndexError.into());
    }
    let mut split = lines.iter().skip(3).filter(|s| !s.trim().is_empty()).map(|s| s.split(' ')).collect::<Vec<_>>();
    let names: Vec<String> =
      some_or_err!(split.iter_mut().map(|s| s.nth(1).map(|x| x[..x.len() - 1].to_owned())).collect(),
                   ErrorKind::InvalidIndexError.into());
    let url_strings: Vec<String> = some_or_err!(split.iter_mut().map(|s| s.nth(0).map(|s| s.to_owned())).collect(),
                                                ErrorKind::InvalidIndexError.into());
    let urls: Vec<Url> = try!(url_strings.into_iter().map(|s| network::parse_url(s)).collect());
    let urls: LinkedHashMap<String, Url> = names.into_iter().zip(urls.into_iter()).collect();
    Ok(Index { files: urls })
  }
}

/// Produce URLs to HTML content from URLs to HTML content.
///
/// Generally, this should produce the same URLs as the URLs passed to it, unless the user is
/// requesting a specific file from a multi-file paste.
pub trait ProduceUrls {
  fn produce_urls(&self, bins: &Bins, res: Response, urls: Vec<&Url>) -> Result<Vec<Url>>;
}

/// Produce URLs to raw content from URLs to HTML content.
pub trait ProduceRawUrls {
  fn produce_raw_urls(&self, bins: &Bins, urls: Vec<&Url>) -> Result<Vec<Url>>;
}

/// Produce raw content from a URL to HTML content.
pub trait ProduceRawContent: ProduceRawUrls {
  fn produce_raw_contents(&self, bins: &Bins, url: &Url) -> Result<Vec<PasteFile>> {
    let client = Client::new();
    let raw_urls = try!(self.produce_raw_urls(bins, vec![url]));
    let url = some_or_err!(raw_urls.get(0),
                           "no urls available from raw_urls (this is a bug)".into());
    let mut res = try!(client.get(url.as_str()).send());
    let mut contents = String::new();
    try!(res.read_to_string(&mut contents));
    let name = some_or_err!(some_or_err!(url.path_segments(), "url was a root url".into()).last(),
                            "url did not have a last segment".into());
    Ok(vec![PasteFile::new(name.to_owned(), contents)])
  }
}

/// Produce a URL to HTML content from raw content.
pub trait UploadContent {
  fn upload(&self, bins: &Bins, content: PasteFile) -> Result<Url>;
}

/// Produce a URL to HTML content from a batch of raw content.
pub trait UploadBatchContent: UploadContent {
  fn upload_all(&self, bins: &Bins, content: Vec<PasteFile>) -> Result<Url>;
}

impl<T> UploadBatchContent for T
  where T: GeneratesIndex + UploadContent
{
  fn upload_all(&self, bins: &Bins, content: Vec<PasteFile>) -> Result<Url> {
    let index = try!(self.generate_index(bins, content));
    (self as &UploadContent).upload(bins,
                                    PasteFile {
                                      name: "index.md".to_owned(),
                                      data: index.to_string()
                                    })
  }
}

/// Generate an index for multiple files.
pub trait GeneratesIndex {
  fn generate_index(&self, bins: &Bins, content: Vec<PasteFile>) -> Result<Index>;
}

/// A bin, which can upload content in raw form and download content in raw and HTML form.
pub trait Bin: Sync + ProduceUrls + ProduceRawContent + UploadBatchContent {
  fn get_name(&self) -> &str;

  fn get_domain(&self) -> &str;
}

lazy_static! {
  pub static ref ENGINES: Vec<Box<Bin>> = {
      vec![
        Box::new(sprunge::Sprunge::new())
      ]
  };
}

pub fn get_bin_names<'a>() -> Vec<&'a str> {
  ENGINES.iter().map(|e| e.get_name()).collect()
}

pub fn get_bin_by_name<'a>(name: &'a str) -> Option<&Box<Bin>> {
  ENGINES.iter().find(|e| e.get_name().to_lowercase() == name.to_lowercase())
}

pub fn get_bin_by_domain<'a>(domain: &'a str) -> Option<&Box<Bin>> {
  ENGINES.iter().find(|e| e.get_domain().to_lowercase() == domain.to_lowercase())
}
