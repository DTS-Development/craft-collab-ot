//! 모든 op 종류가 따르는 공통 계약. 서버는 op를 해석하지 않고 순서(rev)만 매긴다(서버 순서 OT).
//! 그래서 필요한 성질은 TP1(두 동시 op를 어느 순서로 적용해도 같은 결과) 하나뿐이다.
use serde_json::Value;
use std::fmt::Debug;

/// 변환·적용 오류. 정상 흐름에서는 나오지 않으며, 나오면 상태가 어긋난 것이다(호출한 쪽이 재동기화한다).
#[derive(Clone, Debug, PartialEq)]
pub enum OtError {
    /// 적용 위치가 문서 밖이거나 대상 id가 맞지 않는다.
    Invalid(String),
}

impl std::fmt::Display for OtError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            OtError::Invalid(s) => write!(f, "잘못된 op: {s}"),
        }
    }
}
impl std::error::Error for OtError {}

/// 같은 상태에서 갈라진 동시 op 둘을 맞춘다.
///
/// `a.transform(b, a_first)`는 "b가 이미 적용된 상태"에서 쓸 수 있게 바꾼 a를 돌려준다
/// (결과는 0개 이상: 사라질 수도, 쪼개질 수도 있다).
/// `a_first`는 서버 순서에서 a가 b보다 앞서는가. 같은 자리 삽입의 앞뒤, 같은 키의 승자를 가른다.
///
/// TP1: a가 b보다 앞서면, `apply(apply(s, a), b.transform(a, false)) == apply(apply(s, b), a.transform(b, true))`.
pub trait Transform: Sized + Clone + Debug {
    fn transform(&self, other: &Self, a_first: bool) -> Vec<Self>;
}

/// op 열 `a`와 `b`(둘 다 같은 상태에서 시작하는 순차 묶음)를 서로 맞춘다.
/// 돌려주는 `(a2, b2)`: `b` 뒤에 `a2`를 적용한 것과 `a` 뒤에 `b2`를 적용한 것이 같은 상태가 된다.
/// `a_first`는 묶음 `a`가 `b`보다 서버 순서에서 앞서는가. op가 쪼개지거나 사라져도 같은 규칙이다.
pub fn transform_seq<T: Transform>(a: &[T], b: &[T], a_first: bool) -> (Vec<T>, Vec<T>) {
    if a.is_empty() || b.is_empty() {
        return (a.to_vec(), b.to_vec());
    }
    if a.len() == 1 && b.len() == 1 {
        return (a[0].transform(&b[0], a_first), b[0].transform(&a[0], !a_first));
    }
    if a.len() > 1 {
        // a의 첫 op를 b 전체와 맞추고, 나머지는 그 결과 b와 맞춘다
        let (a0, rest) = a.split_at(1);
        let (a0t, bt) = transform_seq(a0, b, a_first);
        let (rest_t, bt2) = transform_seq(rest, &bt, a_first);
        let mut out = a0t;
        out.extend(rest_t);
        return (out, bt2);
    }
    let (b0, rest) = b.split_at(1);
    let (at, b0t) = transform_seq(a, b0, a_first);
    let (at2, rest_t) = transform_seq(&at, rest, a_first);
    let mut bout = b0t;
    bout.extend(rest_t);
    (at2, bout)
}

/// 문서 모델: op를 적용하고 비교용 정규형을 만든다. 퍼즈 하네스와 시험이 쓴다.
pub trait Doc: Clone + Debug {
    type Op: Transform;
    fn apply(&mut self, op: &Self::Op) -> Result<(), OtError>;
    /// 수렴 비교용 정규형(직렬화 순서·내부 id 정규화 후의 JSON). 내보낸 바이트가 아니라 모델 동등을 본다.
    fn canon(&self) -> Value;
}

/// 되돌리기: 적용 직전의 문서를 받아 효과를 지우는 op 열을 만든다(적용 순서의 역순으로 적용한다).
pub trait Invert<D: Doc>: Sized {
    fn invert(&self, before: &D) -> Vec<Self>;
}
