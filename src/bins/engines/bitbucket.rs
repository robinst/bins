use bins::{Bins, PasteFile};
use bins::configuration::BetterLookups;
use bins::engines::Engine;
use bins::engines::indexed::{IndexedDownload, DownloadsFile};
use bins::error::*;
use hyper::Url;
use hyper::client::Client;
use hyper::header::{Authorization, Basic, ContentType, Headers, UserAgent};
use hyper::mime::{Attr, Mime, TopLevel, SubLevel, Value};
use hyper::status::StatusCode;
use rand::{thread_rng, Rng};
use rustc_serialize::json::{self, Json, Object};
use rustc_serialize::base64::{MIME, ToBase64};
use std::io::Read;

pub struct Bitbucket;

impl Bitbucket {
  pub fn new() -> Self {
    Bitbucket {}
  }
}

unsafe impl Sync for Bitbucket {}

impl Engine for Bitbucket {
  fn get_name(&self) -> &str {
    "bitbucket"
  }

  fn get_domain(&self) -> &str {
    "bitbucket.org"
  }

  fn upload(&self, bins: &Bins, data: &[PasteFile]) -> Result<String> {
    let authorization = try!(authorization(bins));

    let boundary = random_boundary();
    let headers = prepare_headers(&boundary, authorization);
    let body = try!(prepare_body(bins, data, &boundary));

    let client = Client::new();
    let builder = client.post("https://api.bitbucket.org/2.0/snippets")
      .headers(headers)
      .body(&body);

    let mut response = try!(builder.send().map_err(|e| e.to_string()));
    let mut response_body = String::new();
    try!(response.read_to_string(&mut response_body).map_err(|e| e.to_string()));
    if response.status != StatusCode::Created {
      let msg = format!("snippet could not be created, response: {}\n{}",
                        response.status,
                        response_body);
      return Err(msg.into());
    }

    let snippet = try!(Json::from_str(&response_body).map_err(|e| e.to_string()));
    let url = some_or_err!(snippet.find_path(&["links", "html", "href"])
                             .and_then(|j| j.as_string()),
                           "string links.html.href not found in response".into());
    Ok(url.to_string())
  }

  fn get_raw(&self, bins: &Bins, url: &mut Url) -> Result<String> {
    let segments: Vec<_> = some_or_err!(url.path_segments(), "url has no path".into()).collect();
    if segments.len() != 3 || segments[0] != "snippets" {
      return Err("url path expected to be of form /snippets/{username}/{id}".into());
    }
    let username = segments[1];
    let id = segments[2];
    if bins.arguments.files.len() > 1 {
      return Err("currently, only one file is able to be retrieved in input mode".into());
    }
    let authorization = try!(authorization(bins));
    let mut headers = Headers::new();
    headers.set(UserAgent("bins".to_string()));
    headers.set(authorization);

    let client = Client::new();
    let mut response = try!(client.get(&format!("https://api.bitbucket.org/2.0/snippets/{}/{}", username, id))
      .headers(headers.clone())
      .send()
      .map_err(|e| e.to_string()));
    let mut response_body = String::from("");
    try!(response.read_to_string(&mut response_body).map_err(|e| e.to_string()));
    if response.status != StatusCode::Ok {
      let msg = format!("snippet could not be read, response: {}\n{}",
                        response.status,
                        response_body);
      return Err(msg.into());
    }

    let snippet = try!(Json::from_str(&response_body).map_err(|e| e.to_string()));
    let files = some_or_err!(snippet.find("files").and_then(|json| json.as_object()),
                             "object files not found in response".into());
    let file_url = try!(get_file_url(&files, bins));
    let download = IndexedDownload {
      url: file_url.to_string(),
      headers: headers.clone(),
      target: None
    };
    download.download()
  }
}

fn config_value<'a>(bins: &'a Bins, key: &'a str) -> Result<&'a str> {
  let value = some_or_err!(bins.config.lookup_str(key),
                           format!("no {} set in configuration", key).into());
  if value.is_empty() {
    return Err(format!("{} in configuration was empty", key).into());
  }
  return Ok(value);
}

fn random_boundary() -> String {
  thread_rng().gen_ascii_chars().take(69).collect()
}

fn authorization(bins: &Bins) -> Result<Authorization<Basic>> {
  let username = try!(config_value(bins, "bitbucket.username"));
  let app_password = try!(config_value(bins, "bitbucket.app_password"));
  Ok(Authorization(Basic {
    username: username.to_string(),
    password: Some(app_password.to_string())
  }))
}

fn prepare_headers(boundary: &str, authorization: Authorization<Basic>) -> Headers {
  let mut headers = Headers::new();
  let content_type = ContentType(Mime(TopLevel::Multipart,
                                      SubLevel::Ext("related".to_string()),
                                      vec![(Attr::Boundary, Value::Ext(boundary.to_string()))]));
  headers.set(content_type);
  headers.set_raw("MIME-Version", vec![b"1.0".to_vec()]);
  headers.set(UserAgent("bins".to_string()));
  headers.set(authorization);

  headers
}

fn prepare_body(bins: &Bins, data: &[PasteFile], boundary: &str) -> Result<String> {
  let properties = SnippetProperties {
    title: "bins".to_string(),
    is_private: bins.arguments.private
  };
  let properties_json = try!(json::encode(&properties).map_err(|e| e.to_string()));

  let mut body = MultipartRelatedBody::new(&boundary);
  body.add_json(&properties_json);
  for file in data {
    body.add_file(&file.name, file.data.as_bytes());
  }

  Ok(body.end())
}

fn get_file_url<'a>(files: &'a Object, bins: &Bins) -> Result<&'a str> {
  let target_file = bins.arguments.files.get(0);
  let nth = bins.arguments.nth;

  let keys = files.keys().cloned().map(|s| s.to_lowercase()).collect::<Vec<_>>();
  if keys.len() < 1 {
    return Err("snippet had no files".into());
  }
  if keys.len() > 1 && target_file.is_none() && nth.is_none() {
    let file_names = keys.iter().map(|s| String::from(" ") + s).collect::<Vec<_>>().join("\n");
    let message = format!("snippet had more than one file, but no target file was specified\n\nfiles available:\n{}",
                          file_names);
    return Err(message.into());
  }
  let target_result: Result<&String> = match target_file {
    Some(file) => Ok(file),
    None => {
      let nth = nth.unwrap_or(0);
      let file = files.iter().nth(nth).map(|j| j.0);
      file.ok_or(format!("file {} did not exist", nth).into())
    }
  };
  let target = try!(target_result).to_lowercase();
  if !keys.contains(&target) {
    return Err(format!("snippet did not contain file {}", target).into());
  }
  let file = some_or_err!(files.get(&target),
                          format!("object files.{} not found in response", target).into());
  let file_url = some_or_err!(file.find_path(&["links", "self", "href"])
                                .and_then(|j| j.as_string()),
                              "string links.self.href not found in response".into());
  Ok(file_url)
}

#[derive(RustcEncodable)]
struct SnippetProperties {
  title: String,
  is_private: bool
}

struct MultipartRelatedBody<'a> {
  boundary: &'a str,
  content: String
}

impl<'a> MultipartRelatedBody<'a> {
  fn new(boundary: &str) -> MultipartRelatedBody {
    MultipartRelatedBody {
      boundary: boundary,
      content: String::new()
    }
  }

  fn add_json(&mut self, json: &str) {
    self.add_boundary();
    self.add_line("Content-Type: application/json; charset=\"utf-8\"");
    self.add_line("MIME-Version: 1.0");
    self.add_line("Content-ID: snippet");
    self.end_line();

    self.add_line(json);
    self.end_line();
  }

  fn add_file(&mut self, filename: &str, content: &[u8]) {
    self.add_boundary();
    self.add_line("Content-Type: text/plain; charset=\"utf-8\"");
    self.add_line("MIME-Version: 1.0");
    self.add_line("Content-Transfer-Encoding: base64");

    self.add("Content-ID: \"");
    self.add(filename);
    self.add("\"");
    self.end_line();

    self.add("Content-Disposition: attachment; filename=\"");
    self.add(filename);
    self.add("\"");
    self.end_line();

    self.end_line();

    self.content.push_str(&content.to_base64(MIME));
    self.end_line();
  }

  fn end(mut self) -> String {
    self.content.push_str("--");
    self.content.push_str(self.boundary);
    self.content.push_str("--");
    self.end_line();
    self.content
  }

  fn add_boundary(&mut self) {
    self.content.push_str("--");
    self.content.push_str(self.boundary);
    self.end_line();
  }

  fn add(&mut self, s: &str) {
    self.content.push_str(s);
  }

  fn add_line(&mut self, line: &str) {
    self.content.push_str(line);
    self.end_line();
  }

  fn end_line(&mut self) {
    self.content.push_str("\r\n");
  }
}
