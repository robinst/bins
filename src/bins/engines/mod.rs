pub mod sprunge;

use bins::error::*;
use bins::network::download::Downloader;
use bins::network::upload::Uploader;
use bins::network;
use bins::{Bins, PasteFile};
use hyper::client::Response;
use hyper::Url;
use linked_hash_map::LinkedHashMap;
use std::io::Read;
use std::iter::repeat;
use std::collections::HashMap;

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

pub struct RemotePasteFile {
  pub name: String,
  pub url: Url
}

/// Produce information about HTML content from URLs to HTML content.
pub trait ProduceInfo {
  fn produce_info(&self, bins: &Bins, res: Response, urls: Vec<&Url>) -> Result<Vec<RemotePasteFile>>;
}

/// Produce information about raw content from URLs to HTML content.
pub trait ProduceRawInfo {
  fn produce_raw_info(&self, bins: &Bins, urls: Vec<&Url>) -> Result<Vec<RemotePasteFile>>;
}

/// Produce raw content from a URL to HTML content.
pub trait ProduceRawContent: ProduceRawInfo + Downloader {
  fn produce_raw_contents(&self, bins: &Bins, url: &Url) -> Result<Vec<PasteFile>> {
    let raw_info = try!(self.produce_raw_info(bins, vec![url]));
    let raw_info: Vec<RemotePasteFile> = if bins.arguments.files.len() > 0 {
      let files: Vec<String> = bins.arguments.files.iter().map(|s| s.to_lowercase()).collect();
      raw_info.into_iter().filter(|p| files.contains(&p.name.to_lowercase())).collect()
    } else if let Some(ref range) = bins.arguments.range {
      let mut numbered_info: HashMap<usize, RemotePasteFile> = raw_info.into_iter().enumerate().collect();
      try!(range.clone().into_iter().map(|n| numbered_info.remove(&n).ok_or(format!("file {} not found", n))).collect())
    } else if bins.arguments.all {
      raw_info
    } else {
      return Err("paste had multiple files, but no behavior was specified on the command line".into());
    };
    if bins.arguments.raw_urls {
      return Ok(vec![PasteFile {
                       name: "urls".to_owned(),
                       data: raw_info.into_iter().map(|r| r.url.as_str().to_owned()).collect::<Vec<_>>().join("\n")
                     }]);
    }
    let names: Vec<String> = raw_info.iter().map(|p| p.name.clone()).collect();
    let all_contents: Vec<String> = try!(raw_info.iter().map(|p| self.download(&p.url)).collect());
    let files: LinkedHashMap<String, String> = names.into_iter().zip(all_contents.into_iter()).collect();
    Ok(files.into_iter()
      .map(|(name, content)| {
        PasteFile {
          name: name.clone(),
          data: content.clone()
        }
      })
      .collect())
  }
}

/// Produce a URL to HTML content from raw content.
pub trait UploadContent: Uploader {
  fn upload_paste(&self, bins: &Bins, content: PasteFile) -> Result<Url>;
}

/// Produce a URL to HTML content from a batch of raw content.
pub trait UploadBatchContent: UploadContent {
  fn upload_all(&self, bins: &Bins, content: Vec<PasteFile>) -> Result<Url>;
}

impl<T> UploadBatchContent for T
  where T: GenerateIndex + UploadContent
{
  fn upload_all(&self, bins: &Bins, content: Vec<PasteFile>) -> Result<Url> {
    let index = try!(self.generate_index(bins, content));
    self.upload_paste(bins,
                      PasteFile {
                        name: "index.md".to_owned(),
                        data: index.to_string()
                      })
  }
}

/// Generate an index for multiple files.
pub trait GenerateIndex {
  fn generate_index(&self, bins: &Bins, content: Vec<PasteFile>) -> Result<Index>;
}

/// A bin, which can upload content in raw form and download content in raw and HTML form.
pub trait Bin: Sync + ProduceInfo + ProduceRawContent + UploadBatchContent {
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
