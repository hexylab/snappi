# Git フック（個人情報・秘密情報のチェック）

コミット・push の前に次を確かめる。CI（`.github/workflows/gitleaks.yml`）でも同じチェックを行う。

| フック | 内容 |
|---|---|
| `pre-commit` | 作者・コミッターのメールアドレスが GitHub の noreply か / ステージした変更を gitleaks で検査 |
| `pre-push` | push するコミットのメールアドレス / そのコミットを gitleaks で検査 |

gitleaks のルールは `.gitleaks.toml`（標準ルール + Windows のユーザーフォルダのパス + メールアドレス）。

## 準備

```bash
winget install gitleaks
npm install   # prepare スクリプトが git config core.hooksPath .githooks を設定する
```

npm を使わない場合は `git config core.hooksPath .githooks` を手で実行する。

## 個人に固有の文字列（任意・推奨）

ユーザー名やメールアドレスなど、自分に固有の文字列は `.gitleaks.toml` に書かない（書くとそれ自体が公開される）。
`.git/gitleaks.local.toml` に書けばコミットされず、フックはこちらを優先して使う。

```toml
[extend]
path = ".gitleaks.toml"

[[rules]]
id = "local-personal-strings"
description = "個人に固有の文字列"
regex = '''(?i)(ユーザー名|メールアドレスのローカル部)'''
```

## 誤検出のとき

- 例示用のパスは `C:\Users\...\` のように書く（名前の部分が `.` で始まれば対象外）。
- それでも誤検出になる行は、行末に `gitleaks:allow` を書くと除外できる。
