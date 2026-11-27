# craft-collab-ot

문서 편집기 여럿이 같은 문서를 동시에 편집할 때 쓰는 변환(OT) 코드다. 서버 순서 OT 방식이다.

- 서버는 변경을 변환하지 않는다. 받은 순서대로 방 전체 번호(`rev`)를 매겨 모두에게 돌려줄 뿐이다.
- 변환은 각 편집기(클라이언트)가 이 크레이트로만 한다. 같은 순서로 같은 변경을 받으면 모두 같은 문서가 된다.

## 구성

| 모듈 | 내용 |
|---|---|
| `client` | 편집기용 클라이언트 상태기계 `SyncClient`(보낸 변경 확정 대기, 서버 변경 변환, 다시 접속) |
| `core` | 공통 계약 `Transform`·`Doc`·`Invert`·`transform_seq` |
| `reg` | 키-값 레지스터(마지막 쓰기가 이김) |
| `seq` | 원소 목록의 삽입·삭제·이동 |
| `cmd` | 명령 재실행(앱이 변환 규칙을 정한다) |
| `text` | 서식 텍스트 Delta OT(quill-delta와 같은 결과를 내는지 `tests/quill_diff.rs`가 견준다) |
| `sim` | 서버 순서 하네스와 수렴 퍼즈(편집기 여럿이 무작위로 편집해도 같은 문서로 모이는지 확인) |

## 쓰는 곳

DTS-Development의 문서 편집기 포크(deckcraft 등)가 Cargo git 의존(rev 고정)으로 가져다 쓴다. 서버·브라우저 연결 쪽은
dts-craft의 `@dts-development/doc-sync`(서버)와 `@dts-development/craft-bridge`(호스트)가 맡는다.

## 시험

```sh
cargo test
```

`tests/fixtures`는 `tools/gen-quill-fixtures.mjs`로 만든다.

## 라이선스

MIT 또는 Apache-2.0 중 하나를 고를 수 있다. [LICENSE-MIT](LICENSE-MIT), [LICENSE-APACHE](LICENSE-APACHE).
