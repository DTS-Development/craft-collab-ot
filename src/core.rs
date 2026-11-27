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
    /// 두 op가 서로 건드리지 않아 `transform`이 양쪽을 그대로 돌려주는가(true면 변환을 건너뛴다).
    /// 확실하지 않으면 false로 둔다(기본값).
    fn independent(&self, _other: &Self) -> bool {
        false
    }
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
    // 재귀 깊이가 입력 길이가 아니라 "op가 여러 개로 쪼개진 겹침 수"에만 비례하도록 반복문으로 푼다
    // (원격 묶음 하나가 스택을 넘기지 못하게). 순서와 결과는 옛 재귀(`transform_seq_rec`)와 같다.
    if a.len() > 1 {
        // a의 op를 하나씩 현재의 b 전체와 맞추고, b는 그 결과로 바꿔 나간다
        let mut cur_b = b.to_vec();
        let mut out = Vec::with_capacity(a.len());
        for ai in a {
            let (t, nb) = transform_seq(std::slice::from_ref(ai), &cur_b, a_first);
            out.extend(t);
            cur_b = nb;
        }
        return (out, cur_b);
    }
    // a가 하나, b가 여럿: b의 op를 하나씩 현재의 a와 맞춘다
    let mut cur_a = a.to_vec();
    let mut bout = Vec::with_capacity(b.len());
    for bj in b {
        // 서로 건드리지 않는 op는 변환하지 않고 그대로 지나간다
        if cur_a.iter().all(|x| x.independent(bj)) {
            bout.push(bj.clone());
            continue;
        }
        let (na, bt) = transform_seq(&cur_a, std::slice::from_ref(bj), a_first);
        bout.extend(bt);
        cur_a = na;
    }
    (cur_a, bout)
}

/// 옛 재귀 구현: 반복문 구현과 결과가 같은지 견주는 시험용이다.
#[cfg(test)]
pub(crate) fn transform_seq_rec<T: Transform>(a: &[T], b: &[T], a_first: bool) -> (Vec<T>, Vec<T>) {
    if a.is_empty() || b.is_empty() {
        return (a.to_vec(), b.to_vec());
    }
    if a.len() == 1 && b.len() == 1 {
        return (a[0].transform(&b[0], a_first), b[0].transform(&a[0], !a_first));
    }
    if a.len() > 1 {
        let (a0, rest) = a.split_at(1);
        let (a0t, bt) = transform_seq_rec(a0, b, a_first);
        let (rest_t, bt2) = transform_seq_rec(rest, &bt, a_first);
        let mut out = a0t;
        out.extend(rest_t);
        return (out, bt2);
    }
    let (b0, rest) = b.split_at(1);
    let (at, b0t) = transform_seq_rec(a, b0, a_first);
    let (at2, rest_t) = transform_seq_rec(&at, rest, a_first);
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
