//! Cryptographic primitives for OFD: SM3 digest, SM4 block cipher, SM2
//! signatures, X.509 certificate inspection and GB/T 20520 timestamps.
//! PKCS#12 is follow-up work.

#![forbid(unsafe_code)]
#![warn(missing_docs)]

pub mod asymmetric;
pub mod cert;
pub mod der;
pub mod digest;
pub mod symmetric;
pub mod timestamp;
pub mod trust;

pub use asymmetric::{Sm2KeyPair, sm2_verify, sm2_verify_digest};
pub use cert::{
    Certificate, KeyUsage, signed_value_content, signed_value_covers, signed_value_digest,
    split_signed_value,
};
pub use digest::{SM3_OID, Sm3Hash, sm3};
pub use symmetric::{Sm4Key, Sm4Mode};
pub use timestamp::TimeStampToken;
pub use trust::{MAX_CHAIN_DEPTH, PathError, TrustAnchors, validate_path};
