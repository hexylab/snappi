//! カメラの振る舞い仕様のパラメータ (Issue #28, #39)。
//!
//! 値の意味と根拠は `docs/camera-spec-v6.md` を参照（v5 から引き継いだ値は `docs/old/camera-spec-v5.md`）。仕様書と値を変えるときは両方を更新すること。

/// ズーム強さ「控えめ」の倍率
pub const ZOOM_SUBTLE: f64 = 1.5;
/// ズーム強さ「標準」の倍率（既定）
pub const ZOOM_STANDARD: f64 = 1.8;
/// ズーム強さ「強め」の倍率
pub const ZOOM_STRONG: f64 = 2.2;

/// R2: この間隔未満のアクティビティは1つの区間にまとめる
pub const MERGE_GAP_MS: u64 = 3000;
/// R3: 最初のアクティビティのこの時間前に寄り始める
pub const LEAD_MS: u64 = 600;
/// R3: 最後のアクティビティからこの時間後に引き始める
pub const HOLD_MS: u64 = 1500;
/// R4: 区間の最短長
pub const MIN_SEGMENT_MS: u64 = 2000;
/// R4: 区間同士の隙間がこれ未満なら結合する
pub const MIN_GAP_MS: u64 = 1500;
/// R5: 録画開始からこの時間内のアクティビティは Snappi 自身の操作とみなして無視する
pub const START_GUARD_MS: u64 = 1000;
/// R5: 録画終了前のこの時間内のアクティビティは Snappi 自身の操作とみなして無視する
pub const END_GUARD_MS: u64 = 1500;
/// R1: クリックからリリースまでにこの距離 (px) 以上動いたらドラッグとみなす
pub const DRAG_MIN_DISTANCE: f64 = 10.0;

/// ズームイン（1.0 → Z）にかける時間。LEAD_MS 以下にして M7 を満たす
pub const ZOOM_IN_MS: u64 = 500;
/// ズームアウト（Z → 1.0）にかける時間
pub const ZOOM_OUT_MS: u64 = 700;

/// F2: パンは臨界減衰バネで追従する。その半減期 (ms)
pub const PAN_HALF_LIFE_MS: u64 = 200;
/// F2/F5: パンの速度上限（画面幅/秒）。M5 の基準より余裕を持たせる
pub const MAX_PAN_SPEED: f64 = 2.5;
/// F2: デッドゾーンの判定に使う注目点の先読み時間 (ms)。バネの遅れを補う
pub const PAN_LOOKAHEAD_MS: u64 = 300;

/// F2: デッドゾーンの大きさ（ビューポートの幅・高さに対する比率。中央 60%）
pub const DEAD_ZONE_RATIO: f64 = 0.6;
/// F5: 注目点がビューポート幅のこの倍数以上離れたら「遠い移動」とみなす
pub const FAR_JUMP_VIEWPORTS: f64 = 1.5;
/// F5: 遠い移動の後、次のアクティビティまでこの時間以上あれば、いったん引いてから寄る
pub const REZOOM_MIN_MS: u64 = 2500;

/// メトリクス上で「寄っている」とみなす zoom の下限
pub const ZOOMED_THRESHOLD: f64 = 1.25;

// ---- v6: 主役とショット (Issue #39) ----

/// S1: 操作からこの時間内に起きた画面の変化を、その操作の結果（反応）とみなす
pub const RESPONSE_MS: u64 = 1200;
/// S1: 変化したセルがこれ未満の変化領域は使わない（細かいノイズ）
pub const MIN_CHANGE_CELLS: u32 = 3;
/// S1: 主役に含める変化領域の、操作位置・主役からの距離 (px)
pub const NEAR_PX: f64 = 160.0;
/// S1: 反応の中で最大の変化に比べてセル数がこの比率未満の小さな片は、主役に含めない
pub const FRAGMENT_RATIO: f64 = 0.03;
/// S1: これ以上のセルが変化した領域は、操作位置から遠くても反応とみなす（ダイアログ・メニュー等）
pub const BIG_CHANGE_CELLS: u32 = 300;
/// S1: 操作の無い時間（前後この時間に操作が無い）に変化する場所は、常に動いている場所
/// （キャレットの点滅・時計等）とみなして主役から除く
pub const AMBIENT_IDLE_MS: u64 = 1500;
/// S1: 常に動いている場所とみなす変化領域のセル数の上限
pub const AMBIENT_MAX_CELLS: u32 = 30;

/// S3: 主役の周りに残す余白（フレームの幅・高さに対する比率）
pub const SUBJECT_MARGIN: f64 = 0.025;
/// S3: 主役が収まる倍率がこれ未満なら寄らない（全体表示）
pub const MIN_SHOT_ZOOM: f64 = 1.25;
/// S3: 主役をまとめると倍率がこの比率より下がるなら、まとめずに次のショットへ移る
pub const SHOT_ZOOM_KEEP: f64 = 0.85;
/// S4: 主役のこの割合以上が今の枠に入っていれば、枠を動かさない（少し動いた程度では動かさない）
pub const SHOT_COVER_RATIO: f64 = 0.7;
/// S4: 同じ場所の操作なら、間隔がこの時間未満までは場面を続ける（寄ったまま）
pub const SCENE_BRIDGE_MS: u64 = 8000;
/// S4: ショットを延ばすとき、反応が最後に見えた時刻からこの時間は映し続ける
pub const RESPONSE_HOLD_MS: u64 = 800;
/// S5: ウィンドウの矩形が主役をこの距離 (px) の誤差で含んでいれば、枠をウィンドウ内に収める
pub const WINDOW_TOLERANCE_PX: f64 = 24.0;

/// F6: ショット間の移動（パン・ズーム）にかける最長時間
pub const SHOT_MOVE_MAX_MS: u64 = 1200;

/// 自動トリム（T1）: 録画の長さがこれ未満ならトリムしない
pub const AUTO_TRIM_MIN_DURATION_MS: u64 = 5000;
/// 自動トリム（T1）: 録画開始からこの時間内の大きな画面の変化（Snappi のウィンドウが消える等）が終わるまでを切る
pub const AUTO_TRIM_START_SETTLE_MS: u64 = 2500;
