# 바뀐 것 / Changes

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
