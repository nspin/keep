use std::fmt;
use std::path::{Path, PathBuf};
use std::str::{self, FromStr};

use itertools::Itertools;
use thiserror::Error;

const MARKER: &str = "0";
const CHILD_PREFIX: &str = "0_";

#[derive(Clone, Debug, Hash, PartialOrd, Ord, PartialEq, Eq)]
pub struct ShadowPathComponent(
    // invariants: matches [^/\0]+ and not in {".", ".."}
    String,
);

impl ShadowPathComponent {
    const DISALLOWED_CHARS: &'static [char] = &['/', '\0'];

    pub fn new(s: String) -> Result<Self, ShadowPathError> {
        Self::check(s.as_str())?;
        Ok(Self(s))
    }

    fn check(s: &str) -> Result<(), ShadowPathError> {
        match s {
            "." | ".." => Err(ShadowPathError::DisallowedComponent),
            _ if s.contains(Self::DISALLOWED_CHARS) => Err(ShadowPathError::DisallowedChar),
            _ if s.is_empty() => Err(ShadowPathError::Empty),
            _ => Ok(()),
        }
    }

    pub fn encode(&self) -> String {
        format!("{}{}", CHILD_PREFIX, self)
    }
}

impl AsRef<str> for ShadowPathComponent {
    fn as_ref(&self) -> &str {
        &self.0
    }
}

impl fmt::Display for ShadowPathComponent {
    fn fmt(&self, fmt: &mut fmt::Formatter) -> fmt::Result {
        write!(fmt, "{}", self.0)
    }
}

impl FromStr for ShadowPathComponent {
    type Err = ShadowPathError;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        Self::check(s)?;
        Self::new(s.to_owned())
    }
}

#[derive(Clone, Debug, Hash, PartialOrd, Ord, PartialEq, Eq)]
pub struct ShadowPath(Vec<ShadowPathComponent>);

impl ShadowPath {
    pub fn new() -> Self {
        Self(Vec::new())
    }

    pub fn components(&self) -> &[ShadowPathComponent] {
        &self.0
    }

    pub fn push(&mut self, component: ShadowPathComponent) {
        self.0.push(component)
    }

    pub fn pop(&mut self) -> Option<ShadowPathComponent> {
        self.0.pop()
    }

    pub fn append_to(&self, path: impl AsRef<Path>) -> PathBuf {
        let mut path = path.as_ref().to_owned();
        for c in self.components() {
            path.push(&c.0);
        }
        path
    }

    pub fn encode(&self) -> String {
        self.components()
            .iter()
            .map(ShadowPathComponent::encode)
            .join("/")
    }

    pub fn encode_marker(&self) -> String {
        self.components()
            .iter()
            .map(ShadowPathComponent::encode)
            .chain([ShadowTreeEntryName::Marker.encode()])
            .join("/")
    }
}

impl Default for ShadowPath {
    fn default() -> Self {
        Self::new()
    }
}

impl fmt::Display for ShadowPath {
    fn fmt(&self, fmt: &mut fmt::Formatter) -> fmt::Result {
        write!(fmt, "{}", self.components().iter().format("/"))
    }
}

impl FromStr for ShadowPath {
    type Err = ShadowPathError;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        Ok(Self(if s.is_empty() {
            vec![]
        } else {
            s.split('/')
                .map(ShadowPathComponent::from_str)
                .collect::<Result<Vec<ShadowPathComponent>, Self::Err>>()?
        }))
    }
}

#[derive(Clone, Debug, PartialOrd, Ord, PartialEq, Eq)]
pub enum ShadowTreeEntryName {
    Marker,
    Child(ShadowPathComponent),
}

impl ShadowTreeEntryName {
    pub fn is_marker(&self) -> bool {
        matches!(self, Self::Marker)
    }

    pub fn child(&self) -> Option<&ShadowPathComponent> {
        match self {
            Self::Child(child) => Some(child),
            _ => None,
        }
    }

    pub fn encode(&self) -> String {
        match self {
            Self::Marker => MARKER.to_string(),
            Self::Child(child) => child.encode(),
        }
    }

    pub fn decode(s: &str) -> Result<Self, ShadowEncodedPathError> {
        Ok(if s == MARKER {
            Self::Marker
        } else {
            match s.strip_prefix(CHILD_PREFIX) {
                Some(child) => Self::Child(child.parse()?),
                None => return Err(ShadowEncodedPathError::MissingPrefix),
            }
        })
    }
}

#[derive(Error, Debug)]
pub enum ShadowPathError {
    #[error("disallowed component")]
    DisallowedComponent,
    #[error("disallowed character")]
    DisallowedChar,
    #[error("empty")]
    Empty,
}

#[derive(Error, Debug)]
pub enum ShadowEncodedPathError {
    #[error("missing prefix")]
    MissingPrefix,
    #[error("malformed component")]
    ShadowPathError(
        #[source]
        #[from]
        ShadowPathError,
    ),
}

#[cfg(test)]
mod tests {
    use super::*;

    fn ensure_err<T: FromStr>(s: &'static str) {
        assert!(T::from_str(s).is_err());
    }

    fn ensure_inverse<T: FromStr + ToString>(s: &'static str)
    where
        <T as FromStr>::Err: fmt::Debug,
    {
        assert_eq!(T::from_str(s).unwrap().to_string(), s);
    }

    #[test]
    fn component() {
        ensure_err::<ShadowPathComponent>(".");
        ensure_err::<ShadowPathComponent>("..");
        ensure_err::<ShadowPathComponent>("");
        ensure_err::<ShadowPathComponent>("x/y");
        ensure_err::<ShadowPathComponent>("x\0y");
        ensure_inverse::<ShadowPathComponent>("abc");
    }

    #[test]
    fn path() {
        ensure_err::<ShadowPath>("/x/y");
        ensure_err::<ShadowPath>("x/y/");
        ensure_err::<ShadowPath>("x//y"); // TODO support
        ensure_inverse::<ShadowPath>("");
        ensure_inverse::<ShadowPath>("abc");
        ensure_inverse::<ShadowPath>("x/y");
    }

    #[test]
    fn decode() {
        assert!(ShadowTreeEntryName::decode("xy").is_err());
        matches!(
            ShadowTreeEntryName::decode("0").unwrap(),
            ShadowTreeEntryName::Marker
        );
        assert_eq!(
            ShadowTreeEntryName::decode("0_x")
                .unwrap()
                .child()
                .unwrap()
                .to_string(),
            "x"
        );
    }
}
