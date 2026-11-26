//! 차등 시험: JS quill-delta(5.x)가 만든 결과와 이 크레이트의 Delta가 같은지 본다.
//! 재료는 `tools/gen-quill-fixtures.mjs`가 만든다(ASCII 글만: JS는 UTF-16, 여기는 UTF-8 길이라 같게 한다). 배포 묶음에는 들어가지 않는다.
use craft_collab_ot::text::Delta;
use serde_json::Value;

fn d(v: &Value) -> Delta {
    serde_json::from_value(v.clone()).expect("delta 형식")
}

#[test]
fn quill_delta_와_같은_결과() {
    let cases: Vec<Value> = serde_json::from_str(include_str!("fixtures/quill.json")).unwrap();
    assert!(cases.len() >= 800);
    for (i, c) in cases.iter().enumerate() {
        let (base, a, b) = (d(&c["base"]), d(&c["a"]), d(&c["b"]));
        assert_eq!(base.compose(&a), d(&c["compose_base_a"]), "compose #{i}");
        assert_eq!(a.transform(&b, true), d(&c["t_b_after_a_prio"]), "transform(priority) #{i}");
        assert_eq!(a.transform(&b, false), d(&c["t_b_after_a_noprio"]), "transform(no priority) #{i}");
        // invert는 값이 아니라 효과를 비교한다: Quill은 없던 속성을 지우는 retain에 null을 남기지만(효과 없음) 이쪽은 줄인다
        let after = base.compose(&a);
        assert_eq!(after.compose(&a.invert(&base)), base, "invert(이 크레이트) 되돌림 #{i}");
        assert_eq!(after.compose(&d(&c["invert_a"])), base, "invert(quill) 되돌림 #{i}");
    }
}
