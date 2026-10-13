# カメラ評価用の録画データ (Issue #28)

各ディレクトリが1つの評価シナリオ。仕様とシナリオの一覧は `docs/camera-spec-v6.md` を参照。

- 録画ディレクトリと同じ形式（`meta.json` / `events.jsonl` / `ui_events.jsonl` / `window_rects.jsonl` / `frame_timestamps.txt` /
  `changes.jsonl`（画面の変化領域）/ `window_events.jsonl`（ウィンドウタイトルは消す））。
  **`frames/` と `audio.wav` はコミットしない**（`--debug-video` で目視確認するときは手元の録画を使う）。
- `expect.json` にシナリオ固有の期待値を書く。
- `expect.json` に `"synthetic": true` があるシナリオは `../gen_synthetic.py` で生成した合成データ。
  `gen_synthetic.py` を引数なしで実行すると、合成データのシナリオだけを再生成する（実録画は上書きしない）。

## 実録画への置き換え手順

### 1. 録画する

Snappi（`npm run tauri dev` か、ビルド済みのアプリ）で、下の表の操作を録画する。

- 録画の開始・停止は `Ctrl+Shift+R`。開始後 1 秒と停止前 1.5 秒の操作は無視される（仕様 R5）ので、
  **開始したら 2 秒ほど待ってから操作を始め、最後の操作のあと 4 秒ほど待ってから停止する**。
- `window-mode` 以外はディスプレイ録画にする。
- マウスはゆっくりでなくてよい。普段どおりに操作する。
- パスワードや個人情報は入力しない（入力した文字は記録されないが、UI 要素名とウィンドウ名は記録される）。

| シナリオ | 操作（目安 20〜30 秒） |
|---|---|
| `vscode-typing` | VS Code でエディタの中をクリック → 2〜3 行コードを入力 → `Ctrl+S` → マウスをエディタの外へ動かして待つ |
| `terminal` | ターミナル（Windows Terminal 等）を**あらかじめ前面にしておき**、クリックせずにコマンドを 2 つ入力して Enter |
| `browser-form` | ブラウザでフォームのあるページを開いておき、入力欄を 3〜4 個、順にクリックして入力 → 送信ボタン付近をクリック |
| `menu-navigation` | メモ帳等でメニューバーの「ファイル」→「名前を付けて保存」→ ダイアログでファイル名を入力 → キャンセル |
| `far-jumps` | 画面の左上と右下を交互に 6 回クリックする（各 1.5 秒ほど間を空ける）。何も起きない場所（デスクトップ等）でよい |
| `drag-scroll` | ファイルやウィンドウ内の要素を左から右へドラッグ＆ドロップ → 長いページの中央で 8 秒ほどスクロールし続ける |
| `idle-gaps` | どこかをクリックして入力 → **10 秒何もしない（マウスも動かさない）** → 別の場所をクリックして入力 |
| `window-mode` | ウィンドウ録画でエディタ等を録画。中をクリックして入力 → タイトルバーをドラッグしてウィンドウを動かす → 移動後のウィンドウ内をクリックして入力 |

### 2. 取り込む

```bash
# 最新の録画を vscode-typing として取り込む（録画ID か録画ディレクトリも指定できる）
python testdata/import_recording.py latest vscode-typing
```

`frames/` と `audio.wav` を除いてコピーし（`changes.jsonl` が無ければ先に frames から作る。`window_events.jsonl` は
ウィンドウタイトルを消す）、`meta.json` のローカルパスを消し、`expect.json` の下書き
（`snappi-eval --draft-expect`、仕様 R1〜R5 から求めた期待値）を書き出して、メトリクスを表示する。

### 3. 確認してコミットする

- 表示された **UI 要素名**（`ui_events.jsonl` の `name`）に公開したくないものがないか確認する。
- `expect.json` の下書きを確認し、シナリオの意図に合わない期待値は直す。下書きは仕様どおりの
  時間帯を機械的に置くだけなので、たとえば `idle-gaps` なら「放置中に全体表示」の zoomed_out が
  入っていることを確かめる。
- カメラの動きは手元の録画（frames あり）で目視確認できる:
  `cd src-tauri && cargo run --release --bin snappi-eval -- <録画ディレクトリ> --debug-video out.mp4`
- `expect.json` の下書きは時間帯と visible だけ。**`framed`（主役の矩形が枠内）と `within`（枠がウィンドウ内）は、
  録画フレームを見て「編集者ならこう映す」を人が書き足す**（エンジンの出力から書き写さない）。
- シナリオを置き換えたら、結果を再計測して `docs/camera-spec-v6.md` の §11 を更新する:

```bash
cd src-tauri
cargo run -q --bin snappi-eval -- ../testdata/recordings --table
```
