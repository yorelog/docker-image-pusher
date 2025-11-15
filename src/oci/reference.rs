use std::fmt;
use std::str::FromStr;

use crate::PusherError;

#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct Reference {
    pub registry: String,
    pub repository: String,
    pub tag: Option<String>,
    pub digest: Option<String>,
}

impl Reference {
    pub fn parse(input: &str) -> Result<Self, PusherError> {
        let remainder = input.trim();
        if remainder.is_empty() {
            return Err(PusherError::PullError("Empty image reference".to_string()));
        }

        let (registry, rest) = if remainder.contains('/')
            && remainder
                .split('/')
                .next()
                .map(|seg| seg.contains('.') || seg.contains(':') || seg == "localhost")
                .unwrap_or(false)
        {
            let mut parts = remainder.splitn(2, '/');
            (
                parts.next().unwrap().to_string(),
                parts.next().unwrap_or_default().to_string(),
            )
        } else {
            ("registry-1.docker.io".to_string(), remainder.to_string())
        };

        let mut repository = rest;
        let mut tag = None;
        let mut digest = None;

        if let Some(idx) = repository.rfind('@') {
            digest = Some(repository[idx + 1..].to_string());
            repository = repository[..idx].to_string();
        }

        if let Some(idx) = repository.rfind(':') {
            if repository[idx + 1..]
                .chars()
                .all(|c| c.is_ascii_alphanumeric() || c == '_' || c == '-' || c == '.')
            {
                tag = Some(repository[idx + 1..].to_string());
                repository = repository[..idx].to_string();
            }
        }

        if repository.is_empty() {
            return Err(PusherError::PullError(
                "Repository missing from reference".to_string(),
            ));
        }

        if registry == "registry-1.docker.io" && !repository.contains('/') {
            repository = format!("library/{}", repository);
        }

        Ok(Self {
            registry,
            repository,
            tag,
            digest,
        })
    }

    pub fn registry_host(&self) -> &str {
        &self.registry
    }
}

impl FromStr for Reference {
    type Err = PusherError;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        Self::parse(s)
    }
}

impl fmt::Display for Reference {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let mut out = format!("{}/{}", self.registry, self.repository);
        if let Some(tag) = &self.tag {
            out.push(':');
            out.push_str(tag);
        }
        if let Some(digest) = &self.digest {
            out.push('@');
            out.push_str(digest);
        }
        write!(f, "{}", out)
    }
}
