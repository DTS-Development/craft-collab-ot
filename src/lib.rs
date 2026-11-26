#![allow(clippy::len_without_is_empty)]
//! craft-collab-ot: word·deck·grid·pdf 포크가 같이 쓰는 동시 편집 변환 코드.
//! 서버는 변환하지 않는다(서버 순서 OT) — 변환은 클라이언트(Rust)에서 이 크레이트로만 한다.
pub mod text;
