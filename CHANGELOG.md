# 바뀐 것 / Changes

## v0.29
- 검색칸에 치는 동안 마지막 말은 그 글자로 시작하는 태그·해시태그도 보임 (so → solo), Enter = 정확히 — while typing, the last word also matches tags / hashtags starting with it; Enter = exactly
- OR 모드면 콤마로 나눈 태그·해시태그는 하나라도 맞으면 됨 (필터와 - 조건은 그대로 좁힘) — in OR mode comma-separated tags and hashtags match if any one does (filters and - conditions still narrow)
- 검색 뷰 클릭(OR)은 같은 종류의 조건에만 | 로 붙음 — OR clicks in Search view join only a condition of the same kind
- 저장한 검색·최근 검색은 OR 를 a|b 로 적어 둠; 앨범을 오가는 것은 최근 검색에 안 쌓임 — saved and recent searches spell OR out as a|b; moving between albums no longer fills Recent searches
- 그림과 앨범을 같이 고르고 Del = 한 번 묻고 OK 때 같이 처리 — Del with pictures and albums selected asks once and does both on OK
- 앨범 옮기기·지우기·이름 바꾸기·가져오기 뒤 Ctrl+Z 는 되돌릴 수 없다고 알림 (예전 것을 몰래 되돌리지 않음) — Ctrl+Z after an album move / delete / rename or an import says it cannot be undone instead of quietly undoing an older edit
- 검색 뷰 목록 숫자가 '태그 여부' 거름을 따름; 검색 뷰에서는 album: 을 쳐도 거름 적용 — Search view counts follow the Workflow-info filter; typed album: words are filtered the same way
- 그림 오른쪽 클릭 → 앨범 ✓ 해제 = Del 과 같은 규칙; 같은 앨범에 붙여넣기 해도 잘라 둔 것 유지; 검색 뷰에서 잘라 둔 그림은 붙일 때 다른 앨범을 지우지 않음 — the Album submenu ✓ follows the Del rule; a cut survives a paste into the same album; a cut from Search view keeps its other albums
- '＋ 가져오기'도 열린 앨범에 넣음; 휴지통에 있는 그림을 다시 가져오면 알려 줌; 해시태그 이름을 바꾸면 a|b 안과 최근 검색도 따라 바뀜; 오른쪽 해시태그 칩 = 검색에 넣기·빼기 — the Import dialog puts pictures into the open album; re-importing a trashed picture says so; hashtag renames reach a|b groups and recent searches; the right-panel tag chip adds to / removes from the search

## v0.28
- 문장으로 된 프롬프트(Anima 등)도 문장 끝 마침표에서 태그를 나눔: "solo. A girl. jojo pose"에서 solo·jojo pose가 검색됨 — sentence prompts (Anima etc.) are also split into tags at full stops ending a sentence, so solo and jojo pose in "solo. A girl. jojo pose" are found

## v0.27
- 맨 위(/)에 있던 그림을 앨범에 복사해 넣으면 맨 위에도 그대로 남음 (그 앨범에서 Del = 앨범에서만 빠지고 맨 위에 남음). 옮기기(Ctrl+X, 그냥 끌어 놓기)는 맨 위에서 빠짐 — Copying a picture from the top (/) into an album keeps it at the top too (Del in that album only takes it out of the album). Moving (Ctrl+X, plain drag) takes it off the top

## v0.26
- 앨범 뷰의 맨 위(/)도 앨범처럼 그림을 둘 수 있음: 앨범에 든 그림을 맨 위에도 같이 (Ctrl+V, 끌어 놓기, 오른쪽 클릭 → 앨범 → / (맨 위)). 맨 위에서 Del = 맨 위에서만 빼기 — The Album view's top (/) holds pictures like an album: a picture in an album can sit at the top too (Ctrl+V, drag and drop, right-click → Album → / (top)). Del at the top removes it from the top only

## v0.25
- 앨범 뷰 Shift+Del: 그림은 imgr 휴지통으로 (모든 앨범에서 안 보임), 앨범은 하위 앨범·안의 그림까지 (그림은 휴지통) — Shift+Del in Album view: pictures go to the imgr Trash (gone from every album); albums are deleted with their sub-albums and pictures (pictures to the Trash)
- ⚙ 설정에서 Del ↔ Shift+Del 바꾸기 — Settings can swap Del and Shift+Del
- 휴지통 그림에 붙던 빨간 '휴지통' 글씨와 흐림 없앰 — pictures in the Trash are no longer dimmed and labelled

## v0.24
- 창 제목은 판 번호만 (imgr v0.24) — the window title is just the name and version

## v0.23
- 창 제목에 판 번호 (imgr v0.23 — …) — the window title shows the version

## v0.22
- 해시태그 옆 숫자에서 휴지통 그림을 뺌 (누르면 보이는 수와 같게) — hashtag counts no longer include pictures in the trash (same as what a click shows)

## v0.21
- 검색 조건을 콤마로 나눔 (long hair, scar) — search conditions are separated by commas
- 그냥 글자는 프롬프트 태그를 통째로 찾음, __scar__ / __scar / scar__ = 들어간 / 끝나는 / 시작하는 태그 — plain text finds a whole prompt tag; __scar__ / __scar / scar__ = contains / ends with / starts with
- 검색칸 왼쪽 [AND | OR]: 검색 뷰에서 누른 항목을 새 조건으로, 또는 마지막 조건에 | 로 붙임. Ctrl+클릭 = 그 종류는 이것만 — an [AND | OR] switch left of the search box: items clicked in Search view are added as a new condition or joined to the last one with |; Ctrl+click = only this of its kind

## v0.20
- 기록을 db.json 대신 data\imgr.db(SQLite)에 저장, 바뀐 그림만 씀 — records are kept in data\imgr.db (SQLite) instead of db.json; a save writes only the pictures that changed
- 처음 켤 때 db.json을 한 번 옮기고 하나하나 대조, db.json.v016으로 남김 — on first start db.json is moved once, checked field by field and kept as db.json.v016
- 앨범 이름이 길면 두 줄까지 보임 — long album names show on up to two lines

## v0.16
- 사전(L) 없앰: 사전 화면, `lib:` 검색, "사전 항목 만들기", "사전 예시로 고정" — removed the Library (L): its screen, the `lib:` search word and the related picture menu items

## v0.15
- PNG 정보를 연결선 따라 읽음: UNETLoader(Anima·Flux·GGUF) 모델도 체크포인트로, 문자열 노드로 만든 프롬프트도 — reads ComfyUI info by following wires: UNETLoader / GGUF models count as checkpoints, prompts built by string nodes are found
- 예전에 정보가 비어 있던 그림은 한 번 다시 읽음 — pictures that came out empty before are read again once

## v0.14
- Visual C++ 재배포 패키지(VCRUNTIME140.dll) 없이도 켜짐 — runs without the Visual C++ Redistributable (C runtime built in)
- 자동 빌드 도구 새 판으로 (Node 24) — release workflow actions updated (Node 24)

## v0.13
- 화면 언어 한국어 / English (⚙ 설정, 처음 켤 때는 윈도우 언어) — screen language Korean / English (⚙ Settings; first start follows Windows)
- 영문 README.md · docs/usage.md, 한글은 README_ko.md · docs/usage_ko.md
- 받기: GitHub Releases의 zip (GitHub Actions가 빌드) — download the zip from Releases (built by GitHub Actions)

## v0.12
- 스크립트 조작 창구(127.0.0.1:47811)는 처음엔 닫힘: ⚙ 설정 "스크립트 조작 허용" 또는 `IMGR_CTL=1`
- 두 번 켜면 먼저 켜진 창을 앞으로 (네트워크 없이 윈도우 신호로)
- 사전의 _gen 폴더는 기본값 없이 처음에 정함

## v0.11
- 크게 보기: `` ` `` 키로 위아래 안내 글 숨기기/보이기
- "탐색기에서 보기" = imgs 안에서 그 그림이 있는 폴더를 열고 그림을 골라 둠
- ⚙ 설정: 폴더 하나에 넣는 그림 수, 사라진 그림 기록 지우기
- 위의 보기 버튼(검색 뷰·앨범 뷰·크게·비교·표·사전) 없앰 — 왼쪽 칸과 단축키로
- 점수 N/A(점수 없음), 검색 뷰 "태그 여부"(워크플로 / 태그없음, 처음엔 워크플로만)
- 휴지통은 앨범 뷰의 `/` 다음으로, "오늘 넣은 것"·"사라진 것" 없앰
- 검색칸의 앨범을 이름으로 (`album:여행+`, 같은 이름이 또 있으면 경로 `album:여행/바다`)
- 오른쪽 칸: 프롬프트를 그림 바로 아래에(글자 크게), 패스, 시각 한 줄

## v0.10
- 검색 뷰 앨범 = 하위 앨범의 그림까지
- 앨범 뷰의 "무소속" 칸 없앰: 맨 위(`/`)에 앨범 다음으로 어느 앨범에도 없는 그림. 그림을 `/`에 놓기·Ctrl+X → 맨 위 Ctrl+V = 그 앨범에서 빼기

## v0.09
- 앨범 뷰: 앨범 칸 한 번 클릭 = 열기(고르기는 Ctrl+클릭), 그림을 다른 앨범에 끌어 놓기 = 옮기기(Ctrl = 복사)

## v0.08
- 앨범 뷰에서 방향키·PageUp/Down·Home/End가 앨범 칸과 그림을 한 줄로 이동
- 검색 뷰 앨범은 체크포인트처럼 클릭 하나 / Ctrl+클릭 여러 개, 앨범 나무는 줄 전체로 고름

## v0.07
- 앨범 뷰 / 검색 뷰로 나눔, 앨범 뷰에서 Ctrl+N = 새 앨범, 그림 Del = 이 앨범에서만 빼기

## v0.06
- 앨범 화면에서 검색칸 고치기 고침, 창이 좁으면 위 버튼이 다음 줄로

## v0.05
- 앨범 화면을 탐색기처럼: 앨범도 그림처럼 고르기·끌어 옮기기·Ctrl+X/V·Del·F2

## v0.04
- 프리셋 대신 앨범(하위 앨범, 여러 앨범에 같이), 그림 폴더 이름 `imgsrc` → `imgs` (v0.03 이전 판과 섞어 쓰지 말 것)

## v0.03
- 켤 때·F5에 폴더를 그대로 믿음, 직접 넣은 파일도 등록, 같은 그림(내용)은 한 번만, 이름이 겹치면 `이름 (2).png`

## v0.02
- 이름 바꾸기, 여러 개 같이 보기, LoRA 구분, 한글 입력 고침, 미리보기 모서리 손잡이

## v0.01
- 처음 판
