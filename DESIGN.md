---
version: alpha
name: Hexyl Light
description: Hexyl のサービス・アプリケーションで共通に使う、クリーム地とオレンジを基調にした温かみのあるライトテーマのデザインシステム。
colors:
  background: "#faf6f0"
  surface: "#ffffff"
  surface-dim: "#f3ece2"
  outline: "#ece3d6"
  on-surface: "#14100a"
  on-surface-dim: "#857662"
  primary: "#f08c26"
  primary-strong: "#e0790f"
  on-primary: "#ffffff"
  primary-deep: "#b9620e"
  primary-deep-outline: "#ba650f"
  primary-container: "#fff5e8"
  on-primary-container: "#934900"
  eyebrow: "#916029"
  muted: "#70614f"
  error: "#d9544f"
  error-container: "#fdf0ef"
  warning-container: "#fff3cd"
  on-warning-container: "#594500"
  success: "#2fae6e"
  switch-off: "#a99d8d"
  option-outline: "#ded4c6"
  dialog-subtitle: "#6c5c49"
  step-badge: "#995515"
  step-badge-container: "#fff0dc"
  select-outline: "#d9cebe"
  tip-container: "#f7f2e9"
  on-tip-container: "#624b2f"
typography:
  headline-md:
    fontFamily: "system-ui, Inter, 'Hiragino Sans', 'Noto Sans JP', sans-serif"
    fontSize: 24px
    fontWeight: 700
    lineHeight: 1.6
  headline-sm:
    fontFamily: "system-ui, Inter, 'Hiragino Sans', 'Noto Sans JP', sans-serif"
    fontSize: 18px
    fontWeight: 700
    lineHeight: 1.6
    letterSpacing: 0.01em
  title:
    fontFamily: "system-ui, Inter, 'Hiragino Sans', 'Noto Sans JP', sans-serif"
    fontSize: 16px
    fontWeight: 700
    lineHeight: 1.6
  reading-lg:
    fontFamily: "system-ui, Inter, 'Hiragino Sans', 'Noto Sans JP', sans-serif"
    fontSize: 22px
    fontWeight: 400
    lineHeight: 1.7
  body-md:
    fontFamily: "system-ui, Inter, 'Hiragino Sans', 'Noto Sans JP', sans-serif"
    fontSize: 16px
    fontWeight: 400
    lineHeight: 1.6
  body-sm:
    fontFamily: "system-ui, Inter, 'Hiragino Sans', 'Noto Sans JP', sans-serif"
    fontSize: 14px
    fontWeight: 400
    lineHeight: 1.6
  label-md:
    fontFamily: "system-ui, Inter, 'Hiragino Sans', 'Noto Sans JP', sans-serif"
    fontSize: 14px
    fontWeight: 700
    lineHeight: 1.6
  label-lg:
    fontFamily: "system-ui, Inter, 'Hiragino Sans', 'Noto Sans JP', sans-serif"
    fontSize: 15px
    fontWeight: 700
    lineHeight: 1.6
  label-sm:
    fontFamily: "system-ui, Inter, 'Hiragino Sans', 'Noto Sans JP', sans-serif"
    fontSize: 13px
    fontWeight: 600
    lineHeight: 1.6
  label-sm-bold:
    fontFamily: "system-ui, Inter, 'Hiragino Sans', 'Noto Sans JP', sans-serif"
    fontSize: 13px
    fontWeight: 700
    lineHeight: 1.6
  label-sm-regular:
    fontFamily: "system-ui, Inter, 'Hiragino Sans', 'Noto Sans JP', sans-serif"
    fontSize: 13px
    fontWeight: 400
    lineHeight: 1.6
  caption:
    fontFamily: "system-ui, Inter, 'Hiragino Sans', 'Noto Sans JP', sans-serif"
    fontSize: 12px
    fontWeight: 400
    lineHeight: 1.65
  eyebrow:
    fontFamily: "system-ui, Inter, 'Hiragino Sans', 'Noto Sans JP', sans-serif"
    fontSize: 11px
    fontWeight: 700
    lineHeight: 1.6
    letterSpacing: 0.14em
  card-title:
    fontFamily: "system-ui, Inter, 'Hiragino Sans', 'Noto Sans JP', sans-serif"
    fontSize: 13px
    fontWeight: 700
    lineHeight: 1.6
    letterSpacing: 0.04em
rounded:
  md: 12px
  lg: 18px
  xl: 22px
  full: 999px
spacing:
  xs: 4px
  sm: 8px
  md: 12px
  lg: 16px
  xl: 24px
  gutter: 16px
  margin: 24px
  sidebar-width: 260px
components:
  button-primary:
    backgroundColor: "{colors.primary}"
    textColor: "{colors.on-primary}"
    typography: "{typography.label-md}"
    rounded: "{rounded.full}"
    padding: "{spacing.sm}"
  button-primary-hover:
    backgroundColor: "{colors.primary-strong}"
  button-danger:
    backgroundColor: "{colors.error}"
    textColor: "{colors.on-primary}"
    typography: "{typography.label-md}"
    rounded: "{rounded.full}"
    padding: "{spacing.sm}"
  button-secondary:
    backgroundColor: "{colors.surface-dim}"
    textColor: "{colors.on-surface}"
    typography: "{typography.label-md}"
    rounded: "{rounded.full}"
    padding: "{spacing.sm}"
  button-secondary-hover:
    backgroundColor: "{colors.outline}"
  button-toggle:
    backgroundColor: "{colors.surface}"
    textColor: "{colors.on-surface}"
    typography: "{typography.label-md}"
    rounded: "{rounded.full}"
    padding: "{spacing.sm}"
  button-toggle-pressed:
    backgroundColor: "{colors.surface-dim}"
    textColor: "{colors.on-primary-container}"
  button-icon:
    backgroundColor: transparent
    textColor: "{colors.on-surface-dim}"
    rounded: 8px
    padding: "{spacing.xs}"
    size: 16px
  button-icon-hover:
    backgroundColor: "{colors.surface-dim}"
    textColor: "{colors.on-surface}"
  button-compact:
    backgroundColor: "{colors.surface-dim}"
    textColor: "{colors.on-surface}"
    typography: "{typography.label-sm-bold}"
    rounded: "{rounded.md}"
    padding: "{spacing.sm}"
  status-pill:
    backgroundColor: "{colors.surface}"
    textColor: "{colors.on-surface-dim}"
    typography: "{typography.label-sm}"
    rounded: "{rounded.full}"
    padding: "{spacing.xs}"
  status-pill-live:
    textColor: "{colors.success}"
  status-pill-error:
    textColor: "{colors.error}"
  meter-track:
    backgroundColor: "{colors.surface-dim}"
    rounded: "{rounded.full}"
    width: 96px
    height: 8px
  meter-fill:
    backgroundColor: "{colors.success}"
    rounded: "{rounded.full}"
    height: 8px
  card:
    backgroundColor: "{colors.surface}"
    rounded: "{rounded.lg}"
  card-title:
    textColor: "{colors.muted}"
    typography: "{typography.card-title}"
    padding: "{spacing.md}"
  card-label:
    textColor: "{colors.on-surface}"
    typography: "{typography.label-lg}"
  reading-text:
    textColor: "{colors.on-surface}"
    typography: "{typography.reading-lg}"
    padding: "{spacing.lg}"
  reading-text-pending:
    textColor: "{colors.muted}"
  empty-placeholder:
    textColor: "{colors.on-surface-dim}"
    typography: "{typography.body-sm}"
  floating-chip:
    backgroundColor: "{colors.primary}"
    textColor: "{colors.on-primary}"
    typography: "{typography.label-sm}"
    rounded: "{rounded.full}"
    padding: "{spacing.sm}"
  floating-chip-hover:
    backgroundColor: "{colors.primary-strong}"
  input-select:
    backgroundColor: "{colors.surface}"
    textColor: "{colors.on-surface}"
    typography: "{typography.label-sm-regular}"
    rounded: 10px
    padding: 7px
    height: 40px
    borderColor: "{colors.select-outline}"
  sidebar:
    backgroundColor: "{colors.surface}"
    rounded: "{rounded.lg}"
    padding: "{spacing.md}"
    width: "{spacing.sidebar-width}"
  sidebar-item:
    backgroundColor: transparent
    textColor: "{colors.on-surface}"
    typography: "{typography.label-sm}"
    rounded: "{rounded.md}"
    padding: "{spacing.sm}"
  sidebar-item-active:
    backgroundColor: "{colors.surface-dim}"
    textColor: "{colors.primary-strong}"
  disclosure-summary:
    backgroundColor: "{colors.surface}"
    textColor: "{colors.on-surface}"
    typography: "{typography.label-sm}"
    rounded: "{rounded.md}"
    padding: 10px
  popover:
    backgroundColor: "{colors.surface}"
    rounded: 16px
    padding: 18px
    width: 280px
  dialog:
    backgroundColor: "{colors.surface}"
    textColor: "{colors.on-surface}"
    rounded: "{rounded.xl}"
    padding: 28px
    width: 760px
  dialog-eyebrow:
    textColor: "{colors.eyebrow}"
    typography: "{typography.eyebrow}"
  dialog-subtitle:
    textColor: "{colors.dialog-subtitle}"
    typography: "{typography.label-sm-regular}"
  dialog-section:
    backgroundColor: "{colors.surface}"
    textColor: "{colors.on-surface}"
    rounded: 16px
    padding: 20px
  dialog-step-badge:
    backgroundColor: "{colors.step-badge-container}"
    textColor: "{colors.step-badge}"
    typography: "{typography.label-sm-bold}"
    rounded: 9px
    width: 28px
    height: 28px
  dialog-tip:
    backgroundColor: "{colors.tip-container}"
    textColor: "{colors.on-tip-container}"
    rounded: 10px
    padding: "12px 14px"
  switch:
    backgroundColor: "{colors.switch-off}"
    rounded: 20px
    width: 44px
    height: 26px
  switch-on:
    backgroundColor: "{colors.primary-deep}"
  option-card:
    backgroundColor: "{colors.surface}"
    textColor: "{colors.on-surface}"
    typography: "{typography.label-md}"
    rounded: "{rounded.md}"
    padding: 14px
    borderColor: "{colors.option-outline}"
  option-card-selected:
    backgroundColor: "{colors.primary-container}"
    textColor: "{colors.on-surface}"
    borderColor: "{colors.primary-deep-outline}"
  error-bar:
    backgroundColor: "{colors.error-container}"
    textColor: "{colors.error}"
    rounded: "{rounded.md}"
    padding: "{spacing.sm}"
  warning-notice:
    backgroundColor: "{colors.warning-container}"
    textColor: "{colors.on-warning-container}"
    typography: "{typography.body-md}"
    padding: 10px
---

# Hexyl Light

## 概要

Hexyl Light は、Hexyl のサービス・アプリケーションで共通に使うデザインシステムです。各プロダクトはこのファイルのトークンとルールに従って UI を組み立て、プロダクト間で見た目と操作感を揃えます。

インターフェースは「ダッシュボード」よりも「読むための面」として振る舞います。主役となるコンテンツを大きく落ち着いた面に置き、その周りのクロム（ヘッダー・ボタン・ステータス類）は控えめにします。

性格は **温かく、落ち着いていて、飾らない** こと。地はクリーム色のオフホワイトで、ほとんど見えないオレンジのドットパターン（22px グリッド、不透明度 10%）を敷き、空の画面でも無機質に見えないようにします。コンテンツは純白のカードに載せ、太い枠線ではなくコントラストの低いヘアラインで区切ります。オレンジは唯一のアクセントカラーで、ユーザーに操作してほしい場所にだけ使います。それ以外はクリーム地に温かみのあるグレーの文字で構成し、長時間見続けても疲れない画面にします。

**ダークモードはありません。** Hexyl Light は意図的にライトテーマ専用です。テーマを 2 つ持つと、すべての画面で両方の見た目を揃え続ける負担が倍になるためです。`prefers-color-scheme` の分岐は追加しないでください。

## カラー

パレットは温かみのあるニュートラルの階調ひとつと、オレンジのアクセントひとつ、そして用途を限定したステータスカラーで構成します。すべての色相が暖色寄りで、UI のどこにも冷たいグレーに見える部分を作りません。

- **Background (#faf6f0):** 温かいクリーム色。ページの地と、ダイアログ本文の地。
- **Surface (#ffffff) / Surface Dim (#f3ece2):** カード・パネル・サイドバー・ダイアログには純白を使う。Dim はセカンダリボタンの通常状態、ホバーしたリスト行、メーターのトラックに使う。
- **Outline (#ece3d6):** 1px の暖色ヘアライン。プロダクト全体で最も基本的な区切り。影を使う前にまずこれを使う。
- **On Surface (#14100a) / On Surface Dim (#857662) / Muted (#70614f):** 本文用のほぼ黒の暖色インク、メタデータや非アクティブなステータス用の明るい暖色グレー、そしてカードの見出しや確定前（処理中）のテキスト用の Muted。
- **Primary (#f08c26) / Primary Strong (#e0790f) / On Primary (#ffffff):** 操作用のオレンジとそのホバー段階。画面の主要な開始操作、最新位置へ戻るフローティングチップ、選択中の項目のアクセントに限って使う。
- **Primary Deep (#b9620e) / Primary Deep Outline (#ba650f):** 白の上でもアクセントとして埋もれない濃いオレンジ。フォーカスリング（3px）、オンのスイッチのトラック、ラジオボタンのアクセント、選択中のオプションカードの枠線に使う。
- **Primary Container (#fff5e8) / On Primary Container (#934900) / Eyebrow (#916029):** 選択状態の淡い背景、押下中のトグルボタンの文字色、ダイアログ上部の小さなアイブロウラベル。
- **Error (#d9544f) / Error Container (#fdf0ef):** 停止・削除などの危険な操作のボタン、エラーのステータス、インラインのエラーバー。
- **Warning Container (#fff3cd) / On Warning Container (#594500):** 注意を促すお知らせ帯にのみ使う。
- **Success (#2fae6e):** 「動作中」の表示にのみ使う — 点滅するステータスドット、動作中を示すステータス文字、レベルメーターの塗り。
- **Switch Off (#a99d8d) / Option Outline (#ded4c6):** コンポーネント専用のニュートラル。オフのスイッチのトラックと、未選択のオプションカードの枠線。
- **Dialog Subtitle (#6c5c49) / Step Badge (#995515) / Step Badge Container (#fff0dc) / Select Outline (#d9cebe) / Tip Container (#f7f2e9) / On Tip Container (#624b2f):** ダイアログ専用の色。ダイアログタイトル下の説明文、番号付きのステップバッジ、ダイアログ内のネイティブ select の枠線、末尾のヒント枠に使う。

## タイポグラフィ

書体はプラットフォームの UI フォントに日本語のフォールバックを加えた 1 種類だけを使います — `system-ui, Inter, 'Hiragino Sans', 'Noto Sans JP', sans-serif`。Web フォントは読み込みません。画面は即座に描画されるべきで、フォントの遅延読み込みで日本語と英字が混在するテキストがリフローしてはいけません。

- **読み物 (`reading-lg`, 22px/1.7):** 最も重要な階層。ユーザーがじっくり読む主役のテキストに使う。文字サイズを利用者が倍率で調整できるようにする場合は、並置されたすべての読み物の面に同じ倍率を掛ける。ウェイトはレギュラー、行間は広め — UI の文字ではなく、読むための文字。
- **見出し (`headline-md`, `headline-sm`, `title`):** 太字で、使う場所は少なく。ダイアログのタイトル、アプリのタイトル、セクション見出しに使う。行の高さは全体の 1.6 をそのまま継承し、詰めない。
- **ラベル (`label-md` 14px/700, `label-sm` 13px/600):** ボタンは `label-md`。ステータスピル、リストの行、フローティングチップは `label-sm`。
- **小さな文字 (`caption` 12px, `body-sm` 14px/400, `eyebrow` 11px/0.14em で大文字風, `card-title` 13px/0.04em):** 設定画面の補足説明、空の状態のプレースホルダー（`body-sm`）、ダイアログのアイブロウ、字間を広げたカード見出しに使う。文字に施す装飾は字間（letter-spacing）だけとする。

## レイアウト

アプリのシェルは `100dvh` の縦方向の flex カラムで、ヘッダー行の下にサイドバーとメイン領域を持つ行が続きます。ヘッダーは `min-height: 64px`、パディング `12px 24px` で、折り返さず、どの幅でも主要な操作ボタンに手が届くようにします。サイドバーは幅 `260px` 固定のカードで、外側に `24px` のマージンを取ります。メイン領域は `16px` のガター、`24px` のページパディングで配置します。

余白は 4px を基準とし、4/8/12/16/24px の段階を使います。`16px` はカード同士のガター、`24px` はページのマージンです。ダイアログの内部はフォーム部品が密集する面なので、やや大きめの独自のリズム（18〜28px）を使います。

ブレークポイントは 3 つです。

- **900px:** サイドバーは絶対配置のオーバーレイ（`min(300px, 100% - 32px)`）になり、暗くした背景の上にハンバーガーボタンから開く。ヘッダーの補助的なラベルやステータス文字は隠す。
- **600px:** 横に並んでいたカードは縦に積み、ページパディングは 12px に減らす。ブランドマークを隠して、操作ボタンが 1 行に収まるようにする。
- **540px:** ダイアログを詰める — パディングを縮め、オプションのグリッドは 1 列にし、アクション行の折り返しを許す。

同じ役割のカードを並べるときは、幅と高さを揃えて同じ重みで扱います。どちらか一方を従属的に見せてはいけません。

## エレベーションと奥行き

奥行きは影ではなく **トーンの重なりとヘアライン** で表現します。重なりは `background`（クリーム）→ `surface`（白いカード）→ `surface-dim`（カード内の一段沈んだコントロール）の順で、すべてのカードは `1px solid outline` の枠線で閉じます。クリーム地の上にヘアライン付きのフラットなカードを置くのが基本で、そこに影を足すのは誤りです。

影は、要素が実際にページの上に浮いている次の 4 か所にだけ使います。

- **ダイアログ:** `0 24px 90px #14100a35`。背景は `#21190d66` に `blur(3px)`。
- **ポップオーバー:** `0 12px 36px #14100a20`。
- **900px 未満のサイドバーオーバーレイ:** `0 12px 36px #14100a20`。背景は `#21190d55`。
- **スイッチのつまみ:** `0 1px 3px #0003`。

選択状態は浮かせるのではなく、色味とリングで表現します。選ばれたオプションカードは `primary-container` の塗り、`primary-deep-outline` の枠線、同色の `0 0 0 1px` のリングを持ちます。

## 形状

角は柔らかく一貫させ、角丸の大きさで要素の役割を表します。

- **ピル (`full`, 999px):** すべてのプライマリボタン、ステータスピル、フローティングチップ、メーター、ライブドット。操作ボタンは常に完全な角丸にする。
- **カード (`lg`, 18px):** カードとサイドバー — コンテンツを収める大きなコンテナ。
- **ダイアログ (`xl`, 22px):** モーダル。カードより一段柔らかくし、別の面として読めるようにする。
- **コントロール (`md`, 12px):** 小さなボタン、リストの行、エラーバー、オプションカード、ディスクロージャーのサマリー。ダイアログ内のブロックは 16px、アイコンボタンや小さな入力は 8〜9px。
- **セレクト:** ヘッダーなどのコントロールでは `10px`、ダイアログ内では `9px` — ネイティブの `<select>` 向けで、システム中で最も小さい角丸。

アイコンはテキストとして描画する絵文字を使います。アイコンライブラリは使わず、追加もしません。

## コンポーネント

### ボタン

`.btn` クラスのボタンは、ピル形状、`label-md` の文字、`8px 24px` のパディング、`0.15s ease` の背景トランジションを共有します。`button-primary`（オレンジ）は画面の主要な開始操作、`button-danger` は停止・削除などの危険な操作、`button-secondary`（Dim の塗りと枠線）はそれ以外すべてに使います。`button-toggle` はセカンダリボタンの一種で、`aria-pressed="true"` のときに `surface-dim` の塗り、`on-primary-container` の文字色、`primary` の枠線に切り替わります。

他のボタン系統はそれぞれの形状を持ちます。`button-compact` は角丸 `12px`、文字 `13px/700`、パディング `8px 12px`。数値を増減するステッパーボタンは同じ角丸 `12px` と `13px/700` の文字で、パディングを `4px 12px` に詰めます。アイコンボタン（`button-icon`）は角丸 `8px`、パディング `4px 8px` です。

無効状態は主に `opacity` で表します（ピルとステッパーは `0.5`、アイコンボタンは `0.4`）。ステッパーボタンは無効時に加えて文字色を `on-surface-dim` に落とします — それ以外に無効状態のための新しい色を作ってはいけません。

### ステータスとメーター

`status-pill` は枠線付きの白いピルです。ライブ版は文字と枠線を `success` 寄りに変え、先頭に 1.4 秒周期で点滅する 8px のドットを置きます。`meter-track` は `96px × 8px` 固定のピルで、長さそのものが値を表すため縮めてはいけません。`success` で塗り、`60ms linear` でアニメーションさせて、50ms ごとの更新にもカクつかずに追従させます。

### カードと読み物

`card` はヘアラインの枠線を持つ白いカードで、下側をヘアラインで区切った `card-title` の見出し行と、`16px 24px` のパディングを持つスクロールする本文で構成します。見出し行には、控えめな `muted` の見出しの隣に、内容を識別する `card-label`（`label-lg`、15px/700、`on-surface`）を添えることができます。

`reading-text` は `reading-lg` の文字で、下マージン `0.7em`、`overflow-wrap: anywhere` とします。確定前（処理中）のテキストには `muted` を使います。内容が空のときは `empty-placeholder` を表示します。

末尾に追記され続けるコンテンツを自動で追従する場合、スクロールの挙動は `smooth` ではなく `auto` にします — smooth スクロールは最新行への追従を壊します。最新位置へ戻る `floating-chip` は、カードの下端から `16px` の位置に中央揃えで浮かべます。

### ダイアログ

ネイティブの `<dialog>` を `min(760px, 100vw - 32px)` の幅で使い、ヘッダー（アイブロウ + タイトル + 説明文）、クリーム色の `background` 上でスクロールする本文、白い固定（sticky）のアクション行で構成します。内部では `dialog-section` のカードで関連するコントロールをまとめ、`switch` はオンのときに濃いオレンジを使い、ラジオ選択の `option-card`（パディング `14px 12px`）は 3 列のグリッドに並べて、小さな画面では 1 列にします。ダイアログ内のフォーカスも、アプリ全体と同じ `3px solid primary-deep` のリングを使います。

### お知らせ

`error-bar` はヘッダー直下に出す淡い赤のインライン帯、`warning-notice` は琥珀色の注意帯です。どちらもページマージンの内側で全幅に表示し、トーストやオーバーレイとしては出しません。

## するべきこと・してはいけないこと

- **する:** すべての値を各プロダクトの CSS カスタムプロパティ（`var(--accent)`、`var(--space-4)` など）から読む。**しない:** トークンで表せる hex や px の値をハードコードする。
- **する:** `primary` のオレンジは、画面の主要な開始操作・最新位置への復帰・選択中の項目など、ごく少数の操作に限る。**しない:** 補助的なクロムをオレンジに塗る。オレンジのボタンが 2 つある画面は 1 つ多い。
- **する:** 同じ役割で並置するカードは、幅・高さ・角丸・文字サイズを揃え、文字サイズの倍率も同じものを掛ける。**しない:** 並置したカードの一方だけを視覚的に優先させる。
- **する:** 面同士は `1px solid var(--outline)` とトーンの段差で区切る。**しない:** ダイアログ・ポップオーバー・モバイルのサイドバーオーバーレイ・スイッチのつまみ以外に影を付ける。
- **する:** フォーカス可能なすべての要素に、オフセット `3px`（スクロールコンテナ内では `-3px`）の `3px solid #b9620e` のフォーカスリングを付ける。**しない:** レイアウトを整えるためにフォーカスのアウトラインを消す。
- **する:** トランジションは色が `0.15s ease`、サイドバーの形状が `0.2s ease`、メーターが `60ms linear`。**しない:** レイアウトをアニメーションさせたり、登場時のモーションを加えたりする。すべてのアニメーションは `prefers-reduced-motion: reduce` で止められるようにする。
- **する:** アイコンには絵文字を使い、タッチ環境ではアイコンボタンのヒット領域を最低 40px にする。**しない:** アイコンフォントや SVG アイコンライブラリを導入する。
- **する:** ライトテーマで設計する。**しない:** ダークテーマ、`prefers-color-scheme` のブロック、テーマ切り替えを追加する。
- **する:** 無効状態は主に `opacity`（0.4〜0.5）で表す。**しない:** そのための新しいグレーを作る — 唯一認められる例外は、`on-surface-dim` にも落とすステッパーボタン。
- **する:** ヘッダーはどの幅でも 1 行に保ち、優先度の低いラベル（補助ラベル、ヘッダーのステータス、ブランドマーク）から隠す。**しない:** 主要な操作ボタンを折り返したり移動させたりする。
- **する:** カードの見出しで内容を明示し、見出し自体は控えめに保つ（`muted`、字間を広げた 13px）。**しない:** クロムを主役のコンテンツと注目を奪い合わせる。
