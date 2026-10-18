use std::fmt;
use std::num::ParseIntError;
use std::str::{self, FromStr, Utf8Error};

use lazy_static::lazy_static;
use regex::Regex;
use thiserror::Error;

#[derive(Clone, Debug, Hash, PartialOrd, Ord, PartialEq, Eq)]
pub struct Shadow {
    content_hash: ContentSha256,
    size: Option<u64>,
}

impl Shadow {
    pub fn new(content_hash: ContentSha256, size: Option<u64>) -> Self {
        Self { content_hash, size }
    }

    pub fn content_hash(&self) -> &ContentSha256 {
        &self.content_hash
    }

    pub fn size(&self) -> Option<u64> {
        self.size
    }

    pub fn to_bytes(&self) -> Vec<u8> {
        self.to_string().as_bytes().to_vec()
    }

    pub fn from_bytes(shadow_content: &[u8]) -> Result<Self, ShadowError> {
        let s = str::from_utf8(shadow_content).map_err(ShadowError::Utf8Error)?;
        s.parse()
    }
}

impl fmt::Display for Shadow {
    fn fmt(&self, fmt: &mut fmt::Formatter) -> fmt::Result {
        writeln!(fmt, "sha256 {}", self.content_hash)?;
        if let Some(size) = self.size {
            writeln!(fmt, "size {}", size)?;
        }
        Ok(())
    }
}

impl FromStr for Shadow {
    type Err = ShadowError;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        lazy_static! {
            static ref RE: Regex =
                Regex::new(r"^sha256 (?P<sha256>[a-z0-9]{64})\n(size (?P<size>[0-9]+)\n)?$")
                    .unwrap();
        }
        let caps = RE.captures(s).ok_or(Self::Err::MalformedShadow)?;

        let content_hash = caps["sha256"].parse()?;
        let size = caps
            .name("size")
            .map(|m| m.as_str().parse())
            .transpose()
            .map_err(Self::Err::MalformedShadowSize)?;

        Ok(Self { content_hash, size })
    }
}

#[derive(Clone, Debug, Hash, PartialOrd, Ord, PartialEq, Eq)]
pub struct ContentSha256 {
    digest: [u8; Self::SHA256_DIGEST_SIZE],
}

impl ContentSha256 {
    const SHA256_DIGEST_SIZE: usize = 32;

    pub fn new(digest: [u8; Self::SHA256_DIGEST_SIZE]) -> Self {
        Self { digest }
    }

    // precondition: digest.len() == Self::SHA256_DIGEST_SIZE
    pub fn from_slice(digest: &[u8]) -> Self {
        assert_eq!(digest.len(), Self::SHA256_DIGEST_SIZE);
        let mut arr = [0; Self::SHA256_DIGEST_SIZE];
        arr.copy_from_slice(digest);
        Self::new(arr)
    }

    pub fn to_hex(&self) -> String {
        self.to_string()
    }

    pub fn from_hex(s: &str) -> Result<Self, ShadowError> {
        Self::from_str(s)
    }
}

impl fmt::Display for ContentSha256 {
    fn fmt(&self, fmt: &mut fmt::Formatter) -> fmt::Result {
        write!(fmt, "{}", hex::encode(self.digest))
    }
}

impl FromStr for ContentSha256 {
    type Err = ShadowError;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        let mut digest = [0; Self::SHA256_DIGEST_SIZE];
        hex::decode_to_slice(s, &mut digest).map_err(ShadowError::MalformedShadowContentHashHex)?;
        Ok(Self::new(digest))
    }
}

#[derive(Error, Debug)]
pub enum ShadowError {
    #[error("malformed")]
    MalformedShadow,
    #[error("error converting from utf-8: {0}")]
    Utf8Error(
        #[source]
        #[from]
        Utf8Error,
    ),
    #[error("malformed content hash hex: {0}")]
    MalformedShadowContentHashHex(#[source] hex::FromHexError),
    #[error("malformed size")]
    MalformedShadowSize(#[source] ParseIntError),
}

#[cfg(test)]
mod tests {
    use super::*;

    fn ensure_err<T: FromStr>(s: &str) {
        assert!(T::from_str(s).is_err());
    }

    fn ensure_inverse<T: FromStr + ToString>(s: &str)
    where
        <T as FromStr>::Err: fmt::Debug,
    {
        assert_eq!(T::from_str(s).unwrap().to_string(), s);
    }

    const TEST_HEX_DIGEST: &str =
        "da60ed9cad3849231c91f0419c8eb59d10d0ccf3fdfa7341fa6f657b684ba1cf";

    #[test]
    fn shadow_content_sha256() {
        ensure_err::<ContentSha256>("");
        ensure_err::<ContentSha256>(&format!(" {}", TEST_HEX_DIGEST));
        ensure_err::<ContentSha256>(&format!("{}0", TEST_HEX_DIGEST));
        ensure_inverse::<ContentSha256>(TEST_HEX_DIGEST);
    }

    #[test]
    fn shadow() {
        ensure_err::<Shadow>("");
        ensure_err::<Shadow>(&format!("sha256 {}\nsize 123", TEST_HEX_DIGEST));
        ensure_err::<Shadow>(&format!("sha256 {}\nsize \n", TEST_HEX_DIGEST));
        ensure_err::<Shadow>(&format!("sha256 {}\r\nsize 123\r\n", TEST_HEX_DIGEST));
        ensure_inverse::<Shadow>(&format!("sha256 {}\nsize 123\n", TEST_HEX_DIGEST));
        ensure_inverse::<Shadow>(&format!("sha256 {}\n", TEST_HEX_DIGEST));
    }
}
