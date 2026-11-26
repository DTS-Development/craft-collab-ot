//! 서식 텍스트 Delta OT (word 본문·deck 글 등 1차원 서식 글).
//!
//! 규격은 Quill Delta(`rich-text` OT 타입)의 compose·transform·invert·transformPosition 의미를
//! 따른다. 서버가 순서(rev)를 정하므로 TP2는 필요 없고, 변환은 항상 같은 선형 로그 위에서 한다.
//!
//! 단위(앱 오프셋과 변환 없이 맞추기 위한 약속):
//! - 글 길이 = UTF-8 바이트 수.
//! - embed 길이 = [`EMBED_LEN`] = 3 (U+FFFC의 UTF-8 길이). 앱 글 안에 개체 자리 문자(U+FFFC)가
//!   있으면 한 embed가 그 3바이트를 차지한다.
//!
//! 주의: 길이는 글자 경계에 맞아야 한다. 유효한 문서에 대한 유효한 delta는 변환·합성 중
//! 글자 안쪽을 자르지 않는다. 외부(원격)에서 온 delta는 [`Delta::check_on`]으로 먼저 검사한다.

mod delta;
mod diff;
#[cfg(test)]
mod fuzz;

pub use delta::{Attrs, Delta, Insert, Op, EMBED_LEN};
