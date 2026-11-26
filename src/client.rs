//! 편집기용 클라이언트 상태기계(서버 순서 OT). 문서 모델을 모르고 op만 다룬다.
//!
//! 쥐는 것: 확정 로그의 정규형(변환 재료), 나가 있는 묶음 하나(`inflight`), 그동안 쌓인 편집(`buffer`).
//! 문서 모델(`confirmed`·`local`)은 호출한 쪽이 갖고, 이 타입이 돌려주는 op를 적용한다:
//! - `Received::confirmed`: 확정 모델에 적용할 정규형 op
//! - `Received::local`: 로컬 모델에 적용할 op(내 미확정 묶음 뒤로 옮긴 것, 내 묶음이 확정되면 비어 있다)
//!
//! 호스트(craft-bridge)와의 약속: 한 번에 묶음 하나만 나간다. 재접속 때는 [`SyncClient::resend`]로
//! 원래 묶음(같은 `seq`·`base`·원본 op)을 그대로 다시 보내고, join `docRev`·하트비트 `rev`는
//! [`SyncClient::safe_rev`]를 쓴다(미확정 묶음의 base 밑으로 두어야 서버가 변환 재료를 지우지 않는다).
use crate::core::{transform_seq, Transform};
use std::collections::VecDeque;

/// 서버가 확정해 내려준 로그 항목(호스트가 `collabChanged`/`apply`에서 넘긴 모양).
#[derive(Clone, Debug)]
pub struct Entry<Op> {
    pub rev: u64,
    /// 보낸 쪽이 이 묶음을 만들 때 기대던 확정 rev
    pub base: u64,
    /// 내가 보낸 묶음이 돌아온 것인가(호스트가 clientId로 판정한다)
    pub own: bool,
    pub ops: Vec<Op>,
}

/// 서버로 내보낼 묶음.
#[derive(Clone, Debug, PartialEq)]
pub struct Batch<Op> {
    pub seq: u64,
    pub base: u64,
    pub ops: Vec<Op>,
}

#[derive(Clone, Debug, PartialEq)]
pub enum ClientError {
    /// 다음 rev가 아닌 항목이 왔다(받은, 가진).
    Gap { got: u64, have: u64 },
    /// 변환에 필요한 확정 로그가 없다(floor 밑으로 지웠거나 replay가 모자란다). 재접속으로 다시 받아야 한다.
    MissingContext { rev: u64 },
    /// 내 묶음이라는데 나가 있는 묶음이 없다.
    UnexpectedOwn { rev: u64 },
}
impl std::fmt::Display for ClientError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            ClientError::Gap { got, have } => write!(f, "rev 건너뜀: 받은 {got} 가진 {have}"),
            ClientError::MissingContext { rev } => write!(f, "변환 재료 rev {rev} 없음"),
            ClientError::UnexpectedOwn { rev } => write!(f, "나가 있는 묶음이 없는데 내 항목이 옴 (rev {rev})"),
        }
    }
}
impl std::error::Error for ClientError {}

/// [`SyncClient::receive`] 결과.
#[derive(Clone, Debug, PartialEq)]
pub struct Received<Op> {
    /// 이미 가진 rev(재접속 replay의 변환 재료)였다. 모델에 적용할 것은 없다.
    pub old: bool,
    pub own: bool,
    pub confirmed: Vec<Op>,
    pub local: Vec<Op>,
}

#[derive(Clone, Debug)]
pub struct SyncClient<Op: Transform> {
    rev: u64,
    /// 이 rev 이하의 정규형 로그는 없다(floor 또는 reset 지점)
    log_floor: u64,
    /// log[i]는 rev `log_floor + 1 + i`의 정규형
    log: VecDeque<Vec<Op>>,
    inflight: Vec<Op>,
    awaiting: bool,
    sent: Option<Batch<Op>>,
    buffer: Vec<Op>,
    seq: u64,
}

impl<Op: Transform> SyncClient<Op> {
    /// `rev`: 지금 열린 문서가 반영한 서버 rev. `last_seq`: 서버가 확정한 이 clientId의 마지막 번호(welcome.lastSeq).
    pub fn new(rev: u64, last_seq: u64) -> Self {
        SyncClient { rev, log_floor: rev, log: VecDeque::new(), inflight: vec![], awaiting: false, sent: None, buffer: vec![], seq: last_seq }
    }
    pub fn rev(&self) -> u64 {
        self.rev
    }
    /// 나가 있거나 쌓인 편집이 있다.
    pub fn has_pending(&self) -> bool {
        self.awaiting || !self.buffer.is_empty()
    }
    /// join `docRev`·하트비트 `rev`로 보낼 값: 나가 있는 묶음의 base 밑.
    pub fn safe_rev(&self) -> u64 {
        match (&self.sent, self.awaiting) {
            (Some(b), true) => self.rev.min(b.base),
            _ => self.rev,
        }
    }
    /// 미확정 편집(나간 것 + 쌓인 것)을 확정 모델 위에 이어 적용할 순서로 돌려준다.
    pub fn pending(&self) -> Vec<Op> {
        self.inflight.iter().chain(self.buffer.iter()).cloned().collect()
    }
    /// 로컬 편집을 쌓는다(로컬 모델에는 호출한 쪽이 이미 적용했다).
    pub fn local_edit(&mut self, ops: impl IntoIterator<Item = Op>) {
        self.buffer.extend(ops);
    }
    /// 나가 있는 묶음이 없고 쌓인 편집이 있으면 한 묶음으로 낸다.
    pub fn next_send(&mut self) -> Option<Batch<Op>> {
        if self.awaiting || self.buffer.is_empty() {
            return None;
        }
        self.inflight = std::mem::take(&mut self.buffer);
        self.awaiting = true;
        self.seq += 1;
        let b = Batch { seq: self.seq, base: self.rev, ops: self.inflight.clone() };
        self.sent = Some(b.clone());
        Some(b)
    }
    /// 확정되지 않은 묶음을 원래 그대로(같은 seq·base·원본 op) 다시 보낸다.
    pub fn resend(&self) -> Option<Batch<Op>> {
        if self.awaiting { self.sent.clone() } else { None }
    }
    /// 서버가 알린 floor 이하의 변환 재료를 버린다. 나가 있는 묶음이 기댄 구간은 서버가 지켜 준다.
    pub fn set_floor(&mut self, floor: u64) {
        while self.log_floor < floor && !self.log.is_empty() {
            self.log.pop_front();
            self.log_floor += 1;
        }
    }
    /// welcome의 floor를 받았다. 접속마다 서버가 아는 값이 기준이다(이전 값과 max를 취하지 않는다).
    /// 방이 다시 만들어져 floor가 내려가면 로그를 비우고 하한을 내린다. 이어지는 replay가 `floor+1`부터 rev까지의 변환 재료를 다시 채운다.
    pub fn welcome_floor(&mut self, floor: u64) {
        if floor < self.log_floor {
            self.log.clear();
            self.log_floor = floor;
        } else {
            self.set_floor(floor);
        }
    }
    /// 문서를 저장본(`rev`)으로 다시 열었다. 로그와 미확정 편집을 버리고 버린 편집(나간 것 + 쌓인 것)을 돌려준다
    /// (호출한 쪽이 새 문서 위에 다시 얹는다. 3-way).
    pub fn reset(&mut self, rev: u64) -> Vec<Op> {
        let dropped = self.pending();
        self.rev = rev;
        self.log_floor = rev;
        self.log.clear();
        self.inflight.clear();
        self.buffer.clear();
        self.awaiting = false;
        self.sent = None;
        dropped
    }

    fn log_at(&self, rev: u64) -> Option<&Vec<Op>> {
        if rev <= self.log_floor { None } else { self.log.get((rev - self.log_floor - 1) as usize) }
    }
    /// base 뒤 확정 구간을 따라 항목을 정규형으로 만든다.
    fn canonicalize(&self, e: &Entry<Op>) -> Result<Vec<Op>, ClientError> {
        let mut canon = e.ops.clone();
        for r in (e.base + 1)..e.rev {
            let l = self.log_at(r).ok_or(ClientError::MissingContext { rev: r })?;
            canon = transform_seq(&canon, l, false).0;
        }
        Ok(canon)
    }

    /// 서버가 확정한 항목을 순서대로 받는다.
    pub fn receive(&mut self, e: &Entry<Op>) -> Result<Received<Op>, ClientError> {
        if e.rev <= self.rev {
            // 재접속 replay: 이미 반영한 rev. 변환 재료가 모자라면 채운다.
            if e.rev > self.log_floor {
                let idx = (e.rev - self.log_floor - 1) as usize;
                if idx >= self.log.len() {
                    // 로그가 비어 있던 구간(새로 연 문서·reset 뒤)을 채울 수 없는 순서면 무시한다
                    if idx == self.log.len() {
                        let c = self.canonicalize(e)?;
                        self.log.push_back(c);
                    }
                }
            }
            return Ok(Received { old: true, own: false, confirmed: vec![], local: vec![] });
        }
        if e.rev != self.rev + 1 {
            return Err(ClientError::Gap { got: e.rev, have: self.rev });
        }
        let canon = self.canonicalize(e)?;
        let mut local = vec![];
        if e.own {
            if !self.awaiting {
                return Err(ClientError::UnexpectedOwn { rev: e.rev });
            }
            self.inflight.clear();
            self.awaiting = false;
            self.sent = None;
        } else {
            // 확정 op가 앞선다: 내 미확정 묶음 뒤로 옮긴다
            let (c1, inflight) = transform_seq(&canon, &self.inflight, true);
            let (c2, buffer) = transform_seq(&c1, &self.buffer, true);
            local = c2;
            self.inflight = inflight;
            self.buffer = buffer;
        }
        self.rev = e.rev;
        self.log.push_back(canon.clone());
        Ok(Received { old: false, own: e.own, confirmed: canon, local })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::reg::RegOp;
    use serde_json::json;

    fn entry(rev: u64, base: u64, own: bool, ops: Vec<RegOp>) -> Entry<RegOp> {
        Entry { rev, base, own, ops }
    }

    #[test]
    fn 보내기_재전송_safe_rev() {
        let mut c = SyncClient::<RegOp>::new(3, 7);
        c.local_edit([RegOp::set("a", json!(1))]);
        let b = c.next_send().unwrap();
        assert_eq!((b.seq, b.base), (8, 3));
        assert!(c.next_send().is_none()); // 나가 있는 동안은 하나만
        c.local_edit([RegOp::set("b", json!(2))]);
        // 남의 항목이 오면 rev는 오르지만 safe_rev는 나간 묶음의 base 밑이다
        let r = c.receive(&entry(4, 3, false, vec![RegOp::set("a", json!(9))])).unwrap();
        assert_eq!(c.rev(), 4);
        assert_eq!(c.safe_rev(), 3);
        assert!(r.local.is_empty() || !r.local.is_empty());
        assert_eq!(c.resend().unwrap(), b); // 원본 그대로
        // 내 묶음이 확정되면 다음 묶음이 나간다
        let r = c.receive(&entry(5, 3, true, b.ops.clone())).unwrap();
        assert!(r.own && r.local.is_empty());
        assert_eq!(c.safe_rev(), 5);
        assert_eq!(c.next_send().unwrap().seq, 9);
    }

    #[test]
    fn 건너뜀과_재료_없음과_reset() {
        let mut c = SyncClient::<RegOp>::new(0, 0);
        assert_eq!(c.receive(&entry(2, 0, false, vec![])), Err(ClientError::Gap { got: 2, have: 0 }));
        assert_eq!(c.receive(&entry(1, 0, true, vec![])), Err(ClientError::UnexpectedOwn { rev: 1 }));
        c.receive(&entry(1, 0, false, vec![RegOp::set("a", json!(1))])).unwrap();
        c.set_floor(1);
        assert_eq!(c.receive(&entry(2, 0, false, vec![])), Err(ClientError::MissingContext { rev: 1 }));
        c.local_edit([RegOp::set("x", json!(1))]);
        let dropped = c.reset(10);
        assert_eq!(dropped.len(), 1);
        assert_eq!((c.rev(), c.has_pending()), (10, false));
    }

    #[test]
    fn replay로_모자란_변환_재료를_채운다() {
        let mut c = SyncClient::<RegOp>::new(2, 0); // 새로 연 문서: rev 2, 로그 없음
        // floor 위의 이미 반영한 rev는 하한 밑이라 무시된다
        assert!(c.receive(&entry(1, 0, false, vec![])).unwrap().old);
        c.welcome_floor(0); // 방이 다시 만들어져 floor가 내려감: replay가 재료를 채운다
        assert!(c.receive(&entry(1, 0, false, vec![RegOp::set("a", json!(0))])).unwrap().old);
        assert!(c.receive(&entry(2, 1, false, vec![RegOp::set("a", json!(1))])).unwrap().old);
        let r = c.receive(&entry(3, 1, false, vec![RegOp::set("a", json!(2))])).unwrap(); // rev 2를 재료로 변환한다
        assert!(!r.old);
        assert_eq!(c.rev(), 3);
    }
}
