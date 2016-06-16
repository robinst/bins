#[macro_use]
pub mod macros;
pub mod arguments;
pub mod configuration;
pub mod engines;
pub mod error;
pub mod network;

extern crate std;
extern crate toml;

use bins::arguments::Arguments;
use bins::engines::*;
use bins::error::*;
use hyper::Url;
use std::collections::HashMap;
use std::fs::File;
use std::io::prelude::*;
use std::ops::Range;
use std::path::Path;
use toml::Value;

#[derive(Clone)]
pub struct PasteFile {
  pub name: String,
  pub data: String
}

impl PasteFile {
  fn new(name: String, data: String) -> Self {
    PasteFile {
      name: name,
      data: data
    }
  }
}

pub struct Bins {
  pub config: Value,
  pub arguments: Arguments
}

impl Bins {
  pub fn new(config: Value, arguments: Arguments) -> Self {
    Bins {
      config: config,
      arguments: arguments
    }
  }

  pub fn get_engine(&self) -> Result<&Box<Bin>> {
    let service = match self.arguments.service {
      Some(ref s) => s,
      None => return Err("no service was specified and no default service was set.".into()),
    };
    match engines::get_bin_by_name(service) {
      Some(engine) => Ok(engine),
      None => return Err(format!("unknown service \"{}\"", service).into()),
    }
  }

  fn read_file<P: AsRef<Path>>(&self, p: P) -> Result<String> {
    let path = p.as_ref();
    let name = some_or_err!(path.to_str(), "file name was not valid unicode".into());
    if !path.exists() {
      return Err(format!("{} does not exist", name).into());
    }
    if !path.is_file() {
      return Err(format!("{} is not a file", name).into());
    }
    let mut file = match File::open(path) {
      Ok(f) => f,
      Err(e) => return Err(format!("could not open {}: {}", name, e).into()),
    };
    let mut s = String::new();
    if let Err(e) = file.read_to_string(&mut s) {
      return Err(format!("could not read {}: {}", name, e).into());
    }
    Ok(s)
  }

  fn read_file_to_pastefile<P: AsRef<Path>>(&self, p: P) -> Result<PasteFile> {
    let path = p.as_ref();
    match self.read_file(path) {
      Ok(s) => {
        let n = some_or_err!(path.file_name(), "not a valid file name".into());
        Ok(PasteFile::new(n.to_string_lossy().into_owned(), s))
      }
      Err(s) => Err(s),
    }
  }

  pub fn get_to_paste(&self) -> Result<Vec<PasteFile>> {
    let arguments = &self.arguments;
    let message = &arguments.message;
    let paste_files: Vec<PasteFile> = if message.is_some() {
      vec![PasteFile::new(String::from("message"), message.clone().unwrap())]
    } else if !arguments.files.is_empty() {
      let files = arguments.files.clone();
      let results = files.iter()
        .map(|s| Path::new(s))
        .map(|p| self.read_file_to_pastefile(p))
        .map(|r| r.map_err(|e| e.iter().map(|x| x.to_string()).collect::<Vec<_>>().join("\n")))
        .collect::<Vec<_>>();
      for res in results.iter().cloned() {
        if res.is_err() {
          return Err(res.err().unwrap().into());
        }
      }
      let mut pastes =
        results.iter().cloned().map(|r| r.unwrap()).filter(|p| !p.data.trim().is_empty()).collect::<Vec<_>>();
      self.handle_duplicate_file_names(&mut pastes);
      pastes
    } else {
      let mut buffer = String::new();
      if let Err(e) = std::io::stdin().read_to_string(&mut buffer) {
        return Err(format!("error reading stdin: {}", e).into());
      }
      vec![PasteFile::new(String::from("stdin"), buffer)]
    };
    if paste_files.iter().filter(|p| !p.data.trim().is_empty()).count() < 1 {
      return Err("no files (or all empty files) to paste".into());
    }
    Ok(paste_files)
  }

  fn handle_duplicate_file_names(&self, pastes: &mut Vec<PasteFile>) {
    let mut names_map: HashMap<String, i32> = HashMap::new();
    for mut paste in pastes {
      let name = paste.name.clone();
      if names_map.contains_key(&name) {
        let parts = name.rsplit('.');
        let (beginning, end) = if parts.clone().count() > 1 {
          let mut beginning_parts = parts.clone().skip(1).collect::<Vec<_>>();
          beginning_parts.reverse();
          let beginning = beginning_parts.join(".");
          let end = parts.take(1).next().map_or(String::new(), |s| String::from(".") + s);
          (beginning, end)
        } else {
          (name.clone(), String::from(""))
        };
        let number = names_map.entry(name.clone()).or_insert(1);
        paste.name = format!("{}_{}{}", beginning, number, end);
        *number += 1;
      }
      names_map.entry(name.clone()).or_insert(1);
    }
  }

  fn get_engine_for_url<'a>(&'a self, url: &'a Url) -> Result<&Box<Bin>> {
    let domain = some_or_err!(url.domain(), "input url had no domain".into());
    let engine = some_or_err!(engines::get_bin_by_domain(domain),
                              format!("could not find a bin for domain {}", domain).into());
    Ok(engine)
  }

  fn get_raw(&self, url_string: &str) -> Result<String> {
    let mut url = try!(network::parse_url(url_string.as_ref()));
    let url_clone = url.clone();
    let bin = try!(self.get_engine_for_url(&url_clone));
    let files = try!(bin.produce_raw_contents(self, &mut url));
    let file = some_or_err!(files.get(0), "".into()); // FIXME: use all files
    Ok(file.data.clone())
  }

  pub fn get_output(&self) -> Result<String> {
    if let Some(ref input) = self.arguments.input {
      return self.get_raw(input);
    }
    let to_paste = try!(self.get_to_paste());
    let engine = try!(self.get_engine());
    let upload_url = if to_paste.len() > 1 {
      try!(engine.upload_all(self, to_paste))
    } else if to_paste.len() == 1 {
      let file = &to_paste[0];
      try!(engine.upload(self, file.clone())) // FIXME: shouldn't have to use clone
    } else {
      return Err("no files to upload".into());
    };
    Ok(upload_url.as_str().to_owned())
  }
}

pub struct FlexibleRange {
  pub ranges: Vec<Range<usize>>
}

impl FlexibleRange {
  pub fn parse<S: Into<String>>(string: S) -> Result<FlexibleRange> {
    let string = string.into();
    let parts = string.split(',').map(|s| s.trim());
    let mut range: Vec<Range<usize>> = Vec::new();
    for part in parts {
      let bounds = part.split('-').map(|s| s.trim()).collect::<Vec<_>>();
      match bounds.len() {
        1 => {
          let num = bounds[0];
          if num.is_empty() {
            return Err("empty part in range".into());
          }
          let num = try!(num.parse::<usize>());
          range.push(num..num + 1);
        }
        2 => {
          let lower = bounds[0];
          let upper = bounds[1];
          if lower.is_empty() || upper.is_empty() {
            return Err("incomplete part in range".into());
          }
          let lower: usize = try!(lower.parse());
          let upper: usize = try!(upper.parse());
          let r: Range<usize> = if lower > upper {
            lower + 1..upper
          } else {
            lower..upper + 1
          };
          range.push(r);
        }
        _ => return Err("too many dashes in range".into())
      }
    }
    Ok(FlexibleRange { ranges: range })
  }
}

impl Iterator for FlexibleRange {
  type Item = usize;

  fn next(&mut self) -> Option<Self::Item> {
    if self.ranges.len() < 1 {
      return None;
    }
    let n = {
      let mut range = &mut self.ranges[0];
      if range.start > range.end {
        let normalized = range.end..range.start;
        let ret = normalized.rev().next();
        if ret.is_some() {
          range.start -= 1;
        }
        ret
      } else {
        range.next()
      }
    };
    if n.is_none() {
      self.ranges.remove(0);
      return self.next();
    }
    n
  }
}
