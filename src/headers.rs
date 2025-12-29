use core::fmt;
use std::collections::HashMap;

use anyhow::{bail, Result};

#[derive(Debug, Clone)]
pub struct Headers(pub HashMap<String, String>);

impl Headers {
    pub fn new() -> Self {
        Self(HashMap::new())
    }

    pub fn insert(&mut self, k: &str, v: &str) -> Option<String> {
        self.0.insert(k.to_string(), v.to_string())
    }

    pub fn get(&self, k: &str) -> Option<&String> {
        self.0.get(k.to_lowercase().as_str())
    }

    #[cfg(test)]
    pub fn len(&self) -> usize {
        self.0.len()
    }

    pub fn parse_headers(data: &[u8]) -> Result<(Option<(String, String)>, usize)> {

        // now convert headers to a string
        let header_str = String::from_utf8_lossy(data).to_string();
        // split that on '\r\n'
        if let Some((raw_header, _rest)) = header_str.split_once("\r\n") {
            let p = raw_header.len() + 2;
            let raw_header = raw_header.trim();
            if raw_header.is_empty() {
                return Ok((None, p));
            }
            if let Some((field_name, field_value)) = raw_header.split_once(':') {
                if field_name.trim_end() != field_name {
                    bail!("Field name included invalid whitespace: '{}'", raw_header);
                }
                if !Self::valid_field_name(field_name) {
                    bail!("Invalid characters detected: '{}'", field_name);
                }
                Ok((Some((field_name.trim().to_lowercase(), field_value.trim().to_string())), p))
            } else {
                bail!("No ':' found in raw_header: {}", raw_header);
            }
        } else {
            Ok((None, 0))
        }
    }

    fn valid_field_name(s: &str) -> bool {
        if s.is_empty() {
            return false;
        }
        // checks for only valid characters
        let upper_chars = 'A'..='Z';
        let lower_chars = 'a'..='z';
        let digits = '0'..='9';
        let special_chars = ['!', '#', '$', '%', '&', '\'', '*', '+', '-', '.', '^', '_', '`', '|', '~'];
        let valid_chars: Vec<char> = upper_chars.chain(lower_chars).chain(digits).chain(special_chars).collect();
        
        s.chars().all(|c| valid_chars.contains(&c))
    }
}

impl fmt::Display for Headers {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f,"{}",
            self.0.iter().map(|(k,v)| format!("{}: {}\r\n", k, v)).collect::<String>()
        )
    }
}