//! 수렴 퍼즈 하네스: 서버 순서 OT의 클라이언트·서버를 메모리에서 흉내 낸다.
//!
//! - 서버는 op를 해석하지 않고 `rev`만 매겨 로그에 쌓고 모두에게 방송한다(실제 doc-sync와 같다).
//! - 클라이언트는 `confirmed`(서버 순서대로 적용한 모델)와 `local`(confirmed + 미확정 op)을 쥔다.
//!   들어온 로그 항목은 자기 `[base, rev)` 구간의 확정 항목에 대해 같은 순서로 변환해 정규형으로 만든다.
//!   미확정 묶음은 한 번에 하나만 서버에 나가 있다(`inflight`), 그동안 만든 편집은 `buffer`에 쌓는다.
//! - 채널은 클라이언트별 FIFO이고, 어느 채널을 먼저 처리할지·중복 전송 여부를 seed 난수로 정한다.
//!
//! 앱은 [`SimDoc`]만 구현하면 같은 하네스로 수렴을 시험할 수 있다.
use crate::core::{transform_seq, Doc};
use std::collections::VecDeque;

/// seed 하나로 재현되는 작은 난수기(xorshift64*).
pub struct Rng(u64);
impl Rng {
    pub fn new(seed: u64) -> Self {
        Rng(seed.wrapping_mul(0x9E37_79B9_7F4A_7C15) | 1)
    }
    pub fn next(&mut self) -> u64 {
        self.0 ^= self.0 >> 12;
        self.0 ^= self.0 << 25;
        self.0 ^= self.0 >> 27;
        self.0.wrapping_mul(0x2545_F491_4F6C_DD1D)
    }
    /// 0..n (n > 0)
    pub fn below(&mut self, n: usize) -> usize {
        (self.next() % n as u64) as usize
    }
    /// num/den 확률로 참
    pub fn chance(&mut self, num: usize, den: usize) -> bool {
        self.below(den) < num
    }
}

/// 퍼즈가 쓸 문서: 시작 상태와 현재 상태에서 유효한 무작위 op를 만든다.
pub trait SimDoc: Doc {
    /// 모든 클라이언트가 같은 상태로 시작한다.
    fn initial() -> Self;
    /// 이 문서에 바로 적용할 수 있는 op 하나(없으면 None).
    fn random_op(&self, rng: &mut Rng) -> Option<Self::Op>;
}

#[derive(Clone, Debug)]
pub struct Entry<Op> {
    pub rev: u64,
    pub client: usize,
    pub seq: u64,
    /// 보낸 쪽이 마지막으로 적용한 rev(이 상태에서 ops가 시작한다)
    pub base: u64,
    pub ops: Vec<Op>,
}

struct Client<D: Doc> {
    id: usize,
    rev: u64,
    confirmed: D,
    local: D,
    inflight: Vec<D::Op>,
    buffer: Vec<D::Op>,
    /// 확정된 정규형 로그(인덱스 = rev - 1)
    log: Vec<Vec<D::Op>>,
    seq: u64,
    last_sent: Option<Entry<D::Op>>,
}

impl<D: Doc> Client<D> {
    fn new(id: usize, init: &D) -> Self {
        Client { id, rev: 0, confirmed: init.clone(), local: init.clone(), inflight: vec![], buffer: vec![], log: vec![], seq: 0, last_sent: None }
    }

    /// 나가 있는 묶음이 없고 쌓인 편집이 있으면 한 묶음으로 보낸다.
    fn try_send(&mut self) -> Option<Entry<D::Op>> {
        if !self.inflight.is_empty() || self.buffer.is_empty() {
            return None;
        }
        self.inflight = std::mem::take(&mut self.buffer);
        self.seq += 1;
        let e = Entry { rev: 0, client: self.id, seq: self.seq, base: self.rev, ops: self.inflight.clone() };
        self.last_sent = Some(e.clone());
        Some(e)
    }

    fn receive(&mut self, e: &Entry<D::Op>) -> Result<(), String> {
        if e.rev != self.rev + 1 {
            return Err(format!("client {} rev 건너뜀: 받은 {} 가진 {}", self.id, e.rev, self.rev));
        }
        // 보낸 쪽이 아직 몰랐던 확정 항목(base, rev)에 대해 같은 순서로 변환해 정규형을 만든다
        let mut canon = e.ops.clone();
        for r in (e.base as usize)..(e.rev as usize - 1) {
            canon = transform_seq(&canon, &self.log[r], false).0;
        }
        for op in &canon {
            self.confirmed.apply(op).map_err(|x| format!("client {} confirmed 적용 실패 rev {}: {x}", self.id, e.rev))?;
        }
        if e.client == self.id {
            // 내 묶음이 확정됐다. local은 이미 반영돼 있다.
            self.inflight.clear();
            self.last_sent = None;
        } else {
            // 남의 확정 op를 내 미확정 묶음 뒤로 옮겨 local에 적용한다(확정 op가 앞선다)
            let (c1, inflight) = transform_seq(&canon, &self.inflight, true);
            let (c2, buffer) = transform_seq(&c1, &self.buffer, true);
            for op in &c2 {
                self.local.apply(op).map_err(|x| format!("client {} local 적용 실패 rev {}: {x}", self.id, e.rev))?;
            }
            self.inflight = inflight;
            self.buffer = buffer;
        }
        self.rev = e.rev;
        self.log.push(canon);
        Ok(())
    }
}

#[derive(Clone, Debug)]
pub struct FuzzFail {
    pub seed: u64,
    pub clients: usize,
    /// 실패를 재현하는 가장 작은 편집 수(prefix 축소 결과)
    pub steps: usize,
    pub message: String,
}

/// 한 번 돌린다. 편집을 `steps`번까지 만들고, 모든 채널이 빌 때까지 처리한 뒤 정규형을 비교한다.
pub fn run_once<D: SimDoc>(seed: u64, clients: usize, steps: usize) -> Result<(), String> {
    let mut rng = Rng::new(seed);
    let init = D::initial();
    let mut cs: Vec<Client<D>> = (0..clients).map(|i| Client::new(i, &init)).collect();
    // 서버: rev 매기기 + 중복 거름. op 내용은 보지 않는다.
    let mut srv_rev = 0u64;
    let mut srv_seen = vec![0u64; clients];
    let mut up: Vec<VecDeque<Entry<D::Op>>> = (0..clients).map(|_| VecDeque::new()).collect();
    let mut down: Vec<VecDeque<Entry<D::Op>>> = (0..clients).map(|_| VecDeque::new()).collect();
    let mut budget = steps;
    let mut guard = 0usize;
    loop {
        guard += 1;
        if guard > 200 * (steps + clients) + 10_000 {
            return Err("하네스가 끝나지 않음".into());
        }
        // 일어날 수 있는 일: 편집(0) / 올리기 도착(1) / 내리기 도착(2) / 마지막 전송 재전송(3)
        let mut evs: Vec<(u8, usize)> = vec![];
        for c in 0..clients {
            if budget > 0 {
                evs.push((0, c));
                if cs[c].last_sent.is_some() {
                    evs.push((3, c));
                }
            }
            if !up[c].is_empty() {
                evs.push((1, c));
            }
            if !down[c].is_empty() {
                evs.push((2, c));
            }
        }
        if evs.is_empty() {
            break;
        }
        let (kind, c) = evs[rng.below(evs.len())];
        match kind {
            0 => {
                budget -= 1;
                if let Some(op) = cs[c].local.random_op(&mut rng) {
                    cs[c].local.apply(&op).map_err(|x| format!("client {c} 만든 op를 못 적용: {x}"))?;
                    cs[c].buffer.push(op);
                }
            }
            1 => {
                let m = up[c].pop_front().unwrap();
                if m.seq > srv_seen[c] {
                    if m.base > srv_rev {
                        return Err(format!("base가 서버 rev보다 큼: {m:?}"));
                    }
                    srv_seen[c] = m.seq;
                    srv_rev += 1;
                    let e = Entry { rev: srv_rev, ..m };
                    for q in down.iter_mut() {
                        q.push_back(e.clone());
                    }
                } // 이미 처리한 seq(중복 전송)는 버린다
            }
            2 => {
                let m = down[c].pop_front().unwrap();
                cs[c].receive(&m)?;
            }
            _ => {
                if let Some(e) = cs[c].last_sent.clone() {
                    up[c].push_back(e); // 같은 seq를 다시 보낸다
                }
            }
        }
        if let Some(e) = cs[c].try_send() {
            up[c].push_back(e);
        }
        // 확정된 직후 다음 묶음도 바로 나갈 수 있게, 다른 클라이언트도 점검한다
        for i in 0..clients {
            if let Some(e) = cs[i].try_send() {
                up[i].push_back(e);
            }
        }
    }
    for c in &cs {
        if !c.inflight.is_empty() || !c.buffer.is_empty() {
            return Err(format!("client {} 미확정 op가 남음", c.id));
        }
        if c.rev != srv_rev {
            return Err(format!("client {} rev {} != 서버 {}", c.id, c.rev, srv_rev));
        }
        if c.confirmed.canon() != c.local.canon() {
            return Err(format!("client {} confirmed != local\n  {}\n  {}", c.id, c.confirmed.canon(), c.local.canon()));
        }
    }
    let first = cs[0].confirmed.canon();
    for c in &cs[1..] {
        if c.confirmed.canon() != first {
            return Err(format!("수렴 실패 client 0 vs {}\n  {}\n  {}", c.id, first, c.confirmed.canon()));
        }
    }
    Ok(())
}

/// seed 범위를 돌린다. 실패하면 편집 수를 줄여 가며 가장 작은 재현을 찾아 돌려준다.
pub fn run_fuzz<D: SimDoc>(seeds: std::ops::Range<u64>, clients: usize, steps: usize) -> Result<(), FuzzFail> {
    for seed in seeds {
        if let Err(first) = run_once::<D>(seed, clients, steps) {
            let (mut best, mut message) = (steps, first);
            for n in 1..steps {
                if let Err(m) = run_once::<D>(seed, clients, n) {
                    best = n;
                    message = m;
                    break;
                }
            }
            return Err(FuzzFail { seed, clients, steps: best, message });
        }
    }
    Ok(())
}
