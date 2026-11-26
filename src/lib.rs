//! craft-collab-ot: word·deck·grid·pdf 포크가 같이 쓰는 동시 편집 변환 코드.
//! 서버는 변환하지 않는다(서버 순서 OT) — 변환은 클라이언트(Rust)에서 이 크레이트로만 한다.
//!
//! - [`client`]: 편집기용 클라이언트 상태기계(`SyncClient`)
//! - [`core`]: 공통 계약(`Transform`·`Doc`·`Invert`·`transform_seq`)
//! - [`reg`]: 키 -> 값 레지스터(마지막 쓰기가 이김)
//! - [`seq`]: 원소 목록 삽입·삭제·이동
//! - [`cmd`]: 명령 재실행(앱이 변환 규칙을 정한다)
//! - [`text`]: 서식 텍스트 Delta OT
//! - [`sim`]: 서버 순서 OT 수렴 퍼즈 하네스
pub mod client;
pub mod cmd;
pub mod core;
pub mod reg;
pub mod seq;
pub mod sim;
pub mod text;
#[cfg(test)]
mod tests;
