use bins::error::*;
use bins::{Bins, PasteFile};
use bins::engines::Engine;
use bins::engines::indexed::{IndexedDownload, DownloadsFile};
use bins::configuration::BetterLookups;
use hyper::client::Client;
use hyper::header::{Headers, ContentType, UserAgent, Authorization, Basic};
use hyper::status::StatusCode;
use std::collections::BTreeMap;
use std::io::Read;
use rustc_serialize::json::{self, Json};
use hyper::Url;

#[derive(RustcEncodable, RustcDecodable)]
struct GistUpload {
  files: BTreeMap<String, GistFile>,
  description: String,
  public: bool
}

impl GistUpload {
  fn new(description: Option<String>, public: bool) -> Self {
    let map = BTreeMap::new();
    GistUpload {
      files: map,
      description: description.unwrap_or_else(String::new),
      public: public
    }
  }

  fn from(bins: &Bins, files: &[PasteFile]) -> Self {
    let mut gist = GistUpload::new(None, !bins.arguments.private);
    for file in files {
      gist.files.insert(file.name.clone(), GistFile::from(file.data.clone()));
    }
    gist
  }

  fn get_url(&self, bins: &Bins, nth: Option<usize>) -> Result<String> {
    let target_file = bins.arguments.files.get(0);
    let files: BTreeMap<String, &GistFile> = self.files.iter().map(|(k, v)| (k.to_lowercase(), v)).collect();
    if files.len() < 1 {
      return Err("gist had no files".into());
    }
    if files.len() > 1 && target_file.is_none() && nth.is_none() {
      let file_names = self.files.iter().map(|(s, _)| String::from("  ") + s).collect::<Vec<_>>().join("\n");
      let message = format!("gist had more than one file, but no target file was specified\n\nfiles available:\n{}",
                            file_names);
      return Err(message.into());
    }
    let get_url = || {
      let nth = nth.unwrap_or(0);
      let file = files.iter().nth(nth);
      let whatever = some_or_err!(file, format!("file {} did not exist", nth).into());
      Ok(whatever.0)
    };
    let target_result: Result<&String> = match target_file {
      Some(file) => Ok(file),
      None => get_url(),
    };
    let target = try!(target_result).to_lowercase();
    if !files.contains_key(&target) {
      return Err("gist did not contain file".into());
    }
    let file = &some_or_err!(files.get(&target), "gist did not contain file".into());
    let option_raw_url = &file.raw_url;
    let raw_url = some_ref_or_err!(option_raw_url, "file had no raw_url".into());
    Ok(raw_url.to_owned())
  }
}

#[derive(RustcEncodable, RustcDecodable)]
struct GistFile {
  content: String,
  raw_url: Option<String>
}

impl From<String> for GistFile {
  fn from(string: String) -> Self {
    GistFile {
      content: string,
      raw_url: None
    }
  }
}

pub struct Gist;

impl Gist {
  pub fn new() -> Self {
    Gist {}
  }
}

unsafe impl Sync for Gist {}

impl Engine for Gist {
  fn get_name(&self) -> &str {
    "gist"
  }

  fn get_domain(&self) -> &str {
    "gist.github.com"
  }

  fn upload(&self, bins: &Bins, data: &[PasteFile]) -> Result<String> {
    let upload = GistUpload::from(bins, data);
    let j = try!(json::encode(&upload).map_err(|e| e.to_string()));
    let client = Client::new();
    let mut res = try!({
      let mut builder = client.post("https://api.github.com/gists")
        .body(&j)
        .header(ContentType::json())
        .header(UserAgent(String::from("bins")));
      if bins.arguments.auth {
        if let Some(username) = bins.config.lookup_str("gist.username") {
          if let Some(token) = bins.config.lookup_str("gist.access_token") {
            if !username.is_empty() && !token.is_empty() {
              builder = builder.header(Authorization(Basic {
                username: username.to_owned(),
                password: Some(token.to_owned())
              }));
            }
          }
        }
      }
      builder.send()
        .map_err(|e| e.to_string())
    });
    let mut s = String::from("");
    try!(res.read_to_string(&mut s).map_err(|e| e.to_string()));
    if res.status != StatusCode::Created {
      println!("{}", s);
      return Err("paste could not be created".into());
    }
    let raw_gist = try!(Json::from_str(&s).map_err(|e| e.to_string()));
    let gist = some_or_err!(raw_gist.as_object(),
                            "response was not a json object".into());
    let html_url = some_or_err!(gist.get("html_url"), "no html_url_key".into());
    let url = some_or_err!(html_url.as_string(), "html_url was not a string".into());
    Ok(url.to_owned())
  }

  fn get_raw(&self, bins: &Bins, url: &mut Url) -> Result<String> {
    let id = some_or_err!(some_or_err!(url.path_segments(), "could not get path of url".into()).last(),
                          "could not get last path of url".into());
    if bins.arguments.files.len() > 1 {
      return Err("currently, only one file is able to be retrieved in input mode".into());
    }
    let client = Client::new();
    let mut res = try!(client.get(&format!("https://api.github.com/gists/{}", id))
      .header(UserAgent(String::from("bins")))
      .send()
      .map_err(|e| e.to_string()));
    let mut s = String::from("");
    try!(res.read_to_string(&mut s).map_err(|e| e.to_string()));
    if res.status != StatusCode::Ok {
      println!("{}", s);
      return Err("status was not ok".into());
    }
    let gist_upload: GistUpload = try!(json::decode(&s));
    let raw_url = try!(gist_upload.get_url(bins, bins.arguments.nth));
    let download = IndexedDownload {
      url: raw_url,
      headers: Headers::new(),
      target: None
    };
    download.download()
  }
}
