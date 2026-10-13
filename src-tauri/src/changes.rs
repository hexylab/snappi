//! 画面の変化領域 (Issue #39)。
//!
//! 隣り合う録画フレームの差分をセル単位で求め、まとまって変化した領域を矩形にして
//! `changes.jsonl` に保存する。カメラはこれを「画面の中で変化が起きている場所」（主役）の
//! 主な手がかりにする（仕様 `docs/camera-spec-v6.md` §3）。
//!
//! - 1行 = 1フレーム: `{"t":1124,"rects":[[l,t,r,b,cells],...]}`。`t` はそのフレームの録画時刻、
//!   矩形はひとつ前のフレームから変化した領域（キャプチャ領域基準の px）、`cells` は変化したセル数。
//!   変化の無いフレームは書かない。
//! - フィルタ（ノイズの除去や操作との対応付け）はしない生の信号。使う側（`camera::subject`）で選ぶ。
//! - frames は大きく評価データにコミットしないため、評価・カメラ計算はこのファイルだけを使う。

use crate::recording_data::{frame_path, read_frame_count};
use crate::config::RecordingMeta;
use anyhow::{Context, Result};
use image::GrayImage;
use rayon::prelude::*;
use serde::{Deserialize, Serialize};
use std::io::Write;
use std::path::Path;

pub const CHANGES_FILE: &str = "changes.jsonl";

/// セルの大きさ（フレームの px）
const CELL: u32 = 16;
/// セル内はこの間隔で画素を間引いて比べる
const SAMPLE_STEP: u32 = 2;
/// 輝度の差がこれより大きい画素を「変化した」とみなす（JPEG のノイズを除く）
const PIXEL_THRESHOLD: u8 = 24;
/// 変化した画素がセル内でこの数以上あればセルが変化したとみなす
const CELL_MIN_PIXELS: u32 = 2;
/// 変化したセル同士がこのセル数以内の距離なら同じ領域にまとめる
const JOIN_CELLS: i32 = 2;
/// 一度にデコードするフレーム数（メモリを抑えるため）
const CHUNK: usize = 32;

/// 1フレームの変化
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct FrameChange {
    pub t: u64,
    pub rects: Vec<ChangeRect>,
}

/// 変化した領域。JSON では `[l, t, r, b, cells]`
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
#[serde(from = "[i64; 5]", into = "[i64; 5]")]
pub struct ChangeRect {
    pub rect: Rect,
    /// 変化したセルの数（領域の「量」。矩形の面積とは別）
    pub cells: u32,
}

impl From<[i64; 5]> for ChangeRect {
    fn from(a: [i64; 5]) -> Self {
        Self { rect: Rect::new(a[0] as f64, a[1] as f64, a[2] as f64, a[3] as f64), cells: a[4].max(0) as u32 }
    }
}

/// 座標は 1px 未満を丸めて保存する
impl From<ChangeRect> for [i64; 5] {
    fn from(c: ChangeRect) -> Self {
        let r = c.rect;
        [r.l.round() as i64, r.t.round() as i64, r.r.round() as i64, r.b.round() as i64, c.cells as i64]
    }
}

/// 軸に平行な矩形 `[l, r] x [t, b]`（キャプチャ領域基準の px）
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Rect {
    pub l: f64,
    pub t: f64,
    pub r: f64,
    pub b: f64,
}

impl Rect {
    pub fn new(l: f64, t: f64, r: f64, b: f64) -> Self {
        Self { l, t, r, b }
    }

    pub fn from_array(a: [f64; 4]) -> Self {
        Self::new(a[0], a[1], a[2], a[3])
    }

    pub fn point(x: f64, y: f64) -> Self {
        Self::new(x, y, x, y)
    }

    pub fn width(&self) -> f64 {
        self.r - self.l
    }

    pub fn height(&self) -> f64 {
        self.b - self.t
    }

    pub fn area(&self) -> f64 {
        self.width().max(0.0) * self.height().max(0.0)
    }

    pub fn center(&self) -> (f64, f64) {
        ((self.l + self.r) / 2.0, (self.t + self.b) / 2.0)
    }

    pub fn union(&self, o: &Rect) -> Rect {
        Rect::new(self.l.min(o.l), self.t.min(o.t), self.r.max(o.r), self.b.max(o.b))
    }

    pub fn intersect(&self, o: &Rect) -> Option<Rect> {
        let r = Rect::new(self.l.max(o.l), self.t.max(o.t), self.r.min(o.r), self.b.min(o.b));
        (r.l <= r.r && r.t <= r.b).then_some(r)
    }

    /// 各辺を `m` px 外側に広げた矩形
    pub fn inflate(&self, m: f64) -> Rect {
        Rect::new(self.l - m, self.t - m, self.r + m, self.b + m)
    }

    pub fn contains_point(&self, x: f64, y: f64) -> bool {
        x >= self.l && x <= self.r && y >= self.t && y <= self.b
    }

    /// `o` が（`tol` px の誤差を許して）内側にあるか
    pub fn contains(&self, o: &Rect, tol: f64) -> bool {
        o.l >= self.l - tol && o.t >= self.t - tol && o.r <= self.r + tol && o.b <= self.b + tol
    }
}

/// 録画ディレクトリの changes.jsonl を読む。無ければ空。
pub fn load(dir: &Path) -> Vec<FrameChange> {
    let Ok(content) = std::fs::read_to_string(dir.join(CHANGES_FILE)) else {
        return Vec::new();
    };
    let mut out: Vec<FrameChange> = content
        .lines()
        .map(str::trim)
        .filter(|l| !l.is_empty())
        .filter_map(|l| serde_json::from_str(l).ok())
        .collect();
    out.sort_by_key(|c| c.t);
    out
}

/// changes.jsonl が無く、frames がある録画なら作る。作った（または既にある）なら true。
pub fn ensure(dir: &Path) -> Result<bool> {
    if dir.join(CHANGES_FILE).exists() {
        return Ok(true);
    }
    if !dir.join("frames").is_dir() {
        return Ok(false);
    }
    generate(dir)?;
    Ok(true)
}

/// 録画のフレームから changes.jsonl を作る（既存のファイルは上書きする）。
pub fn generate(dir: &Path) -> Result<()> {
    let meta: RecordingMeta = serde_json::from_str(&std::fs::read_to_string(dir.join("meta.json"))?)?;
    let data = crate::recording_data::RecordingData::load(dir)?;
    let frames_dir = dir.join("frames");
    let count = read_frame_count(dir, &meta) as usize;
    let started = std::time::Instant::now();

    let mut changes = Vec::new();
    let mut prev: Option<GrayImage> = None;
    for chunk_start in (0..count).step_by(CHUNK) {
        let idx: Vec<usize> = (chunk_start..(chunk_start + CHUNK).min(count)).collect();
        let lumas: Vec<Option<GrayImage>> = idx
            .par_iter()
            .map(|&i| image::open(frame_path(&frames_dir, i as u64)).ok().map(|img| img.to_luma8()))
            .collect();
        let pairs: Vec<(usize, Option<&GrayImage>, Option<&GrayImage>)> = idx
            .iter()
            .enumerate()
            .map(|(j, &i)| {
                let a = if j == 0 { prev.as_ref() } else { lumas[j - 1].as_ref() };
                (i, a, lumas[j].as_ref())
            })
            .collect();
        let found: Vec<Option<FrameChange>> = pairs
            .par_iter()
            .map(|&(i, a, b)| {
                let (a, b) = (a?, b?);
                let scale = b.width() as f64 / data.width().max(1) as f64;
                let rects = diff_regions(a, b, scale);
                (!rects.is_empty()).then(|| FrameChange { t: data.frame_times.get(i).copied().unwrap_or(0), rects })
            })
            .collect();
        changes.extend(found.into_iter().flatten());
        prev = lumas.into_iter().last().flatten();
    }

    let path = dir.join(CHANGES_FILE);
    let tmp = path.with_extension("jsonl.tmp");
    {
        let mut w = std::io::BufWriter::new(std::fs::File::create(&tmp)?);
        for c in &changes {
            writeln!(w, "{}", serde_json::to_string(c)?)?;
        }
        w.flush()?;
    }
    std::fs::rename(&tmp, &path).with_context(|| format!("{} を書けません", path.display()))?;
    log::info!(
        "changes.jsonl: {} フレーム中 {} フレームに変化 ({:.1}s)",
        count,
        changes.len(),
        started.elapsed().as_secs_f64()
    );
    Ok(())
}

/// 2つのフレーム（輝度）の変化領域。`scale` はフレームの px / キャプチャ領域の px。
/// 大きさの違うフレーム（ウィンドウ録画でのリサイズ）は全体が変化したとみなす。
pub fn diff_regions(a: &GrayImage, b: &GrayImage, scale: f64) -> Vec<ChangeRect> {
    let (w, h) = (b.width(), b.height());
    if a.dimensions() != b.dimensions() {
        let cells = w.div_ceil(CELL) * h.div_ceil(CELL);
        return vec![ChangeRect { rect: Rect::new(0.0, 0.0, w as f64 / scale, h as f64 / scale), cells }];
    }
    let (gw, gh) = (w.div_ceil(CELL), h.div_ceil(CELL));
    let mut grid = vec![false; (gw * gh) as usize];
    let (ra, rb) = (a.as_raw(), b.as_raw());
    for cy in 0..gh {
        for cx in 0..gw {
            let mut n = 0;
            'cell: for y in (cy * CELL..((cy + 1) * CELL).min(h)).step_by(SAMPLE_STEP as usize) {
                let row = (y * w) as usize;
                for x in (cx * CELL..((cx + 1) * CELL).min(w)).step_by(SAMPLE_STEP as usize) {
                    let i = row + x as usize;
                    if ra[i].abs_diff(rb[i]) > PIXEL_THRESHOLD {
                        n += 1;
                        if n >= CELL_MIN_PIXELS {
                            break 'cell;
                        }
                    }
                }
            }
            grid[(cy * gw + cx) as usize] = n >= CELL_MIN_PIXELS;
        }
    }
    cell_regions(&grid, gw as i32, gh as i32)
        .into_iter()
        .map(|(x0, y0, x1, y1, cells)| {
            let px = |c: i32, max: u32| ((c as u32 * CELL).min(max)) as f64 / scale;
            ChangeRect { rect: Rect::new(px(x0, w), px(y0, h), px(x1 + 1, w), px(y1 + 1, h)), cells }
        })
        .collect()
}

/// 変化したセルを `JOIN_CELLS` 以内の距離でつなげた連結成分の外接矩形 `(x0, y0, x1, y1, セル数)`（セル単位・両端含む）
fn cell_regions(grid: &[bool], gw: i32, gh: i32) -> Vec<(i32, i32, i32, i32, u32)> {
    let mut seen = vec![false; grid.len()];
    let mut out = Vec::new();
    for start in 0..grid.len() {
        if !grid[start] || seen[start] {
            continue;
        }
        seen[start] = true;
        let mut stack = vec![start];
        let (mut x0, mut y0, mut x1, mut y1, mut n) = (i32::MAX, i32::MAX, i32::MIN, i32::MIN, 0u32);
        while let Some(i) = stack.pop() {
            let (x, y) = (i as i32 % gw, i as i32 / gw);
            x0 = x0.min(x);
            y0 = y0.min(y);
            x1 = x1.max(x);
            y1 = y1.max(y);
            n += 1;
            for dy in -JOIN_CELLS..=JOIN_CELLS {
                for dx in -JOIN_CELLS..=JOIN_CELLS {
                    let (nx, ny) = (x + dx, y + dy);
                    if nx < 0 || ny < 0 || nx >= gw || ny >= gh {
                        continue;
                    }
                    let j = (ny * gw + nx) as usize;
                    if grid[j] && !seen[j] {
                        seen[j] = true;
                        stack.push(j);
                    }
                }
            }
        }
        out.push((x0, y0, x1, y1, n));
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use image::Luma;

    fn img(w: u32, h: u32) -> GrayImage {
        GrayImage::from_pixel(w, h, Luma([40]))
    }

    fn paint(img: &mut GrayImage, x0: u32, y0: u32, x1: u32, y1: u32) {
        for y in y0..y1 {
            for x in x0..x1 {
                img.put_pixel(x, y, Luma([220]));
            }
        }
    }

    #[test]
    fn test_no_change_no_regions() {
        assert!(diff_regions(&img(160, 160), &img(160, 160), 1.0).is_empty());
    }

    #[test]
    fn test_separate_regions_and_join_distance() {
        let a = img(320, 160);
        let mut b = img(320, 160);
        // 距離 JOIN_CELLS 以内の変化は同じ領域、遠い変化は別の領域
        paint(&mut b, 0, 0, 10, 10); // セル (0,0)
        paint(&mut b, 32, 0, 42, 10); // セル (2,0)（距離 2 なのでつながる）
        paint(&mut b, 290, 130, 300, 140); // セル (18,8)
        let mut r = diff_regions(&a, &b, 1.0);
        r.sort_by(|p, q| p.rect.l.total_cmp(&q.rect.l));
        assert_eq!(r.len(), 2);
        assert_eq!(r[0], ChangeRect { rect: Rect::new(0.0, 0.0, 48.0, 16.0), cells: 2 });
        assert_eq!(r[1], ChangeRect { rect: Rect::new(288.0, 128.0, 304.0, 144.0), cells: 1 });
    }

    #[test]
    fn test_scale_to_capture_coords_and_resize() {
        let a = img(64, 64);
        let mut b = img(64, 64);
        paint(&mut b, 32, 32, 40, 40);
        // フレームがキャプチャ領域の 2 倍の解像度
        assert_eq!(diff_regions(&a, &b, 2.0)[0].rect, Rect::new(16.0, 16.0, 24.0, 24.0));
        // 大きさが変わったら全体
        let all = diff_regions(&img(32, 32), &b, 1.0);
        assert_eq!(all[0].rect, Rect::new(0.0, 0.0, 64.0, 64.0));
    }

    #[test]
    fn test_small_noise_is_ignored() {
        let a = img(32, 32);
        let mut b = img(32, 32);
        b.put_pixel(0, 0, Luma([60])); // 差が小さい
        b.put_pixel(4, 4, Luma([255])); // 1画素だけ
        assert!(diff_regions(&a, &b, 1.0).is_empty());
    }

    #[test]
    fn test_json_format() {
        let c = FrameChange { t: 5, rects: vec![ChangeRect { rect: Rect::new(1.0, 2.0, 3.0, 4.0), cells: 7 }] };
        let s = serde_json::to_string(&c).unwrap();
        assert_eq!(s, r#"{"t":5,"rects":[[1,2,3,4,7]]}"#);
        assert_eq!(serde_json::from_str::<FrameChange>(&s).unwrap(), c);
    }
}
