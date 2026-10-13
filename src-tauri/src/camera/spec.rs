//! カメラの振る舞い仕様のパラメータ (Issue #28)。
//!
//! 値の意味と根拠は `docs/camera-spec-v5.md` を参照。仕様書と値を変えるときは両方を更新すること。

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

/// F2: デッドゾーンの大きさ（ビューポートの幅・高さに対する比率。中央 60%）
pub const DEAD_ZONE_RATIO: f64 = 0.6;
/// F5: 注目点がビューポート幅のこの倍数以上離れたら「遠い移動」とみなす
pub const FAR_JUMP_VIEWPORTS: f64 = 1.5;
/// F5: 遠い移動の後、次のアクティビティまでこの時間以上あれば、いったん引いてから寄る
pub const REZOOM_MIN_MS: u64 = 2500;

/// メトリクス上で「寄っている」とみなす zoom の下限
pub const ZOOMED_THRESHOLD: f64 = 1.25;
